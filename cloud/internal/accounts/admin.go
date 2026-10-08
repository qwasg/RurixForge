package accounts

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"net/url"
	"strconv"
	"strings"
	"time"

	"github.com/go-chi/chi/v5"
	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"

	"forge-cloud/internal/audit"
	"forge-cloud/internal/core"
	"forge-cloud/internal/httpx"
	"forge-cloud/internal/oauth/openai"
	"forge-cloud/internal/ratelimit"
)

// MountAdmin 挂 /accounts… 与 /models…（相对 /api/admin，已在管理员分组内）。
func (s *Service) MountAdmin(r chi.Router) {
	r.Get("/accounts", s.handleList)
	r.Post("/accounts", s.handleCreate)
	r.Post("/accounts/import-codex", s.handleImportCodex)
	r.Post("/accounts/oauth/openai/start", s.handleOAuthStart)
	r.Post("/accounts/oauth/openai/exchange", s.handleOAuthExchange)
	r.Patch("/accounts/{id}", s.handlePatch)
	r.Delete("/accounts/{id}", s.handleDelete)
	r.Post("/accounts/{id}/refresh", s.handleRefresh)
	r.Post("/accounts/{id}/quota", s.handleQuota)
	r.Post("/accounts/{id}/clear-cooldown", s.handleClearCooldown)
	r.Post("/accounts/{id}/test", s.handleTest)
	r.Get("/accounts/{id}/models", s.handleUpstreamModels)
	r.Get("/models", s.handleListModels)
	r.Post("/models", s.handleCreateModel)
	r.Patch("/models/{id}", s.handlePatchModel)
	r.Delete("/models/{id}", s.handleDeleteModel)
}

func (s *Service) audit(r *http.Request, action, target string, detail any) {
	var actor int64
	if c, ok := core.ClaimsFrom(r.Context()); ok {
		actor = c.UserID
	}
	audit.Record(r.Context(), s.db, actor, action, target, detail, httpx.ClientIP(r, s.cfg.TrustProxy))
}

// ---------- 视图 ----------

// View 是管理接口的 Account 形状（不含任何凭据）。
type View struct {
	ID                 int64             `json:"id"`
	Name               string            `json:"name"`
	Platform           string            `json:"platform"`
	AuthType           string            `json:"authType"`
	BaseURL            string            `json:"baseUrl"`
	Email              string            `json:"email"`
	PlanType           string            `json:"planType"`
	Status             string            `json:"status"`
	Priority           int               `json:"priority"`
	Weight             int               `json:"weight"`
	ConcurrencyLimit   int               `json:"concurrencyLimit"`
	CurrentConcurrency int               `json:"currentConcurrency"`
	GroupIDs           []int64           `json:"groupIds"`
	ModelMapping       map[string]string `json:"modelMapping"`
	AllowedModels      []string          `json:"allowedModels"`
	SupportsResponses  bool              `json:"supportsResponses"`
	ProxyURL           string            `json:"proxyUrl"`
	CooldownUntil      *time.Time        `json:"cooldownUntil"`
	LastError          string            `json:"lastError"`
	LastErrorAt        *time.Time        `json:"lastErrorAt"`
	TokenExpiresAt     *time.Time        `json:"tokenExpiresAt"`
	LastRefreshAt      *time.Time        `json:"lastRefreshAt"`
	LastUsedAt         *time.Time        `json:"lastUsedAt"`
	Quota              json.RawMessage   `json:"quota"`
	KeyHint            string            `json:"keyHint"`
	CreatedAt          time.Time         `json:"createdAt"`
	UpdatedAt          time.Time         `json:"updatedAt"`
}

func viewOf(a *Account, current int, now time.Time) View {
	v := View{
		ID: a.ID, Name: a.Name, Platform: a.Platform, AuthType: a.AuthType, BaseURL: a.BaseURL,
		Email: a.Email, PlanType: a.PlanType, Status: a.Status, Priority: a.Priority, Weight: a.Weight,
		ConcurrencyLimit: a.ConcurrencyLimit, CurrentConcurrency: current, GroupIDs: a.GroupIDs,
		ModelMapping: a.ModelMapping, AllowedModels: a.AllowedModels, SupportsResponses: a.SupportsResponses, ProxyURL: a.ProxyURL,
		LastError: a.LastError, LastErrorAt: a.LastErrorAt, TokenExpiresAt: a.TokenExpiresAt,
		LastRefreshAt: a.LastRefreshAt, LastUsedAt: a.LastUsedAt, Quota: a.Quota, KeyHint: a.KeyHint,
		CreatedAt: a.CreatedAt, UpdatedAt: a.UpdatedAt,
	}
	if a.CoolingDown(now) {
		v.CooldownUntil = a.CooldownUntil
	}
	return v
}

// Views 把账号转成管理视图（currentConcurrency 取 Redis 账号槽计数）。
func (s *Service) Views(ctx context.Context, list []*Account) ([]View, error) {
	keys := make([]string, len(list))
	for i, a := range list {
		keys[i] = ratelimit.AccountSlotKey(a.ID)
	}
	counts, err := s.limiter.Counts(ctx, keys)
	if err != nil {
		return nil, err
	}
	now := time.Now()
	out := make([]View, len(list))
	for i, a := range list {
		out[i] = viewOf(a, counts[i], now)
	}
	return out, nil
}

func (s *Service) view(ctx context.Context, a *Account) (View, error) {
	vs, err := s.Views(ctx, []*Account{a})
	if err != nil {
		return View{}, err
	}
	return vs[0], nil
}

func (s *Service) writeAccount(w http.ResponseWriter, r *http.Request, id int64) {
	a, err := s.Get(r.Context(), id)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	v, err := s.view(r.Context(), a)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	httpx.WriteJSON(w, http.StatusOK, v)
}

// ---------- 列表 ----------

func (s *Service) handleList(w http.ResponseWriter, r *http.Request) {
	ctx := r.Context()
	q := r.URL.Query()
	limit, offset := httpx.Pagination(r)
	where := []string{"TRUE"}
	var args []any
	arg := func(v any) string {
		args = append(args, v)
		return "$" + strconv.Itoa(len(args))
	}
	if v := strings.TrimSpace(q.Get("platform")); v != "" {
		where = append(where, "a.platform = "+arg(v))
	}
	if v := strings.TrimSpace(q.Get("status")); v != "" {
		where = append(where, "a.status = "+arg(v))
	}
	if gid := httpx.QueryInt64(r, "groupId"); gid > 0 {
		where = append(where, "EXISTS (SELECT 1 FROM account_groups ag WHERE ag.account_id = a.id AND ag.group_id = "+arg(gid)+")")
	}
	if v := strings.TrimSpace(q.Get("q")); v != "" {
		p := arg("%" + escapeLike(v) + "%")
		where = append(where, "(a.name ILIKE "+p+" OR a.email ILIKE "+p+" OR a.base_url ILIKE "+p+" OR a.external_id ILIKE "+p+")")
	}
	cond := strings.Join(where, " AND ")
	var total int
	if err := s.db.QueryRow(ctx, `SELECT count(*) FROM upstream_accounts a WHERE `+cond, args...).Scan(&total); err != nil {
		httpx.WriteError(w, err)
		return
	}
	rows, err := s.db.Query(ctx, `SELECT `+accountCols+` FROM upstream_accounts a WHERE `+cond+
		` ORDER BY a.id LIMIT `+arg(limit)+` OFFSET `+arg(offset), args...)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	list, err := scanAccounts(rows)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	items, err := s.Views(ctx, list)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	httpx.WriteJSON(w, http.StatusOK, map[string]any{"items": items, "total": total})
}

func escapeLike(s string) string {
	return strings.NewReplacer(`\`, `\\`, `%`, `\%`, `_`, `\_`).Replace(s)
}

// ---------- 新建 / 修改 / 删除 ----------

type accountInput struct {
	Name              *string            `json:"name"`
	Platform          *string            `json:"platform"`
	BaseURL           *string            `json:"baseUrl"`
	APIKey            *string            `json:"apiKey"`
	SupportsResponses *bool              `json:"supportsResponses"`
	Priority          *int               `json:"priority"`
	Weight            *int               `json:"weight"`
	ConcurrencyLimit  *int               `json:"concurrencyLimit"`
	GroupIDs          *[]int64           `json:"groupIds"`
	ModelMapping      *map[string]string `json:"modelMapping"`
	AllowedModels     *[]string          `json:"allowedModels"`
	ProxyURL          *string            `json:"proxyUrl"`
	Status            *string            `json:"status"`
}

// APIKeyAccountParams 是新建 apikey 账号的参数（已套用默认值）。
type APIKeyAccountParams struct {
	Name              string
	Platform          string
	BaseURL           string
	APIKey            string
	SupportsResponses bool
	Priority          int
	Weight            int
	ConcurrencyLimit  int
	GroupIDs          []int64
	ModelMapping      map[string]string
	AllowedModels     []string
	ProxyURL          string
	// ExternalID 非空时按 (platform, apikey, externalId) 去重（auth.json 导入的 OPENAI_API_KEY 用）。
	ExternalID string
}

func normalizeBaseURL(raw, platform string) (string, error) {
	raw = strings.TrimSpace(raw)
	if raw == "" {
		if platform == PlatformAnthropic {
			return DefaultAnthropicBaseURL, nil
		}
		return DefaultOpenAIBaseURL, nil
	}
	u, err := url.Parse(raw)
	if err != nil || (u.Scheme != "http" && u.Scheme != "https") || u.Host == "" || u.RawQuery != "" || u.Fragment != "" {
		return "", core.BadRequest("INVALID_REQUEST", "baseUrl 必须是 http(s) 地址（含版本段，如 https://api.openai.com/v1）")
	}
	return strings.TrimRight(raw, "/"), nil
}

func validateCommon(priority, weight, concurrency int, proxy string, mapping map[string]string) error {
	if priority < 0 || priority > 10000 {
		return core.BadRequest("INVALID_REQUEST", "priority 范围 0–10000")
	}
	if weight < 1 || weight > 10000 {
		return core.BadRequest("INVALID_REQUEST", "weight 范围 1–10000")
	}
	if concurrency < 0 || concurrency > 100000 {
		return core.BadRequest("INVALID_REQUEST", "concurrencyLimit 不能为负")
	}
	if proxy != "" {
		if _, err := openai.ValidateProxyURL(proxy); err != nil {
			return core.BadRequest("INVALID_REQUEST", err.Error())
		}
	}
	if len(mapping) > 500 {
		return core.BadRequest("INVALID_REQUEST", "modelMapping 条目过多")
	}
	for k, v := range mapping {
		if strings.TrimSpace(k) == "" || strings.TrimSpace(v) == "" {
			return core.BadRequest("INVALID_REQUEST", "modelMapping 的键和值都不能为空")
		}
	}
	return nil
}

func validName(name string) error {
	if n := len([]rune(name)); n == 0 || n > 100 {
		return core.BadRequest("INVALID_REQUEST", "账号名称 1–100 个字符")
	}
	return nil
}

func normalizeAllowedModels(ids []string) ([]string, error) {
	if len(ids) > 500 {
		return nil, core.BadRequest("INVALID_REQUEST", "allowedModels 条目过多")
	}
	out := make([]string, 0, len(ids))
	seen := map[string]bool{}
	for _, id := range ids {
		id = strings.TrimSpace(id)
		if !validModelName(id) {
			return nil, core.BadRequest("INVALID_REQUEST", "allowedModels 的模型 ID 不能为空、超过 128 字节或包含空白")
		}
		if !seen[id] {
			seen[id] = true
			out = append(out, id)
		}
	}
	return out, nil
}

// resolveGroups 校验分组存在；为空时归入默认分组。
func (s *Service) resolveGroups(ctx context.Context, ids []int64, defaultIfEmpty bool) ([]int64, error) {
	uniq := make([]int64, 0, len(ids))
	seen := map[int64]bool{}
	for _, id := range ids {
		if !seen[id] {
			seen[id] = true
			uniq = append(uniq, id)
		}
	}
	if len(uniq) == 0 {
		if !defaultIfEmpty {
			return uniq, nil
		}
		gid, err := s.settings.DefaultGroupID(ctx)
		if err != nil {
			return nil, err
		}
		if gid > 0 {
			uniq = append(uniq, gid)
		}
		return uniq, nil
	}
	var n int
	if err := s.db.QueryRow(ctx, `SELECT count(*) FROM groups WHERE id = ANY($1)`, uniq).Scan(&n); err != nil {
		return nil, err
	}
	if n != len(uniq) {
		return nil, core.BadRequest("INVALID_GROUP", "分组不存在")
	}
	return uniq, nil
}

func setGroups(ctx context.Context, tx pgx.Tx, accountID int64, groupIDs []int64, replace bool) error {
	if replace {
		if _, err := tx.Exec(ctx, `DELETE FROM account_groups WHERE account_id = $1`, accountID); err != nil {
			return err
		}
	}
	if len(groupIDs) == 0 {
		return nil
	}
	_, err := tx.Exec(ctx, `INSERT INTO account_groups (account_id, group_id)
		SELECT $1, g FROM unnest($2::bigint[]) AS g ON CONFLICT DO NOTHING`, accountID, groupIDs)
	return err
}

// CreateAPIKeyAccount 新建（或按 ExternalID 去重更新）apikey 账号，返回账号与是否新建。
func (s *Service) CreateAPIKeyAccount(ctx context.Context, p APIKeyAccountParams) (*Account, bool, error) {
	p.Platform = strings.TrimSpace(p.Platform)
	if p.Platform != PlatformOpenAI && p.Platform != PlatformAnthropic {
		return nil, false, core.BadRequest("INVALID_REQUEST", "platform 只能是 openai 或 anthropic")
	}
	base, err := normalizeBaseURL(p.BaseURL, p.Platform)
	if err != nil {
		return nil, false, err
	}
	p.AllowedModels, err = normalizeAllowedModels(p.AllowedModels)
	if err != nil {
		return nil, false, err
	}
	p.APIKey = strings.TrimSpace(p.APIKey)
	if p.APIKey == "" {
		return nil, false, core.BadRequest("API_KEY_REQUIRED", "apiKey 必填")
	}
	p.ProxyURL = strings.TrimSpace(p.ProxyURL)
	if p.Weight == 0 {
		p.Weight = 1
	}
	if err := validateCommon(p.Priority, p.Weight, p.ConcurrencyLimit, p.ProxyURL, p.ModelMapping); err != nil {
		return nil, false, err
	}
	p.Name = strings.TrimSpace(p.Name)
	if p.Name == "" {
		p.Name = p.Platform + "-" + keyHint(p.APIKey)
	}
	if err := validName(p.Name); err != nil {
		return nil, false, err
	}
	groups, err := s.resolveGroups(ctx, p.GroupIDs, true)
	if err != nil {
		return nil, false, err
	}
	sealed, err := s.vault.EncryptJSON(Credentials{APIKey: p.APIKey})
	if err != nil {
		return nil, false, err
	}
	mapping := p.ModelMapping
	if mapping == nil {
		mapping = map[string]string{}
	}
	mb, _ := json.Marshal(mapping)
	ab, _ := json.Marshal(p.AllowedModels)
	var id int64
	var inserted bool
	err = pgx.BeginFunc(ctx, s.db, func(tx pgx.Tx) error {
		if p.ExternalID == "" {
			inserted = true
			if err := tx.QueryRow(ctx, `INSERT INTO upstream_accounts (name, platform, auth_type, base_url, credentials,
				key_hint, supports_responses, model_mapping, proxy_url, priority, weight, concurrency_limit, allowed_models)
				VALUES ($1, $2, 'apikey', $3, $4, $5, $6, $7, $8, $9, $10, $11, $12) RETURNING id`,
				p.Name, p.Platform, base, sealed, keyHint(p.APIKey), p.SupportsResponses, mb, p.ProxyURL,
				p.Priority, p.Weight, p.ConcurrencyLimit, ab).Scan(&id); err != nil {
				return err
			}
		} else if err := tx.QueryRow(ctx, `INSERT INTO upstream_accounts (name, platform, auth_type, base_url, credentials,
				key_hint, external_id, supports_responses, model_mapping, proxy_url, priority, weight, concurrency_limit, allowed_models)
				VALUES ($1, $2, 'apikey', $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
				ON CONFLICT (platform, auth_type, external_id) WHERE external_id <> '' DO UPDATE
				SET credentials = EXCLUDED.credentials, key_hint = EXCLUDED.key_hint,
				    status = CASE WHEN upstream_accounts.status = 'error' THEN 'active' ELSE upstream_accounts.status END,
				    fail_count = 0, last_error = '', updated_at = now()
				RETURNING id, (xmax = 0)`,
			p.Name, p.Platform, base, sealed, keyHint(p.APIKey), p.ExternalID, p.SupportsResponses, mb, p.ProxyURL,
			p.Priority, p.Weight, p.ConcurrencyLimit, ab).Scan(&id, &inserted); err != nil {
			return err
		}
		if inserted || len(p.GroupIDs) > 0 {
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

func (s *Service) handleCreate(w http.ResponseWriter, r *http.Request) {
	var in accountInput
	if err := httpx.DecodeJSON(r, &in, 0); err != nil {
		httpx.WriteError(w, err)
		return
	}
	p := APIKeyAccountParams{Priority: DefaultPriority, Weight: 1}
	if in.Name != nil {
		p.Name = *in.Name
	}
	if in.Platform != nil {
		p.Platform = *in.Platform
	}
	if in.BaseURL != nil {
		p.BaseURL = *in.BaseURL
	}
	if in.APIKey != nil {
		p.APIKey = *in.APIKey
	}
	if in.SupportsResponses != nil {
		p.SupportsResponses = *in.SupportsResponses
	}
	if in.Priority != nil {
		p.Priority = *in.Priority
	}
	if in.Weight != nil {
		p.Weight = *in.Weight
	}
	if in.ConcurrencyLimit != nil {
		p.ConcurrencyLimit = *in.ConcurrencyLimit
	}
	if in.GroupIDs != nil {
		p.GroupIDs = *in.GroupIDs
	}
	if in.ModelMapping != nil {
		p.ModelMapping = *in.ModelMapping
	}
	if in.AllowedModels != nil {
		p.AllowedModels = *in.AllowedModels
	}
	if in.ProxyURL != nil {
		p.ProxyURL = *in.ProxyURL
	}
	a, _, err := s.CreateAPIKeyAccount(r.Context(), p)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	s.audit(r, "account.create", accountTarget(a.ID), map[string]any{
		"name": a.Name, "platform": a.Platform, "authType": a.AuthType, "baseUrl": a.BaseURL, "groupIds": a.GroupIDs,
	})
	v, err := s.view(r.Context(), a)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	httpx.WriteJSON(w, http.StatusOK, v)
}

func accountTarget(id int64) string { return "account:" + strconv.FormatInt(id, 10) }

func (s *Service) handlePatch(w http.ResponseWriter, r *http.Request) {
	id, err := httpx.PathInt64(r, "id")
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	var in accountInput
	if err := httpx.DecodeJSON(r, &in, 0); err != nil {
		httpx.WriteError(w, err)
		return
	}
	ctx := r.Context()
	a, err := s.Get(ctx, id)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	changed := []string{}
	if in.Name != nil {
		a.Name = strings.TrimSpace(*in.Name)
		changed = append(changed, "name")
		if err := validName(a.Name); err != nil {
			httpx.WriteError(w, err)
			return
		}
	}
	if in.Platform != nil && strings.TrimSpace(*in.Platform) != a.Platform {
		httpx.WriteError(w, core.BadRequest("INVALID_REQUEST", "不能修改账号平台"))
		return
	}
	if in.BaseURL != nil {
		if a.AuthType != AuthAPIKey {
			httpx.WriteError(w, core.BadRequest("NOT_SUPPORTED", "OAuth 账号不能设置 baseUrl"))
			return
		}
		base, err := normalizeBaseURL(*in.BaseURL, a.Platform)
		if err != nil {
			httpx.WriteError(w, err)
			return
		}
		a.BaseURL = base
		changed = append(changed, "baseUrl")
	}
	var sealed []byte
	if in.APIKey != nil && strings.TrimSpace(*in.APIKey) != "" {
		if a.AuthType != AuthAPIKey {
			httpx.WriteError(w, core.BadRequest("NOT_SUPPORTED", "OAuth 账号不能设置 apiKey"))
			return
		}
		key := strings.TrimSpace(*in.APIKey)
		if sealed, err = s.vault.EncryptJSON(Credentials{APIKey: key}); err != nil {
			httpx.WriteError(w, err)
			return
		}
		a.KeyHint = keyHint(key)
		changed = append(changed, "apiKey")
	}
	if in.SupportsResponses != nil {
		a.SupportsResponses = *in.SupportsResponses
		changed = append(changed, "supportsResponses")
	}
	if in.Priority != nil {
		a.Priority = *in.Priority
		changed = append(changed, "priority")
	}
	if in.Weight != nil {
		a.Weight = *in.Weight
		changed = append(changed, "weight")
	}
	if in.ConcurrencyLimit != nil {
		a.ConcurrencyLimit = *in.ConcurrencyLimit
		changed = append(changed, "concurrencyLimit")
	}
	if in.ModelMapping != nil {
		a.ModelMapping = *in.ModelMapping
		if a.ModelMapping == nil {
			a.ModelMapping = map[string]string{}
		}
		changed = append(changed, "modelMapping")
	}
	if in.ProxyURL != nil {
		a.ProxyURL = strings.TrimSpace(*in.ProxyURL)
		changed = append(changed, "proxyUrl")
	}
	if in.AllowedModels != nil {
		a.AllowedModels, err = normalizeAllowedModels(*in.AllowedModels)
		if err != nil {
			httpx.WriteError(w, err)
			return
		}
		changed = append(changed, "allowedModels")
	}
	reactivate := false
	if in.Status != nil {
		st := strings.TrimSpace(*in.Status)
		if st != StatusActive && st != StatusDisabled && st != StatusError {
			httpx.WriteError(w, core.BadRequest("INVALID_REQUEST", "status 只能是 active/disabled/error"))
			return
		}
		reactivate = st == StatusActive && a.Status != StatusActive
		a.Status = st
		changed = append(changed, "status")
	}
	if err := validateCommon(a.Priority, a.Weight, a.ConcurrencyLimit, a.ProxyURL, a.ModelMapping); err != nil {
		httpx.WriteError(w, err)
		return
	}
	var groups []int64
	if in.GroupIDs != nil {
		if groups, err = s.resolveGroups(ctx, *in.GroupIDs, false); err != nil {
			httpx.WriteError(w, err)
			return
		}
		changed = append(changed, "groupIds")
	}
	mb, _ := json.Marshal(a.ModelMapping)
	ab, _ := json.Marshal(a.AllowedModels)
	err = pgx.BeginFunc(ctx, s.db, func(tx pgx.Tx) error {
		if _, err := tx.Exec(ctx, `UPDATE upstream_accounts SET name = $2, base_url = $3, key_hint = $4,
			supports_responses = $5, priority = $6, weight = $7, concurrency_limit = $8, model_mapping = $9,
			proxy_url = $10, status = $11, credentials = COALESCE($12, credentials),
			fail_count = CASE WHEN $13 THEN 0 ELSE fail_count END, allowed_models = $14, updated_at = now()
			WHERE id = $1`,
			a.ID, a.Name, a.BaseURL, a.KeyHint, a.SupportsResponses, a.Priority, a.Weight, a.ConcurrencyLimit,
			mb, a.ProxyURL, a.Status, sealed, reactivate, ab); err != nil {
			return err
		}
		if in.GroupIDs != nil {
			return setGroups(ctx, tx, a.ID, groups, true)
		}
		return nil
	})
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	s.audit(r, "account.update", accountTarget(a.ID), map[string]any{"fields": changed})
	s.writeAccount(w, r, a.ID)
}

func (s *Service) handleDelete(w http.ResponseWriter, r *http.Request) {
	id, err := httpx.PathInt64(r, "id")
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	var name string
	err = s.db.QueryRow(r.Context(), `DELETE FROM upstream_accounts WHERE id = $1 RETURNING name`, id).Scan(&name)
	if errors.Is(err, pgx.ErrNoRows) {
		httpx.WriteError(w, errAccountNotFound())
		return
	}
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	s.audit(r, "account.delete", accountTarget(id), map[string]any{"name": name})
	httpx.OK(w)
}

func (s *Service) handleClearCooldown(w http.ResponseWriter, r *http.Request) {
	id, err := httpx.PathInt64(r, "id")
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	tag, err := s.db.Exec(r.Context(), `UPDATE upstream_accounts SET cooldown_until = NULL, updated_at = now() WHERE id = $1`, id)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	if tag.RowsAffected() == 0 {
		httpx.WriteError(w, errAccountNotFound())
		return
	}
	s.audit(r, "account.clear_cooldown", accountTarget(id), nil)
	s.writeAccount(w, r, id)
}

func (s *Service) handleRefresh(w http.ResponseWriter, r *http.Request) {
	id, err := httpx.PathInt64(r, "id")
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	_, err = s.RefreshAccount(r.Context(), id)
	s.audit(r, "account.refresh", accountTarget(id), map[string]any{"ok": err == nil})
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	s.writeAccount(w, r, id)
}

func isUniqueViolation(err error) bool {
	var pgErr *pgconn.PgError
	return errors.As(err, &pgErr) && pgErr.Code == "23505"
}
