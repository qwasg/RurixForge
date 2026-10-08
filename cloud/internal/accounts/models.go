package accounts

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"net/url"
	"strings"
	"time"
	"unicode"

	"github.com/go-chi/chi/v5"
	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"

	"forge-cloud/internal/core"
	"forge-cloud/internal/httpx"
)

// CatalogModel 是模型目录条目（附创建时间，供 /v1/models 的 created 字段）。
type CatalogModel struct {
	core.Model
	CreatedAt time.Time
}

type modelCache struct {
	at   time.Time
	list []CatalogModel
	byID map[string]int
}

// 多实例下管理端改动最多 5 秒后在其它实例生效。
const modelCacheTTL = 5 * time.Second

const modelCols = `id, display_name, platform, upstream_model, capabilities, price_input_micros, price_output_micros,
	price_cache_read_micros, price_cache_write_micros, pool, enabled, is_default, sort, created_at`

func scanModel(row pgx.Row) (*CatalogModel, error) {
	m := &CatalogModel{}
	var caps []byte
	err := row.Scan(&m.ID, &m.DisplayName, &m.Platform, &m.UpstreamModel, &caps,
		&m.Pricing.InputPer1M, &m.Pricing.OutputPer1M, &m.Pricing.CacheReadPer1M, &m.Pricing.CacheWritePer1M,
		&m.Pool, &m.Enabled, &m.IsDefault, &m.Sort, &m.CreatedAt)
	if err != nil {
		return nil, err
	}
	if len(caps) > 0 {
		_ = json.Unmarshal(caps, &m.Capabilities)
	}
	if m.Capabilities.ReasoningEfforts == nil {
		m.Capabilities.ReasoningEfforts = []string{}
	}
	m.CreatedAt = m.CreatedAt.UTC()
	return m, nil
}

func (s *Service) queryModels(ctx context.Context, enabledOnly bool) ([]CatalogModel, error) {
	q := `SELECT ` + modelCols + ` FROM models`
	if enabledOnly {
		q += ` WHERE enabled`
	}
	rows, err := s.db.Query(ctx, q+` ORDER BY sort, id`)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	var out []CatalogModel
	for rows.Next() {
		m, err := scanModel(rows)
		if err != nil {
			return nil, err
		}
		out = append(out, *m)
	}
	return out, rows.Err()
}

func (s *Service) enabledModels(ctx context.Context) (*modelCache, error) {
	s.modelsMu.Lock()
	if c := s.models; c != nil && time.Since(c.at) < modelCacheTTL {
		s.modelsMu.Unlock()
		return c, nil
	}
	s.modelsMu.Unlock()
	list, err := s.queryModels(ctx, true)
	if err != nil {
		return nil, err
	}
	c := &modelCache{at: time.Now(), list: list, byID: make(map[string]int, len(list))}
	for i, m := range list {
		c.byID[m.ID] = i
	}
	s.modelsMu.Lock()
	s.models = c
	s.modelsMu.Unlock()
	return c, nil
}

// InvalidateModels 清空本实例的模型缓存。
func (s *Service) InvalidateModels() {
	s.modelsMu.Lock()
	s.models = nil
	s.modelsMu.Unlock()
}

// EnabledModels 返回启用的模型（按 sort、id 排序）。
func (s *Service) EnabledModels(ctx context.Context) ([]CatalogModel, error) {
	c, err := s.enabledModels(ctx)
	if err != nil {
		return nil, err
	}
	return append([]CatalogModel(nil), c.list...), nil
}

// Model 返回启用的模型；不存在或已停用返回 404 MODEL_NOT_FOUND。
func (s *Service) Model(ctx context.Context, id string) (*core.Model, error) {
	c, err := s.enabledModels(ctx)
	if err != nil {
		return nil, err
	}
	i, ok := c.byID[id]
	if !ok {
		return nil, core.NotFound(core.CodeModelNotFound, "模型不存在或未启用")
	}
	m := c.list[i].Model
	return &m, nil
}

// ---------- 管理接口 /models ----------

type adminModel struct {
	core.Model
	AvailableAccounts int `json:"availableAccounts"`
}

type modelInput struct {
	ID            *string                 `json:"id"`
	DisplayName   *string                 `json:"displayName"`
	Platform      *string                 `json:"platform"`
	UpstreamModel *string                 `json:"upstreamModel"`
	Capabilities  *core.ModelCapabilities `json:"capabilities"`
	Pricing       *core.Pricing           `json:"pricing"`
	Pool          *string                 `json:"pool"`
	Enabled       *bool                   `json:"enabled"`
	IsDefault     *bool                   `json:"isDefault"`
	Sort          *int                    `json:"sort"`
}

func (in *modelInput) apply(m *core.Model) {
	if in.DisplayName != nil {
		m.DisplayName = strings.TrimSpace(*in.DisplayName)
	}
	if in.Platform != nil {
		m.Platform = strings.TrimSpace(*in.Platform)
	}
	if in.UpstreamModel != nil {
		m.UpstreamModel = strings.TrimSpace(*in.UpstreamModel)
	}
	if in.Capabilities != nil {
		m.Capabilities = *in.Capabilities
	}
	if in.Pricing != nil {
		m.Pricing = *in.Pricing
	}
	if in.Pool != nil {
		m.Pool = strings.TrimSpace(*in.Pool)
	}
	if in.Enabled != nil {
		m.Enabled = *in.Enabled
	}
	if in.IsDefault != nil {
		m.IsDefault = *in.IsDefault
	}
	if in.Sort != nil {
		m.Sort = *in.Sort
	}
}

func validModelName(s string) bool {
	if s == "" || len(s) > 128 {
		return false
	}
	for _, r := range s {
		if unicode.IsSpace(r) || unicode.IsControl(r) {
			return false
		}
	}
	return true
}

func normalizeModel(m *core.Model) error {
	if !validModelName(m.ID) {
		return core.BadRequest("INVALID_REQUEST", "模型 ID 必填，不超过 128 个字符且不含空白")
	}
	if m.Platform != PlatformOpenAI && m.Platform != PlatformAnthropic {
		return core.BadRequest("INVALID_REQUEST", "platform 只能是 openai 或 anthropic")
	}
	if m.UpstreamModel != "" && !validModelName(m.UpstreamModel) {
		return core.BadRequest("INVALID_REQUEST", "upstreamModel 不超过 128 个字符且不含空白")
	}
	if len([]rune(m.DisplayName)) > 128 {
		return core.BadRequest("INVALID_REQUEST", "displayName 不超过 128 个字符")
	}
	switch m.Pool {
	case "":
		m.Pool = core.PoolAPI
	case core.PoolAPI, core.PoolForge:
	default:
		return core.BadRequest("INVALID_REQUEST", "pool 只能是 api 或 forge")
	}
	p := m.Pricing
	if p.InputPer1M < 0 || p.OutputPer1M < 0 || p.CacheReadPer1M < 0 || p.CacheWritePer1M < 0 {
		return core.BadRequest("INVALID_REQUEST", "单价不能为负")
	}
	c := &m.Capabilities
	if c.ThinkingMode != "" && c.ThinkingMode != "manual" && c.ThinkingMode != "adaptive" {
		return core.BadRequest("INVALID_REQUEST", "thinkingMode 只能是 manual 或 adaptive")
	}
	if c.ThinkingAlwaysOn && c.ThinkingMode != "adaptive" {
		return core.BadRequest("INVALID_REQUEST", "thinkingAlwaysOn 需要 adaptive 模式")
	}
	if c.ContextWindow < 0 || c.MaxOutput < 0 {
		return core.BadRequest("INVALID_REQUEST", "contextWindow / maxOutput 不能为负")
	}
	efforts := make([]string, 0, len(c.ReasoningEfforts))
	seen := map[string]bool{}
	for _, e := range c.ReasoningEfforts {
		e = strings.TrimSpace(e)
		if e == "" || len(e) > 16 {
			return core.BadRequest("INVALID_REQUEST", "reasoningEfforts 取值非法")
		}
		if !seen[e] {
			seen[e] = true
			efforts = append(efforts, e)
		}
	}
	c.ReasoningEfforts = efforts
	return nil
}

// pathModelID 取路径里的模型 ID（ID 可含 `/`，需 URL 编码；chi 按 RawPath 路由，这里解码）。
func pathModelID(r *http.Request) (string, error) {
	raw := chi.URLParam(r, "id")
	id := raw
	if r.URL.RawPath != "" {
		v, err := url.PathUnescape(raw)
		if err != nil {
			return "", core.BadRequest("INVALID_ID", "模型 ID 非法")
		}
		id = v
	}
	if strings.TrimSpace(id) == "" {
		return "", core.BadRequest("INVALID_ID", "模型 ID 非法")
	}
	return id, nil
}

// AvailableModelCounts 统计分组内实际支持各模型的健康账号；每个平台只查询一次。
func (s *Service) AvailableModelCounts(ctx context.Context, groupID int64, models []CatalogModel) (map[string]int, error) {
	byPlatform := map[string][]*Account{}
	out := map[string]int{}
	for _, m := range models {
		candidates, loaded := byPlatform[m.Platform]
		if !loaded {
			var err error
			candidates, err = s.Candidates(ctx, groupID, m.Platform)
			if err != nil {
				return nil, err
			}
			byPlatform[m.Platform] = candidates
		}
		for _, a := range candidates {
			if a.SupportsModel(&m.Model) {
				out[m.ID]++
			}
		}
	}
	return out, nil
}

func (s *Service) adminModelView(ctx context.Context, m core.Model) (adminModel, error) {
	avail, err := s.AvailableModelCounts(ctx, 0, []CatalogModel{{Model: m}})
	if err != nil {
		return adminModel{}, err
	}
	return adminModel{Model: m, AvailableAccounts: avail[m.ID]}, nil
}

func (s *Service) getModel(ctx context.Context, id string) (*core.Model, error) {
	m, err := scanModel(s.db.QueryRow(ctx, `SELECT `+modelCols+` FROM models WHERE id = $1`, id))
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, core.NotFound(core.CodeModelNotFound, "模型不存在")
	}
	if err != nil {
		return nil, err
	}
	return &m.Model, nil
}

func (s *Service) handleListModels(w http.ResponseWriter, r *http.Request) {
	list, err := s.queryModels(r.Context(), false)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	avail, err := s.AvailableModelCounts(r.Context(), 0, list)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	items := make([]adminModel, 0, len(list))
	for _, m := range list {
		items = append(items, adminModel{Model: m.Model, AvailableAccounts: avail[m.ID]})
	}
	httpx.WriteJSON(w, http.StatusOK, map[string]any{"items": items})
}

// CreateModel 新建模型（管理接口与测试共用）；ID 重复返回 409 MODEL_EXISTS。
func (s *Service) CreateModel(ctx context.Context, m core.Model) (*core.Model, error) {
	m.ID = strings.TrimSpace(m.ID)
	if err := normalizeModel(&m); err != nil {
		return nil, err
	}
	caps, _ := json.Marshal(m.Capabilities)
	err := pgx.BeginFunc(ctx, s.db, func(tx pgx.Tx) error {
		if _, err := tx.Exec(ctx, `INSERT INTO models (id, display_name, platform, upstream_model, capabilities,
			price_input_micros, price_output_micros, price_cache_read_micros, price_cache_write_micros, pool, enabled, is_default, sort)
			VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)`,
			m.ID, m.DisplayName, m.Platform, m.UpstreamModel, caps,
			m.Pricing.InputPer1M, m.Pricing.OutputPer1M, m.Pricing.CacheReadPer1M, m.Pricing.CacheWritePer1M,
			m.Pool, m.Enabled, m.IsDefault, m.Sort); err != nil {
			return err
		}
		return clearOtherDefaults(ctx, tx, m)
	})
	if err != nil {
		var pgErr *pgconn.PgError
		if errors.As(err, &pgErr) && pgErr.Code == "23505" {
			return nil, core.Conflict("MODEL_EXISTS", "模型 ID 已存在")
		}
		return nil, err
	}
	s.InvalidateModels()
	return s.getModel(ctx, m.ID)
}

func clearOtherDefaults(ctx context.Context, tx pgx.Tx, m core.Model) error {
	if !m.IsDefault {
		return nil
	}
	_, err := tx.Exec(ctx, `UPDATE models SET is_default = FALSE, updated_at = now() WHERE is_default AND id <> $1`, m.ID)
	return err
}

func (s *Service) handleCreateModel(w http.ResponseWriter, r *http.Request) {
	var in modelInput
	if err := httpx.DecodeJSON(r, &in, 0); err != nil {
		httpx.WriteError(w, err)
		return
	}
	m := core.Model{Enabled: true}
	if in.ID != nil {
		m.ID = *in.ID
	}
	in.apply(&m)
	created, err := s.CreateModel(r.Context(), m)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	s.audit(r, "model.create", "model:"+created.ID, map[string]any{
		"platform": created.Platform, "upstreamModel": created.UpstreamModel, "pricing": created.Pricing,
		"enabled": created.Enabled, "isDefault": created.IsDefault,
	})
	s.writeAdminModel(w, r, *created)
}

func (s *Service) handlePatchModel(w http.ResponseWriter, r *http.Request) {
	id, err := pathModelID(r)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	var in modelInput
	if err := httpx.DecodeJSON(r, &in, 0); err != nil {
		httpx.WriteError(w, err)
		return
	}
	ctx := r.Context()
	m, err := s.getModel(ctx, id)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	in.apply(m)
	if err := normalizeModel(m); err != nil {
		httpx.WriteError(w, err)
		return
	}
	caps, _ := json.Marshal(m.Capabilities)
	err = pgx.BeginFunc(ctx, s.db, func(tx pgx.Tx) error {
		if _, err := tx.Exec(ctx, `UPDATE models SET display_name = $2, platform = $3, upstream_model = $4,
			capabilities = $5, price_input_micros = $6, price_output_micros = $7, price_cache_read_micros = $8,
			price_cache_write_micros = $9, enabled = $10, is_default = $11, sort = $12, pool = $13, updated_at = now()
			WHERE id = $1`,
			m.ID, m.DisplayName, m.Platform, m.UpstreamModel, caps,
			m.Pricing.InputPer1M, m.Pricing.OutputPer1M, m.Pricing.CacheReadPer1M, m.Pricing.CacheWritePer1M,
			m.Enabled, m.IsDefault, m.Sort, m.Pool); err != nil {
			return err
		}
		return clearOtherDefaults(ctx, tx, *m)
	})
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	s.InvalidateModels()
	s.audit(r, "model.update", "model:"+m.ID, map[string]any{"fields": in.fieldNames()})
	updated, err := s.getModel(ctx, id)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	s.writeAdminModel(w, r, *updated)
}

func (in *modelInput) fieldNames() []string {
	var out []string
	add := func(set bool, name string) {
		if set {
			out = append(out, name)
		}
	}
	add(in.DisplayName != nil, "displayName")
	add(in.Platform != nil, "platform")
	add(in.UpstreamModel != nil, "upstreamModel")
	add(in.Capabilities != nil, "capabilities")
	add(in.Pricing != nil, "pricing")
	add(in.Pool != nil, "pool")
	add(in.Enabled != nil, "enabled")
	add(in.IsDefault != nil, "isDefault")
	add(in.Sort != nil, "sort")
	return out
}

func (s *Service) handleDeleteModel(w http.ResponseWriter, r *http.Request) {
	id, err := pathModelID(r)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	tag, err := s.db.Exec(r.Context(), `DELETE FROM models WHERE id = $1`, id)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	if tag.RowsAffected() == 0 {
		httpx.WriteError(w, core.NotFound(core.CodeModelNotFound, "模型不存在"))
		return
	}
	s.InvalidateModels()
	s.audit(r, "model.delete", "model:"+id, nil)
	httpx.OK(w)
}

func (s *Service) writeAdminModel(w http.ResponseWriter, r *http.Request, m core.Model) {
	v, err := s.adminModelView(r.Context(), m)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	httpx.WriteJSON(w, http.StatusOK, v)
}
