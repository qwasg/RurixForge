package admin

import (
	"encoding/json"
	"io"
	"net/http"
	"strings"
	"unicode/utf8"

	"forge-cloud/internal/billing"
	"forge-cloud/internal/core"
	"forge-cloud/internal/httpx"
)

// 订单管理（15_CLOUD_SERVICE.md §11.3）。

func (s *Service) handleListOrders(w http.ResponseWriter, r *http.Request) {
	limit, offset := httpx.Pagination(r)
	q := r.URL.Query()
	f := billing.OrderFilter{
		UserID: httpx.QueryInt64(r, "userId"),
		Status: strings.TrimSpace(q.Get("status")),
		Kind:   strings.TrimSpace(q.Get("kind")),
		Q:      strings.TrimSpace(q.Get("q")),
	}
	items, total, err := billing.ListOrders(r.Context(), s.d.DB, f, limit, offset)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	httpx.WriteJSON(w, http.StatusOK, map[string]any{"items": items, "total": total})
}

// decodeOptionalJSON 解析可为空的 JSON 请求体（空体或纯空白视为 {}）。
func decodeOptionalJSON(r *http.Request, dst any) error {
	raw, err := io.ReadAll(http.MaxBytesReader(nil, r.Body, httpx.MaxJSONBody))
	if err != nil {
		return core.E(http.StatusRequestEntityTooLarge, "BODY_TOO_LARGE", "请求体过大")
	}
	if strings.TrimSpace(string(raw)) == "" {
		return nil
	}
	if err := json.Unmarshal(raw, dst); err != nil {
		return core.BadRequest("INVALID_JSON", "请求体不是合法 JSON")
	}
	return nil
}

// handleMarkOrderPaid：线下确认收款，pending 订单按 §11.1 生效。
func (s *Service) handleMarkOrderPaid(w http.ResponseWriter, r *http.Request) {
	id, err := httpx.PathInt64(r, "id")
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	var req struct {
		Note string `json:"note"`
	}
	if err := decodeOptionalJSON(r, &req); err != nil {
		httpx.WriteError(w, err)
		return
	}
	note := strings.TrimSpace(req.Note)
	if utf8.RuneCountInString(note) > 200 {
		httpx.WriteError(w, errInvalid("备注不能超过 200 个字"))
		return
	}
	o, err := s.d.Billing.MarkOrderPaid(r.Context(), id, note)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	s.audit(r, "order.mark_paid", target("order", id), map[string]any{
		"userId": o.UserID, "kind": o.Kind, "amountMicros": o.AmountMicros, "note": note,
	})
	httpx.WriteJSON(w, http.StatusOK, o)
}

func (s *Service) handleCancelOrder(w http.ResponseWriter, r *http.Request) {
	id, err := httpx.PathInt64(r, "id")
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	o, err := billing.CancelOrder(r.Context(), s.d.DB, 0, id)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	s.audit(r, "order.cancel", target("order", id), map[string]any{"userId": o.UserID, "kind": o.Kind})
	httpx.WriteJSON(w, http.StatusOK, o)
}
