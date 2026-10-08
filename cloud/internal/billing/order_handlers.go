package billing

import (
	"net/http"

	"forge-cloud/internal/core"
	"forge-cloud/internal/httpx"
)

// 会员购买与订单的用户接口（§11.2）。

type planRequest struct {
	PlanID   int64  `json:"planId"`
	Interval string `json:"interval"`
	Provider string `json:"provider"`
}

func decodePlanRequest(r *http.Request) (planRequest, error) {
	var req planRequest
	if err := httpx.DecodeJSON(r, &req, 0); err != nil {
		return req, err
	}
	if req.PlanID <= 0 {
		return req, core.BadRequest("INVALID_REQUEST", "planId 必填")
	}
	return req, nil
}

func (s *Service) handleQuote(w http.ResponseWriter, r *http.Request) {
	c, err := claims(r)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	req, err := decodePlanRequest(r)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	q, err := s.Quote(r.Context(), c.UserID, req.PlanID, req.Interval)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	httpx.WriteJSON(w, http.StatusOK, q)
}

func (s *Service) handleCheckout(w http.ResponseWriter, r *http.Request) {
	c, err := claims(r)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	req, err := decodePlanRequest(r)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	ctx := r.Context()
	order, err := s.Checkout(ctx, c.UserID, req.PlanID, req.Interval, req.Provider)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	m, err := s.GetMembership(ctx, c.UserID)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	httpx.WriteJSON(w, http.StatusOK, map[string]any{"order": order, "membership": m})
}

func (s *Service) handleCancelScheduled(w http.ResponseWriter, r *http.Request) {
	c, err := claims(r)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	id, err := httpx.PathInt64(r, "id")
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	ctx := r.Context()
	if err := s.CancelScheduled(ctx, c.UserID, id); err != nil {
		httpx.WriteError(w, err)
		return
	}
	m, err := s.GetMembership(ctx, c.UserID)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	httpx.WriteJSON(w, http.StatusOK, m)
}

func (s *Service) handleListOrders(w http.ResponseWriter, r *http.Request) {
	c, err := claims(r)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	limit, offset := httpx.Pagination(r)
	items, total, err := ListOrders(r.Context(), s.db, OrderFilter{UserID: c.UserID}, limit, offset)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	out := make([]Order, 0, len(items))
	for _, it := range items {
		out = append(out, it.Order)
	}
	httpx.WriteJSON(w, http.StatusOK, map[string]any{"items": out, "total": total})
}

func (s *Service) handleCancelOrder(w http.ResponseWriter, r *http.Request) {
	c, err := claims(r)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	id, err := httpx.PathInt64(r, "id")
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	o, err := CancelOrder(r.Context(), s.db, c.UserID, id)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	httpx.WriteJSON(w, http.StatusOK, o.Order)
}
