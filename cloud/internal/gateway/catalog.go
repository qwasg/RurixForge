package gateway

import (
	"net/http"

	"forge-cloud/internal/core"
	"forge-cloud/internal/httpx"
)

// handleModels：GET /v1/models（OpenAI 列表格式，只含有效分组可用的启用模型）。
func (s *Service) handleModels(w http.ResponseWriter, r *http.Request) {
	requestID(w, r)
	fail := func(err error) {
		e := core.AsError(err)
		if e == nil {
			s.d.Log.Error("list models failed", "err", err)
			e = core.E(http.StatusInternalServerError, "INTERNAL", "服务器内部错误")
		}
		writeGatewayError(w, "", e)
	}
	p, err := s.authenticate(r)
	if err != nil {
		fail(err)
		return
	}
	list, err := s.d.Accounts.EnabledModels(r.Context())
	if err != nil {
		fail(err)
		return
	}
	data := make([]map[string]any, 0, len(list))
	for _, m := range list {
		if !p.Group.AllowsModel(m.ID) {
			continue
		}
		data = append(data, map[string]any{
			"id": m.ID, "object": "model", "created": m.CreatedAt.Unix(), "owned_by": m.Platform,
		})
	}
	httpx.WriteJSON(w, http.StatusOK, map[string]any{"object": "list", "data": data})
}

type catalogModel struct {
	ID           string                 `json:"id"`
	DisplayName  string                 `json:"displayName"`
	Platform     string                 `json:"platform"`
	Capabilities core.ModelCapabilities `json:"capabilities"`
	Pricing      core.Pricing           `json:"pricing"`
	// Pool：计费用量池（api|forge，§11.1）。
	Pool      string `json:"pool"`
	Available bool   `json:"available"`
}

// handleCatalog：GET /api/v1/models/catalog（§3.3、§12）。价格乘有效分组倍率；available = 分组内有支持该模型的健康账号；
// defaultModel = 系统设置 defaultModel（启用且分组可用）> is_default 模型 > 第一个模型。
func (s *Service) handleCatalog(w http.ResponseWriter, r *http.Request) {
	ctx := r.Context()
	claims, ok := core.ClaimsFrom(ctx)
	if !ok {
		httpx.WriteError(w, core.Unauthorized("UNAUTHORIZED", "未登录"))
		return
	}
	p, err := s.d.Principals.PrincipalForUser(ctx, claims.UserID)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	list, err := s.d.Accounts.EnabledModels(ctx)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	healthy, err := s.d.Accounts.AvailableModelCounts(ctx, p.Group.ID, list)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	st, err := s.d.Settings.Get(ctx)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	mult := p.Group.RateMultiplier
	models := make([]catalogModel, 0, len(list))
	markedDefault := ""
	for _, m := range list {
		if !p.Group.AllowsModel(m.ID) {
			continue
		}
		name := m.DisplayName
		if name == "" {
			name = m.ID
		}
		models = append(models, catalogModel{
			ID: m.ID, DisplayName: name, Platform: m.Platform, Capabilities: m.Capabilities,
			Pricing: m.Pricing.Scaled(mult), Pool: core.NormalizePool(m.Pool), Available: healthy[m.ID] > 0,
		})
		if m.IsDefault && markedDefault == "" {
			markedDefault = m.ID
		}
	}
	def := ""
	for _, m := range models {
		if st.DefaultModel != "" && m.ID == st.DefaultModel {
			def = m.ID
			break
		}
	}
	if def == "" {
		def = markedDefault
	}
	if def == "" && len(models) > 0 {
		def = models[0].ID
	}
	httpx.WriteJSON(w, http.StatusOK, map[string]any{
		"defaultModel": def, "currency": st.Currency, "rateMultiplier": mult, "models": models,
	})
}
