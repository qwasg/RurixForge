package accounts

import (
	"context"
	"net/http"
	"strconv"
	"time"

	"github.com/redis/go-redis/v9"

	"forge-cloud/internal/core"
	"forge-cloud/internal/oauth/openai"
)

const (
	refreshLockTTL   = 60 * time.Second
	refreshLockWait  = 15 * time.Second
	refreshFailLimit = 3
	// 距 access token 过期不足该时长时，网关先刷新再发请求。
	refreshAhead = 2 * time.Minute
)

type refreshMode int

const (
	refreshForce   refreshMode = iota // 管理端「立即刷新」
	refreshIfStale                    // 上游 401 / 临近过期：token 已被别处刷新则跳过
	refreshIfDue                      // worker：仍满足扫描条件才刷新
)

func refreshLockKey(id int64) string { return "lock:acct-refresh:" + strconv.FormatInt(id, 10) }

var unlockScript = redis.NewScript(`
if redis.call('GET', KEYS[1]) == ARGV[1] then
  return redis.call('DEL', KEYS[1])
end
return 0
`)

// lockRefresh 获取账号刷新锁（多实例/并发请求互斥：OpenAI refresh token 轮换后旧值即失效）。
// 被占用时等待持有者释放后再抢，最多等 refreshLockWait。
func (s *Service) lockRefresh(ctx context.Context, id int64) (func(), bool, error) {
	key := refreshLockKey(id)
	token := core.RandomString(20)
	deadline := time.Now().Add(refreshLockWait)
	for {
		ok, err := s.rdb.SetNX(ctx, key, token, refreshLockTTL).Result()
		if err != nil {
			return nil, false, err
		}
		if ok {
			return func() { _ = unlockScript.Run(context.WithoutCancel(ctx), s.rdb, []string{key}, token).Err() }, true, nil
		}
		if time.Now().After(deadline) {
			return nil, false, nil
		}
		select {
		case <-ctx.Done():
			return nil, false, ctx.Err()
		case <-time.After(150 * time.Millisecond):
		}
	}
}

func refreshDue(a *Account, now time.Time) bool {
	if a.TokenExpiresAt != nil && a.TokenExpiresAt.Before(now.Add(24*time.Hour)) {
		return true
	}
	last := a.CreatedAt
	if a.LastRefreshAt != nil {
		last = *a.LastRefreshAt
	}
	return last.Before(now.Add(-7 * 24 * time.Hour))
}

// refresh 在刷新锁内重新读取账号并按 mode 决定是否刷新。
// 一旦向 token 端点发出请求就必须落库新 token（旧 refresh token 会失效），所以不跟随调用方取消。
func (s *Service) refresh(ctx context.Context, id int64, mode refreshMode, staleAccessToken string) (*Account, *Credentials, error) {
	ctx = context.WithoutCancel(ctx)
	unlock, ok, err := s.lockRefresh(ctx, id)
	if err != nil {
		return nil, nil, err
	}
	if !ok {
		return nil, nil, core.E(http.StatusConflict, "REFRESH_IN_PROGRESS", "该账号正在刷新 token，请稍后重试")
	}
	defer unlock()

	a, err := s.Get(ctx, id)
	if err != nil {
		return nil, nil, err
	}
	if a.AuthType != AuthOAuth {
		return nil, nil, core.BadRequest("NOT_SUPPORTED", "只有 OAuth 账号可以刷新 token")
	}
	creds, err := s.Credentials(a)
	if err != nil {
		return nil, nil, err
	}
	switch mode {
	case refreshIfStale:
		if creds.AccessToken != staleAccessToken {
			return a, creds, nil
		}
	case refreshIfDue:
		if a.Status != StatusActive || !refreshDue(a, time.Now()) {
			return a, creds, nil
		}
	}
	if creds.RefreshToken == "" {
		msg := "没有 refresh token，无法刷新"
		s.recordRefreshFailure(ctx, a.ID, msg)
		return nil, nil, core.E(http.StatusBadGateway, "REFRESH_FAILED", msg)
	}
	client, err := s.oauthClient(a.ProxyURL)
	if err != nil {
		return nil, nil, err
	}
	tctx, cancel := context.WithTimeout(ctx, 30*time.Second)
	tok, err := client.Refresh(tctx, creds.RefreshToken)
	cancel()
	if err != nil {
		msg := "刷新 token 失败：" + err.Error()
		s.recordRefreshFailure(ctx, a.ID, msg)
		return nil, nil, core.E(http.StatusBadGateway, "REFRESH_FAILED", msg)
	}

	next := *creds
	next.AccessToken = tok.AccessToken
	if tok.RefreshToken != "" {
		next.RefreshToken = tok.RefreshToken
	}
	if tok.IDToken != "" {
		next.IDToken = tok.IDToken
	}
	ident := openai.IdentityFromTokens(next.IDToken, next.AccessToken, tok.ExpiresIn)
	if ident.AccountID != "" {
		next.AccountID = ident.AccountID
	}
	next.ExpiresAt = ident.ExpiresAt
	sealed, err := s.vault.EncryptJSON(next)
	if err != nil {
		return nil, nil, err
	}
	if _, err := s.db.Exec(ctx, `UPDATE upstream_accounts SET credentials = $2, token_expires_at = $3,
		last_refresh_at = now(), fail_count = 0, last_error = '',
		status = CASE WHEN status = 'error' THEN 'active' ELSE status END,
		email = CASE WHEN $4 <> '' THEN $4 ELSE email END,
		plan_type = CASE WHEN $5 <> '' THEN $5 ELSE plan_type END,
		updated_at = now()
		WHERE id = $1`, a.ID, sealed, next.ExpiresAt, ident.Email, ident.PlanType); err != nil {
		s.log.Error("persist refreshed token failed", "account", a.ID, "err", err)
		return nil, nil, err
	}
	updated, err := s.Get(ctx, a.ID)
	if err != nil {
		return nil, nil, err
	}
	return updated, &next, nil
}

// recordRefreshFailure：fail_count+1，连续失败 refreshFailLimit 次置 error。
func (s *Service) recordRefreshFailure(ctx context.Context, id int64, msg string) {
	if _, err := s.db.Exec(ctx, `UPDATE upstream_accounts SET fail_count = fail_count + 1,
		last_error = $2, last_error_at = now(),
		status = CASE WHEN status = 'active' AND fail_count + 1 >= $3 THEN 'error' ELSE status END,
		updated_at = now()
		WHERE id = $1`, id, truncate(msg, 500), refreshFailLimit); err != nil {
		s.log.Error("record refresh failure", "account", id, "err", err)
	}
}

// RefreshAccount 立即刷新 OAuth 账号 token（管理端「刷新」）。
func (s *Service) RefreshAccount(ctx context.Context, id int64) (*Account, error) {
	a, _, err := s.refresh(ctx, id, refreshForce, "")
	return a, err
}

// RefreshAfterUnauthorized 在上游 401 时调用：若 token 已被其它请求刷新则直接返回新凭据，否则刷新。
func (s *Service) RefreshAfterUnauthorized(ctx context.Context, a *Account, staleAccessToken string) (*Credentials, error) {
	_, c, err := s.refresh(ctx, a.ID, refreshIfStale, staleAccessToken)
	return c, err
}

// FreshCredentials 解密凭据；OAuth access token 临近过期时先刷新（刷新失败但尚未过期则沿用旧 token）。
func (s *Service) FreshCredentials(ctx context.Context, a *Account) (*Credentials, error) {
	c, err := s.Credentials(a)
	if err != nil {
		return nil, err
	}
	if a.AuthType != AuthOAuth || c.ExpiresAt == nil || time.Until(*c.ExpiresAt) > refreshAhead {
		return c, nil
	}
	_, nc, err := s.refresh(ctx, a.ID, refreshIfStale, c.AccessToken)
	if err != nil {
		if time.Until(*c.ExpiresAt) > 0 {
			return c, nil
		}
		return nil, err
	}
	return nc, nil
}

// RefreshDue 刷新全部到期的 OAuth 账号（token 24 小时内过期或 7 天未刷新），返回成功/失败数。
func (s *Service) RefreshDue(ctx context.Context) (ok, failed int) {
	rows, err := s.db.Query(ctx, `SELECT id FROM upstream_accounts
		WHERE auth_type = 'oauth' AND status = 'active'
		  AND ((token_expires_at IS NOT NULL AND token_expires_at < now() + interval '24 hours')
		       OR COALESCE(last_refresh_at, created_at) < now() - interval '7 days')
		ORDER BY id`)
	if err != nil {
		s.log.Error("refresh scan failed", "err", err)
		return 0, 0
	}
	var ids []int64
	for rows.Next() {
		var id int64
		if err := rows.Scan(&id); err == nil {
			ids = append(ids, id)
		}
	}
	rows.Close()
	for _, id := range ids {
		if ctx.Err() != nil {
			break
		}
		if _, _, err := s.refresh(ctx, id, refreshIfDue, ""); err != nil {
			failed++
			s.log.Warn("oauth refresh failed", "account", id, "err", err)
			continue
		}
		ok++
	}
	return ok, failed
}

// StartWorkers 启动后台 worker（OAuth token 刷新，每 5 分钟一轮），ctx 取消时退出。
func (s *Service) StartWorkers(ctx context.Context) {
	go func() {
		t := time.NewTicker(s.refreshEvery)
		defer t.Stop()
		for {
			if ctx.Err() != nil {
				return
			}
			s.RefreshDue(ctx)
			select {
			case <-ctx.Done():
				return
			case <-t.C:
			}
		}
	}()
}
