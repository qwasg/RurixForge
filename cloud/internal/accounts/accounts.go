// Package accounts：上游账号池（apikey/oauth）、Codex 订阅导入（auth.json、OAuth PKCE、CLI）、
// token 刷新 worker、额度快照、连通性测试，以及模型目录（models 表）与其管理接口
// （15_CLOUD_SERVICE.md §6、§7）。
//
// 凭据以 vault 加密存 upstream_accounts.credentials，任何接口都不回显（apikey 账号只回 keyHint）。
// 账号通过 account_groups 归属分组，调度只在请求方有效分组内选号；新建/导入时未指定分组则归入默认分组。
package accounts

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"net/http"
	"strings"
	"sync"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/redis/go-redis/v9"

	"forge-cloud/internal/config"
	"forge-cloud/internal/core"
	"forge-cloud/internal/oauth/openai"
	"forge-cloud/internal/ratelimit"
	"forge-cloud/internal/syssettings"
	"forge-cloud/internal/vault"
)

const (
	PlatformOpenAI    = "openai"
	PlatformAnthropic = "anthropic"

	AuthOAuth  = "oauth"
	AuthAPIKey = "apikey"

	StatusActive   = "active"
	StatusDisabled = "disabled"
	StatusError    = "error"

	DefaultPriority = 50

	DefaultOpenAIBaseURL    = "https://api.openai.com/v1"
	DefaultAnthropicBaseURL = "https://api.anthropic.com/v1"

	// AnthropicVersion 是调 Anthropic API 时的默认 anthropic-version 头。
	AnthropicVersion = "2023-06-01"
)

type Service struct {
	db       *pgxpool.Pool
	rdb      *redis.Client
	cfg      *config.Config
	log      *slog.Logger
	vault    *vault.Vault
	settings *syssettings.Store
	limiter  *ratelimit.Limiter

	modelsMu sync.Mutex
	models   *modelCache

	// refreshEvery 是刷新 worker 的扫描间隔（测试可调）。
	refreshEvery time.Duration
}

func New(db *pgxpool.Pool, rdb *redis.Client, cfg *config.Config, log *slog.Logger, v *vault.Vault, settings *syssettings.Store) *Service {
	return &Service{
		db: db, rdb: rdb, cfg: cfg, log: log, vault: v, settings: settings,
		limiter:      ratelimit.New(rdb),
		refreshEvery: 5 * time.Minute,
	}
}

// Account 是一行上游账号（凭据保持密文，按需 Credentials 解密）。
type Account struct {
	ID                int64
	Name              string
	Platform          string
	AuthType          string
	BaseURL           string
	KeyHint           string
	ExternalID        string
	Email             string
	PlanType          string
	SupportsResponses bool
	ModelMapping      map[string]string
	ProxyURL          string
	Status            string
	Priority          int
	Weight            int
	ConcurrencyLimit  int
	CooldownUntil     *time.Time
	FailCount         int
	LastError         string
	LastErrorAt       *time.Time
	Quota             json.RawMessage
	TokenExpiresAt    *time.Time
	LastRefreshAt     *time.Time
	LastUsedAt        *time.Time
	CreatedAt         time.Time
	UpdatedAt         time.Time
	GroupIDs          []int64
	AllowedModels     []string

	sealed []byte
}

// IsCodex：openai + oauth（ChatGPT/Codex 订阅）。
func (a *Account) IsCodex() bool { return a.Platform == PlatformOpenAI && a.AuthType == AuthOAuth }

// CoolingDown 表示账号处于冷却期。
func (a *Account) CoolingDown(now time.Time) bool {
	return a.CooldownUntil != nil && a.CooldownUntil.After(now)
}

// SupportsEndpoint 实现 §4.1 平台 × 端点矩阵（chat|responses|messages|embeddings）。
func (a *Account) SupportsEndpoint(endpoint string) bool {
	switch {
	case a.Platform == PlatformOpenAI && a.AuthType == AuthOAuth:
		return endpoint == "chat" || endpoint == "responses"
	case a.Platform == PlatformOpenAI && a.AuthType == AuthAPIKey:
		return endpoint == "chat" || endpoint == "embeddings" || (endpoint == "responses" && a.SupportsResponses)
	case a.Platform == PlatformAnthropic && a.AuthType == AuthAPIKey:
		return endpoint == "chat" || endpoint == "messages"
	}
	return false
}

// UpstreamModel：账号 modelMapping[id] > 模型 upstreamModel > id。
func (a *Account) UpstreamModel(m *core.Model) string {
	if v := strings.TrimSpace(a.ModelMapping[m.ID]); v != "" {
		return v
	}
	if v := strings.TrimSpace(m.UpstreamModel); v != "" {
		return v
	}
	return m.ID
}

// SupportsModel：空列表不限模型；否则匹配目录 ID 或映射后的上游模型名。
func (a *Account) SupportsModel(m *core.Model) bool {
	if len(a.AllowedModels) == 0 {
		return true
	}
	upstream := a.UpstreamModel(m)
	for _, id := range a.AllowedModels {
		if id == m.ID || id == upstream {
			return true
		}
	}
	return false
}

// Credentials 是解密后的上游凭据：apikey 账号 {"apiKey"}；
// oauth 账号 {"accessToken","refreshToken","idToken","accountId","expiresAt"}。
type Credentials struct {
	APIKey       string     `json:"apiKey,omitempty"`
	AccessToken  string     `json:"accessToken,omitempty"`
	RefreshToken string     `json:"refreshToken,omitempty"`
	IDToken      string     `json:"idToken,omitempty"`
	AccountID    string     `json:"accountId,omitempty"`
	ExpiresAt    *time.Time `json:"expiresAt,omitempty"`
}

// Credentials 解密账号凭据。
func (s *Service) Credentials(a *Account) (*Credentials, error) {
	var c Credentials
	if err := s.vault.DecryptJSON(a.sealed, &c); err != nil {
		return nil, fmt.Errorf("账号 %d 凭据解密失败: %w", a.ID, err)
	}
	return &c, nil
}

// HTTPClient 返回访问该账号上游的 HTTP 客户端（账号 proxyUrl 优先，其次全局代理）。
func (s *Service) HTTPClient(a *Account) (*http.Client, error) {
	return openai.HTTPClient(a.ProxyURL, s.cfg.UpstreamProxy)
}

// Limiter 暴露共享的限流器（账号并发计数）。
func (s *Service) Limiter() *ratelimit.Limiter { return s.limiter }

// ---------- 查询 ----------

const accountCols = `a.id, a.name, a.platform, a.auth_type, a.base_url, a.credentials, a.key_hint, a.external_id,
	a.email, a.plan_type, a.supports_responses, a.model_mapping, a.proxy_url, a.status, a.priority, a.weight,
	a.concurrency_limit, a.cooldown_until, a.fail_count, a.last_error, a.last_error_at, a.quota,
	a.token_expires_at, a.last_refresh_at, a.last_used_at, a.created_at, a.updated_at,
	COALESCE((SELECT array_agg(ag.group_id ORDER BY ag.group_id) FROM account_groups ag WHERE ag.account_id = a.id), '{}'),
	a.allowed_models`

func scanAccount(row pgx.Row) (*Account, error) {
	a := &Account{}
	var mapping, quota, allowed []byte
	err := row.Scan(&a.ID, &a.Name, &a.Platform, &a.AuthType, &a.BaseURL, &a.sealed, &a.KeyHint, &a.ExternalID,
		&a.Email, &a.PlanType, &a.SupportsResponses, &mapping, &a.ProxyURL, &a.Status, &a.Priority, &a.Weight,
		&a.ConcurrencyLimit, &a.CooldownUntil, &a.FailCount, &a.LastError, &a.LastErrorAt, &quota,
		&a.TokenExpiresAt, &a.LastRefreshAt, &a.LastUsedAt, &a.CreatedAt, &a.UpdatedAt, &a.GroupIDs, &allowed)
	if err != nil {
		return nil, err
	}
	a.ModelMapping = map[string]string{}
	a.AllowedModels = []string{}
	if len(allowed) > 0 {
		if err := json.Unmarshal(allowed, &a.AllowedModels); err != nil {
			return nil, fmt.Errorf("账号 %d 模型范围无效: %w", a.ID, err)
		}
	}
	if len(mapping) > 0 {
		_ = json.Unmarshal(mapping, &a.ModelMapping)
	}
	a.Quota = json.RawMessage("{}")
	if len(quota) > 0 {
		a.Quota = json.RawMessage(quota)
	}
	if a.GroupIDs == nil {
		a.GroupIDs = []int64{}
	}
	for _, t := range []**time.Time{&a.CooldownUntil, &a.LastErrorAt, &a.TokenExpiresAt, &a.LastRefreshAt, &a.LastUsedAt} {
		if *t != nil {
			u := (*t).UTC()
			*t = &u
		}
	}
	a.CreatedAt = a.CreatedAt.UTC()
	a.UpdatedAt = a.UpdatedAt.UTC()
	return a, nil
}

func scanAccounts(rows pgx.Rows) ([]*Account, error) {
	defer rows.Close()
	var out []*Account
	for rows.Next() {
		a, err := scanAccount(rows)
		if err != nil {
			return nil, err
		}
		out = append(out, a)
	}
	return out, rows.Err()
}

func errAccountNotFound() *core.Error {
	return core.NotFound("ACCOUNT_NOT_FOUND", "上游账号不存在")
}

// Get 按 ID 取账号；不存在返回 404 ACCOUNT_NOT_FOUND。
func (s *Service) Get(ctx context.Context, id int64) (*Account, error) {
	a, err := scanAccount(s.db.QueryRow(ctx, `SELECT `+accountCols+` FROM upstream_accounts a WHERE a.id = $1`, id))
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, errAccountNotFound()
	}
	return a, err
}

// Candidates 返回分组内指定平台的可调度账号（status=active 且不在冷却）。groupID<=0 时不按分组过滤。
func (s *Service) Candidates(ctx context.Context, groupID int64, platform string) ([]*Account, error) {
	rows, err := s.db.Query(ctx, `SELECT `+accountCols+` FROM upstream_accounts a
		WHERE a.platform = $1 AND a.status = 'active' AND (a.cooldown_until IS NULL OR a.cooldown_until <= now())
		  AND ($2::bigint <= 0 OR EXISTS (SELECT 1 FROM account_groups ag WHERE ag.account_id = a.id AND ag.group_id = $2::bigint))
		ORDER BY a.priority, a.id`, platform, groupID)
	if err != nil {
		return nil, err
	}
	return scanAccounts(rows)
}

// HealthyPlatforms 返回分组内存在可调度账号的平台集合（模型目录 available）。
func (s *Service) HealthyPlatforms(ctx context.Context, groupID int64) (map[string]bool, error) {
	rows, err := s.db.Query(ctx, `SELECT DISTINCT a.platform FROM upstream_accounts a
		WHERE a.status = 'active' AND (a.cooldown_until IS NULL OR a.cooldown_until <= now())
		  AND ($1::bigint <= 0 OR EXISTS (SELECT 1 FROM account_groups ag WHERE ag.account_id = a.id AND ag.group_id = $1::bigint))`, groupID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := map[string]bool{}
	for rows.Next() {
		var p string
		if err := rows.Scan(&p); err != nil {
			return nil, err
		}
		out[p] = true
	}
	return out, rows.Err()
}

// ---------- 网关回写 ----------

// Cooldown 让账号冷却 d（不会缩短已有的更长冷却），并记录原因。
func (s *Service) Cooldown(ctx context.Context, id int64, d time.Duration, reason string) error {
	until := time.Now().Add(d).UTC()
	_, err := s.db.Exec(ctx, `UPDATE upstream_accounts
		SET cooldown_until = GREATEST(COALESCE(cooldown_until, $2), $2),
		    last_error = $3, last_error_at = now(), updated_at = now()
		WHERE id = $1`, id, until, truncate(reason, 500))
	return err
}

// MarkError 把账号置为 error（需管理员处理或刷新成功后恢复）。
func (s *Service) MarkError(ctx context.Context, id int64, reason string) error {
	_, err := s.db.Exec(ctx, `UPDATE upstream_accounts
		SET status = CASE WHEN status = 'active' THEN 'error' ELSE status END,
		    last_error = $2, last_error_at = now(), updated_at = now()
		WHERE id = $1`, id, truncate(reason, 500))
	return err
}

// TouchUsed 更新 last_used_at。
func (s *Service) TouchUsed(ctx context.Context, id int64) error {
	_, err := s.db.Exec(ctx, `UPDATE upstream_accounts SET last_used_at = now() WHERE id = $1`, id)
	return err
}

// RecordCodexQuota 把 x-codex-{primary,secondary}-* 头写入 quota.primary / quota.secondary。
// 两组头都缺失时不写库。
func (s *Service) RecordCodexQuota(ctx context.Context, id int64, h http.Header) error {
	primary, secondary := openai.ParseCodexRateLimits(h)
	if primary == nil && secondary == nil {
		return nil
	}
	now := time.Now().UTC()
	patch := map[string]any{"updatedAt": now}
	if primary != nil {
		patch["primary"] = windowJSON(primary, now)
	}
	if secondary != nil {
		patch["secondary"] = windowJSON(secondary, now)
	}
	b, _ := json.Marshal(patch)
	_, err := s.db.Exec(ctx, `UPDATE upstream_accounts SET quota = quota || $2::jsonb WHERE id = $1`, id, b)
	return err
}

func windowJSON(w *openai.RateWindow, now time.Time) map[string]any {
	out := map[string]any{"updatedAt": now}
	if w.UsedPercent != nil {
		out["usedPercent"] = *w.UsedPercent
	}
	if w.WindowMinutes != nil {
		out["windowMinutes"] = *w.WindowMinutes
	}
	if w.ResetAfterSeconds != nil {
		out["resetAfterSeconds"] = *w.ResetAfterSeconds
		out["resetsAt"] = now.Add(time.Duration(*w.ResetAfterSeconds) * time.Second)
	}
	return out
}

// ---------- 小工具 ----------

func truncate(s string, n int) string {
	r := []rune(s)
	if len(r) <= n {
		return s
	}
	return string(r[:n]) + "…"
}

func firstNonEmpty(vs ...string) string {
	for _, v := range vs {
		if strings.TrimSpace(v) != "" {
			return strings.TrimSpace(v)
		}
	}
	return ""
}

func keyHint(key string) string {
	r := []rune(strings.TrimSpace(key))
	if len(r) <= 4 {
		return string(r)
	}
	return string(r[len(r)-4:])
}
