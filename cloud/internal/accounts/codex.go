package accounts

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"os"
	"strings"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/redis/go-redis/v9"

	"forge-cloud/internal/audit"
	"forge-cloud/internal/core"
	"forge-cloud/internal/httpx"
	"forge-cloud/internal/oauth/openai"
)

// ImportOptions 是导入/授权新建账号时的属性；更新已有账号时只替换凭据、补充分组（Name 非空才改名）。
type ImportOptions struct {
	Name             string
	GroupIDs         []int64
	Priority         int
	ConcurrencyLimit int
	ProxyURL         string
}

// OAuthTokens 是一组 Codex 订阅 token（AccountID/Email 为空时取 token 声明）。
type OAuthTokens struct {
	IDToken      string
	AccessToken  string
	RefreshToken string
	AccountID    string
	Email        string
	ExpiresIn    int64
	LastRefresh  *time.Time
}

// UpsertOAuthAccount 按 chatgpt_account_id（external_id）新建或更新 openai+oauth 账号，返回账号与是否新建。
func (s *Service) UpsertOAuthAccount(ctx context.Context, t OAuthTokens, opt ImportOptions) (*Account, bool, error) {
	if t.AccessToken == "" && t.RefreshToken == "" {
		return nil, false, core.BadRequest("INVALID_AUTH_JSON", "缺少 access_token / refresh_token")
	}
	ident := openai.IdentityFromTokens(t.IDToken, t.AccessToken, t.ExpiresIn)
	accountID := firstNonEmpty(ident.AccountID, t.AccountID)
	if accountID == "" {
		return nil, false, core.BadRequest("INVALID_AUTH_JSON", "token 里没有 ChatGPT 账号 ID（chatgpt_account_id）")
	}
	email := firstNonEmpty(ident.Email, t.Email)
	name := strings.TrimSpace(opt.Name)
	explicitName := name != ""
	if name == "" {
		name = email
	}
	if name == "" {
		name = "codex-" + truncateASCII(accountID, 8)
	}
	if err := validName(name); err != nil {
		return nil, false, err
	}
	opt.ProxyURL = strings.TrimSpace(opt.ProxyURL)
	if err := validateCommon(opt.Priority, 1, opt.ConcurrencyLimit, opt.ProxyURL, nil); err != nil {
		return nil, false, err
	}
	groups, err := s.resolveGroups(ctx, opt.GroupIDs, true)
	if err != nil {
		return nil, false, err
	}
	sealed, err := s.vault.EncryptJSON(Credentials{
		AccessToken: t.AccessToken, RefreshToken: t.RefreshToken, IDToken: t.IDToken,
		AccountID: accountID, ExpiresAt: ident.ExpiresAt,
	})
	if err != nil {
		return nil, false, err
	}
	var id int64
	var inserted bool
	err = pgx.BeginFunc(ctx, s.db, func(tx pgx.Tx) error {
		if err := tx.QueryRow(ctx, `INSERT INTO upstream_accounts (name, platform, auth_type, credentials, external_id,
			email, plan_type, supports_responses, proxy_url, priority, concurrency_limit, token_expires_at, last_refresh_at)
			VALUES ($1, 'openai', 'oauth', $2, $3, $4, $5, TRUE, $6, $7, $8, $9, $10)
			ON CONFLICT (platform, auth_type, external_id) WHERE external_id <> '' DO UPDATE SET
			    credentials = EXCLUDED.credentials,
			    email = CASE WHEN EXCLUDED.email <> '' THEN EXCLUDED.email ELSE upstream_accounts.email END,
			    plan_type = CASE WHEN EXCLUDED.plan_type <> '' THEN EXCLUDED.plan_type ELSE upstream_accounts.plan_type END,
			    token_expires_at = EXCLUDED.token_expires_at,
			    last_refresh_at = COALESCE(EXCLUDED.last_refresh_at, upstream_accounts.last_refresh_at),
			    name = CASE WHEN $11 THEN EXCLUDED.name ELSE upstream_accounts.name END,
			    status = CASE WHEN upstream_accounts.status = 'error' THEN 'active' ELSE upstream_accounts.status END,
			    fail_count = 0, last_error = '', updated_at = now()
			RETURNING id, (xmax = 0)`,
			name, sealed, accountID, email, ident.PlanType, opt.ProxyURL, opt.Priority, opt.ConcurrencyLimit,
			ident.ExpiresAt, t.LastRefresh, explicitName).Scan(&id, &inserted); err != nil {
			return err
		}
		if inserted || len(opt.GroupIDs) > 0 {
			return setGroups(ctx, tx, id, groups, false)
		}
		return nil
	})
	if err != nil {
		return nil, false, err
	}
	a, err := s.Get(ctx, id)
	return a, inserted, err
}

// ImportAuthJSON 导入一份 ~/.codex/auth.json：有 tokens 建/更新 Codex 订阅账号；
// 只有 OPENAI_API_KEY 时建 openai apikey 账号（https://api.openai.com/v1，supportsResponses=true）。
func (s *Service) ImportAuthJSON(ctx context.Context, text []byte, opt ImportOptions) (*Account, bool, error) {
	af, err := openai.ParseAuthJSON(text)
	if err != nil {
		return nil, false, core.BadRequest("INVALID_AUTH_JSON", err.Error())
	}
	if af.HasTokens() {
		return s.UpsertOAuthAccount(ctx, OAuthTokens{
			IDToken: af.IDToken, AccessToken: af.AccessToken, RefreshToken: af.RefreshToken,
			AccountID: af.AccountID, Email: af.Email, LastRefresh: af.LastRefresh,
		}, opt)
	}
	return s.CreateAPIKeyAccount(ctx, APIKeyAccountParams{
		Name: opt.Name, Platform: PlatformOpenAI, BaseURL: DefaultOpenAIBaseURL, APIKey: af.APIKey,
		SupportsResponses: true, Priority: opt.Priority, Weight: 1, ConcurrencyLimit: opt.ConcurrencyLimit,
		GroupIDs: opt.GroupIDs, ProxyURL: opt.ProxyURL,
		ExternalID: "key:" + core.SHA256Hex(af.APIKey)[:24],
	})
}

// ImportResult 是批量导入 Codex auth.json 的结果。
type ImportResult struct {
	Created int
	Updated int
	Errors  []string
}

// ImportCodexFiles 供 CLI `accounts import-codex` 使用：逐个读取 auth.json 并建/更新账号。
func (s *Service) ImportCodexFiles(ctx context.Context, paths []string, groupIDs []int64) (ImportResult, error) {
	var res ImportResult
	if _, err := s.resolveGroups(ctx, groupIDs, false); err != nil {
		return res, err
	}
	for _, p := range paths {
		b, err := os.ReadFile(p)
		if err != nil {
			res.Errors = append(res.Errors, fmt.Sprintf("%s: 读取失败: %v", p, err))
			continue
		}
		_, created, err := s.ImportAuthJSON(ctx, b, ImportOptions{GroupIDs: groupIDs, Priority: DefaultPriority})
		if err != nil {
			res.Errors = append(res.Errors, fmt.Sprintf("%s: %s", p, s.importErrorMessage(err)))
			continue
		}
		if created {
			res.Created++
		} else {
			res.Updated++
		}
	}
	audit.Record(ctx, s.db, 0, "account.import_codex", "cli", map[string]any{
		"files": len(paths), "created": res.Created, "updated": res.Updated, "errors": len(res.Errors),
	}, "cli")
	return res, nil
}

func (s *Service) importErrorMessage(err error) string {
	if e := core.AsError(err); e != nil {
		return e.Message
	}
	s.log.Error("import codex failed", "err", err)
	return "内部错误"
}

type importItem struct {
	Name     string          `json:"name"`
	AuthJSON json.RawMessage `json:"authJson"`
}

type importInput struct {
	Items            []importItem `json:"items"`
	GroupIDs         []int64      `json:"groupIds"`
	Priority         *int         `json:"priority"`
	ConcurrencyLimit *int         `json:"concurrencyLimit"`
	ProxyURL         string       `json:"proxyUrl"`
}

type importError struct {
	Index   int    `json:"index"`
	Message string `json:"message"`
}

// authJSONText：authJson 通常是 auth.json 原文字符串，也接受直接嵌入的 JSON 对象。
func authJSONText(raw json.RawMessage) []byte {
	var s string
	if json.Unmarshal(raw, &s) == nil {
		return []byte(s)
	}
	return raw
}

func (s *Service) handleImportCodex(w http.ResponseWriter, r *http.Request) {
	var in importInput
	if err := httpx.DecodeJSON(r, &in, 16<<20); err != nil {
		httpx.WriteError(w, err)
		return
	}
	if len(in.Items) == 0 {
		httpx.WriteError(w, core.BadRequest("INVALID_REQUEST", "items 不能为空"))
		return
	}
	if len(in.Items) > 500 {
		httpx.WriteError(w, core.BadRequest("INVALID_REQUEST", "单次最多导入 500 个"))
		return
	}
	ctx := r.Context()
	opt := ImportOptions{GroupIDs: in.GroupIDs, Priority: DefaultPriority, ProxyURL: strings.TrimSpace(in.ProxyURL)}
	if in.Priority != nil {
		opt.Priority = *in.Priority
	}
	if in.ConcurrencyLimit != nil {
		opt.ConcurrencyLimit = *in.ConcurrencyLimit
	}
	if err := validateCommon(opt.Priority, 1, opt.ConcurrencyLimit, opt.ProxyURL, nil); err != nil {
		httpx.WriteError(w, err)
		return
	}
	if _, err := s.resolveGroups(ctx, in.GroupIDs, false); err != nil {
		httpx.WriteError(w, err)
		return
	}
	var created, updated []*Account
	errs := []importError{}
	for i, it := range in.Items {
		o := opt
		o.Name = it.Name
		a, isNew, err := s.ImportAuthJSON(ctx, authJSONText(it.AuthJSON), o)
		switch {
		case err != nil:
			errs = append(errs, importError{Index: i, Message: s.importErrorMessage(err)})
		case isNew:
			created = append(created, a)
		default:
			updated = append(updated, a)
		}
	}
	cv, err := s.Views(ctx, created)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	uv, err := s.Views(ctx, updated)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	s.audit(r, "account.import_codex", "accounts", map[string]any{
		"created": accountIDs(created), "updated": accountIDs(updated), "errors": len(errs),
	})
	httpx.WriteJSON(w, http.StatusOK, map[string]any{"created": cv, "updated": uv, "errors": errs})
}

func accountIDs(list []*Account) []int64 {
	out := make([]int64, 0, len(list))
	for _, a := range list {
		out = append(out, a.ID)
	}
	return out
}

// ---------- OAuth PKCE ----------

const oauthSessionTTL = 30 * time.Minute

func oauthSessionKey(id string) string { return "oauth:openai:" + id }

type oauthSession struct {
	Verifier  string    `json:"verifier"`
	State     string    `json:"state"`
	CreatedAt time.Time `json:"createdAt"`
}

// OAuthStart 是 /accounts/oauth/openai/start 的响应。
type OAuthStart struct {
	SessionID   string    `json:"sessionId"`
	AuthURL     string    `json:"authUrl"`
	RedirectURI string    `json:"redirectUri"`
	ExpiresAt   time.Time `json:"expiresAt"`
}

// StartOAuth 生成 PKCE 会话（Redis 30 分钟）与授权链接。
func (s *Service) StartOAuth(ctx context.Context) (*OAuthStart, error) {
	p := openai.NewPKCE()
	sid := core.RandomString(32)
	now := time.Now().UTC()
	b, _ := json.Marshal(oauthSession{Verifier: p.Verifier, State: p.State, CreatedAt: now})
	if err := s.rdb.Set(ctx, oauthSessionKey(sid), b, oauthSessionTTL).Err(); err != nil {
		return nil, err
	}
	return &OAuthStart{
		SessionID:   sid,
		AuthURL:     openai.AuthorizeURL(s.cfg.OpenAIAuthURL, p),
		RedirectURI: openai.RedirectURI,
		ExpiresAt:   now.Add(oauthSessionTTL),
	}, nil
}

func (s *Service) handleOAuthStart(w http.ResponseWriter, r *http.Request) {
	out, err := s.StartOAuth(r.Context())
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	httpx.WriteJSON(w, http.StatusOK, out)
}

// OAuthExchangeInput 是 /accounts/oauth/openai/exchange 的请求体。
type OAuthExchangeInput struct {
	SessionID        string  `json:"sessionId"`
	CallbackURL      string  `json:"callbackUrl"`
	Code             string  `json:"code"`
	Name             string  `json:"name"`
	GroupIDs         []int64 `json:"groupIds"`
	Priority         *int    `json:"priority"`
	ConcurrencyLimit *int    `json:"concurrencyLimit"`
	ProxyURL         string  `json:"proxyUrl"`
	// AccountID > 0：重新授权已有账号。
	AccountID int64 `json:"accountId"`
}

func (s *Service) oauthClient(proxy string) (*openai.Client, error) {
	hc, err := openai.HTTPClient(proxy, s.cfg.UpstreamProxy)
	if err != nil {
		return nil, err
	}
	return &openai.Client{AuthURL: s.cfg.OpenAIAuthURL, HTTP: hc}, nil
}

// ExchangeOAuth 用回调里的 code + 会话 verifier 换 token，新建/更新账号（或重新授权 AccountID）。
func (s *Service) ExchangeOAuth(ctx context.Context, in OAuthExchangeInput) (*Account, bool, error) {
	sid := strings.TrimSpace(in.SessionID)
	if sid == "" {
		return nil, false, core.BadRequest("OAUTH_SESSION_INVALID", "缺少 sessionId")
	}
	raw, err := s.rdb.Get(ctx, oauthSessionKey(sid)).Bytes()
	if errors.Is(err, redis.Nil) {
		return nil, false, core.BadRequest("OAUTH_SESSION_INVALID", "授权会话不存在或已过期，请重新生成授权链接")
	}
	if err != nil {
		return nil, false, err
	}
	var sess oauthSession
	if err := json.Unmarshal(raw, &sess); err != nil {
		return nil, false, core.BadRequest("OAUTH_SESSION_INVALID", "授权会话已损坏，请重新生成授权链接")
	}

	code, state := strings.TrimSpace(in.Code), ""
	callback := strings.TrimSpace(in.CallbackURL)
	if callback == "" && strings.Contains(code, "code=") {
		callback, code = code, ""
	}
	if callback != "" {
		if code, state, err = openai.ParseCallback(callback); err != nil {
			return nil, false, core.BadRequest("OAUTH_CALLBACK_INVALID", err.Error())
		}
		if state != sess.State {
			return nil, false, core.BadRequest("OAUTH_STATE_MISMATCH", "回调地址的 state 与授权会话不匹配")
		}
	}
	if code == "" {
		return nil, false, core.BadRequest("OAUTH_CODE_REQUIRED", "请粘贴回调地址或授权码")
	}

	var existing *Account
	proxy := strings.TrimSpace(in.ProxyURL)
	if in.AccountID > 0 {
		if existing, err = s.Get(ctx, in.AccountID); err != nil {
			return nil, false, err
		}
		if !existing.IsCodex() {
			return nil, false, core.BadRequest("NOT_SUPPORTED", "只有 Codex 订阅（openai + oauth）账号可以重新授权")
		}
		if proxy == "" {
			proxy = existing.ProxyURL
		}
	}
	if proxy != "" {
		if _, err := openai.ValidateProxyURL(proxy); err != nil {
			return nil, false, core.BadRequest("INVALID_REQUEST", err.Error())
		}
	}
	client, err := s.oauthClient(proxy)
	if err != nil {
		return nil, false, err
	}
	tctx, cancel := context.WithTimeout(ctx, 30*time.Second)
	defer cancel()
	tok, err := client.ExchangeCode(tctx, code, sess.Verifier)
	if err != nil {
		return nil, false, core.BadRequest("OAUTH_EXCHANGE_FAILED", "换取 token 失败："+err.Error())
	}
	now := time.Now().UTC()
	tokens := OAuthTokens{
		IDToken: tok.IDToken, AccessToken: tok.AccessToken, RefreshToken: tok.RefreshToken,
		ExpiresIn: tok.ExpiresIn, LastRefresh: &now,
	}

	var a *Account
	created := false
	if existing != nil {
		a, err = s.reauthorize(ctx, existing, tokens, in)
	} else {
		opt := ImportOptions{Name: in.Name, GroupIDs: in.GroupIDs, Priority: DefaultPriority, ProxyURL: proxy}
		if in.Priority != nil {
			opt.Priority = *in.Priority
		}
		if in.ConcurrencyLimit != nil {
			opt.ConcurrencyLimit = *in.ConcurrencyLimit
		}
		a, created, err = s.UpsertOAuthAccount(ctx, tokens, opt)
	}
	if err != nil {
		return nil, false, err
	}
	_ = s.rdb.Del(context.WithoutCancel(ctx), oauthSessionKey(sid)).Err()
	return a, created, nil
}

func (s *Service) reauthorize(ctx context.Context, a *Account, t OAuthTokens, in OAuthExchangeInput) (*Account, error) {
	ident := openai.IdentityFromTokens(t.IDToken, t.AccessToken, t.ExpiresIn)
	if ident.AccountID == "" {
		return nil, core.BadRequest("OAUTH_EXCHANGE_FAILED", "token 里没有 ChatGPT 账号 ID（chatgpt_account_id）")
	}
	groups, err := s.resolveGroups(ctx, in.GroupIDs, false)
	if err != nil {
		return nil, err
	}
	name := strings.TrimSpace(in.Name)
	if name != "" {
		if err := validName(name); err != nil {
			return nil, err
		}
	}
	sealed, err := s.vault.EncryptJSON(Credentials{
		AccessToken: t.AccessToken, RefreshToken: t.RefreshToken, IDToken: t.IDToken,
		AccountID: ident.AccountID, ExpiresAt: ident.ExpiresAt,
	})
	if err != nil {
		return nil, err
	}
	err = pgx.BeginFunc(ctx, s.db, func(tx pgx.Tx) error {
		if _, err := tx.Exec(ctx, `UPDATE upstream_accounts SET credentials = $2, external_id = $3,
			email = CASE WHEN $4 <> '' THEN $4 ELSE email END,
			plan_type = CASE WHEN $5 <> '' THEN $5 ELSE plan_type END,
			token_expires_at = $6, last_refresh_at = now(), fail_count = 0, last_error = '',
			status = CASE WHEN status = 'error' THEN 'active' ELSE status END,
			name = CASE WHEN $7 <> '' THEN $7 ELSE name END,
			proxy_url = CASE WHEN $8 <> '' THEN $8 ELSE proxy_url END,
			updated_at = now()
			WHERE id = $1`,
			a.ID, sealed, ident.AccountID, ident.Email, ident.PlanType, ident.ExpiresAt, name,
			strings.TrimSpace(in.ProxyURL)); err != nil {
			return err
		}
		return setGroups(ctx, tx, a.ID, groups, false)
	})
	if isUniqueViolation(err) {
		return nil, core.Conflict("ACCOUNT_EXISTS", "该 ChatGPT 账号已存在于另一个上游账号")
	}
	if err != nil {
		return nil, err
	}
	return s.Get(ctx, a.ID)
}

func (s *Service) handleOAuthExchange(w http.ResponseWriter, r *http.Request) {
	var in OAuthExchangeInput
	if err := httpx.DecodeJSON(r, &in, 0); err != nil {
		httpx.WriteError(w, err)
		return
	}
	a, created, err := s.ExchangeOAuth(r.Context(), in)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	s.audit(r, "account.oauth_exchange", accountTarget(a.ID), map[string]any{
		"created": created, "email": a.Email, "planType": a.PlanType, "reauthorize": in.AccountID > 0,
	})
	v, err := s.view(r.Context(), a)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	httpx.WriteJSON(w, http.StatusOK, v)
}

func truncateASCII(s string, n int) string {
	if len(s) <= n {
		return s
	}
	return s[:n]
}
