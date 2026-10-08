package billing

import (
	"net/http"
	"strconv"
	"strings"
	"time"

	"github.com/go-chi/chi/v5"

	"forge-cloud/internal/core"
	"forge-cloud/internal/httpx"
	"forge-cloud/internal/usage"
)

func claims(r *http.Request) (*core.Claims, error) {
	c, ok := core.ClaimsFrom(r.Context())
	if !ok {
		return nil, core.Unauthorized("UNAUTHORIZED", "未登录")
	}
	return c, nil
}

func errPaymentNotConfigured() error {
	return core.E(http.StatusNotImplemented, "PAYMENT_NOT_CONFIGURED", "未配置在线支付渠道")
}

func (s *Service) handlePlans(w http.ResponseWriter, r *http.Request) {
	items, err := ListPublicPlans(r.Context(), s.db)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	httpx.WriteJSON(w, http.StatusOK, map[string]any{"items": items})
}

func (s *Service) handleBalance(w http.ResponseWriter, r *http.Request) {
	c, err := claims(r)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	ctx := r.Context()
	var balance int64
	if err := s.db.QueryRow(ctx, `SELECT balance_micros FROM users WHERE id = $1`, c.UserID).Scan(&balance); err != nil {
		httpx.WriteError(w, err)
		return
	}
	subs, err := ListSubscriptions(ctx, s.db, c.UserID, true)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	st, err := s.settings.Get(ctx)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	httpx.WriteJSON(w, http.StatusOK, map[string]any{
		"balanceMicros": balance, "currency": st.Currency, "subscriptions": subs,
	})
}

func (s *Service) handleSubscriptions(w http.ResponseWriter, r *http.Request) {
	c, err := claims(r)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	subs, err := ListSubscriptions(r.Context(), s.db, c.UserID, false)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	httpx.WriteJSON(w, http.StatusOK, map[string]any{"items": subs})
}

func (s *Service) handleUsage(w http.ResponseWriter, r *http.Request) {
	c, err := claims(r)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	limit, offset := httpx.Pagination(r)
	f := usage.Filter{UserID: c.UserID, From: httpx.QueryTime(r, "from"), To: httpx.QueryTime(r, "to")}
	items, total, err := usage.List(r.Context(), s.db, f, limit, offset)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	sum, err := usage.Summarize(r.Context(), s.db, f)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	httpx.WriteJSON(w, http.StatusOK, map[string]any{"items": items, "total": total, "summary": sum})
}

func (s *Service) handleUsageDaily(w http.ResponseWriter, r *http.Request) {
	c, err := claims(r)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	days := 30
	if v, err := strconv.Atoi(r.URL.Query().Get("days")); err == nil && v > 0 {
		days = min(v, 366)
	}
	items, err := usage.Daily(r.Context(), s.db, c.UserID, days, time.Now())
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	httpx.WriteJSON(w, http.StatusOK, map[string]any{"items": items})
}

func (s *Service) handleLedger(w http.ResponseWriter, r *http.Request) {
	c, err := claims(r)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	limit, offset := httpx.Pagination(r)
	items, total, err := ListLedger(r.Context(), s.db, c.UserID, limit, offset)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	httpx.WriteJSON(w, http.StatusOK, map[string]any{"items": items, "total": total})
}

func (s *Service) handleCreateOrder(w http.ResponseWriter, r *http.Request) {
	c, err := claims(r)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	var req struct {
		AmountMicros int64  `json:"amountMicros"`
		Provider     string `json:"provider"`
	}
	if err := httpx.DecodeJSON(r, &req, 0); err != nil {
		httpx.WriteError(w, err)
		return
	}
	p, ok := s.payment(strings.TrimSpace(req.Provider))
	if !ok {
		httpx.WriteError(w, errPaymentNotConfigured())
		return
	}
	if req.AmountMicros <= 0 {
		httpx.WriteError(w, core.BadRequest("INVALID_REQUEST", "金额必须大于 0"))
		return
	}
	ctx := r.Context()
	var orderID int64
	if err := s.db.QueryRow(ctx,
		`INSERT INTO payment_orders (user_id, provider, amount_micros) VALUES ($1, $2, $3) RETURNING id`,
		c.UserID, p.Name(), req.AmountMicros).Scan(&orderID); err != nil {
		httpx.WriteError(w, err)
		return
	}
	payURL, err := p.CreateOrder(ctx, orderID, c.UserID, req.AmountMicros)
	if err != nil {
		s.log.Warn("create payment order failed", "provider", p.Name(), "order", orderID, "err", err)
		_, _ = s.db.Exec(ctx, `UPDATE payment_orders SET status = 'failed', updated_at = now() WHERE id = $1`, orderID)
		httpx.WriteError(w, core.E(http.StatusBadGateway, "PAYMENT_FAILED", "创建支付单失败"))
		return
	}
	httpx.WriteJSON(w, http.StatusOK, map[string]any{"orderId": orderID, "payUrl": payURL, "status": "pending"})
}

// handleNotify 处理支付回调（幂等）：充值单入账、会员单生效，规则见 applyNotify。
func (s *Service) handleNotify(w http.ResponseWriter, r *http.Request) {
	p, ok := s.payment(chi.URLParam(r, "provider"))
	if !ok {
		httpx.WriteError(w, errPaymentNotConfigured())
		return
	}
	orderID, paid, err := p.HandleNotify(r)
	if err != nil {
		s.log.Warn("payment notify rejected", "provider", p.Name(), "err", err)
		httpx.WriteError(w, core.BadRequest("PAYMENT_NOTIFY_INVALID", "支付回调校验失败"))
		return
	}
	if err := s.applyNotify(r.Context(), p.Name(), orderID, paid); err != nil {
		httpx.WriteError(w, err)
		return
	}
	httpx.OK(w)
}
