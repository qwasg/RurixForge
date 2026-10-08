package billing

import (
	"context"
	"errors"
	"net/http"
	"sort"
	"time"

	"github.com/jackc/pgx/v5"

	"forge-cloud/internal/core"
	"forge-cloud/internal/httpx"
	"forge-cloud/internal/usage"
)

// 会员梯度（15_CLOUD_SERVICE.md §11）。

const (
	TierHobby = "hobby"

	IntervalMonth = "month"
	IntervalYear  = "year"

	SourceGrant    = "grant"
	SourceRedeem   = "redeem"
	SourcePurchase = "purchase"

	// maxOnDemandLimit 是按量付费上限的取值上限（100 万额度单位）。
	maxOnDemandLimit = 1_000_000 * 1_000_000
)

// Tier 是价格页上的一个档位（plans.tier 非空的套餐）。
type Tier struct {
	PlanID              int64    `json:"planId"`
	Tier                string   `json:"tier"`
	Name                string   `json:"name"`
	Tagline             string   `json:"tagline"`
	Description         string   `json:"description"`
	Features            []string `json:"features"`
	PriceMonthlyMicros  int64    `json:"priceMonthlyMicros"`
	PriceYearlyMicros   int64    `json:"priceYearlyMicros"`
	IncludedAPIMicros   int64    `json:"includedApiMicros"`
	IncludedForgeMicros int64    `json:"includedForgeMicros"`
	DailyLimitMicros    int64    `json:"dailyLimitMicros"`
	Highlight           bool     `json:"highlight"`
	Rank                int      `json:"rank"`
	Enabled             bool     `json:"-"`
	GroupID             *int64   `json:"-"`
}

const tierSelect = `SELECT id, tier, name, tagline, description, features, price_micros, price_yearly_micros,
	quota_micros, forge_quota_micros, daily_limit_micros, highlight, tier_rank, enabled, group_id FROM plans`

func scanTier(row pgx.Row) (Tier, error) {
	var t Tier
	err := row.Scan(&t.PlanID, &t.Tier, &t.Name, &t.Tagline, &t.Description, &t.Features, &t.PriceMonthlyMicros,
		&t.PriceYearlyMicros, &t.IncludedAPIMicros, &t.IncludedForgeMicros, &t.DailyLimitMicros, &t.Highlight,
		&t.Rank, &t.Enabled, &t.GroupID)
	if t.Features == nil {
		t.Features = []string{}
	}
	return t, err
}

// ListTiers 返回启用中的档位（按 tier_rank、价格排序）。
func ListTiers(ctx context.Context, q Querier) ([]Tier, error) {
	rows, err := q.Query(ctx, tierSelect+` WHERE tier <> '' AND enabled ORDER BY tier_rank, price_micros, id`)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := []Tier{}
	for rows.Next() {
		t, err := scanTier(rows)
		if err != nil {
			return nil, err
		}
		out = append(out, t)
	}
	return out, rows.Err()
}

// loadTier 读取档位套餐（含停用的）；不是档位 → 404 PLAN_NOT_FOUND。
func loadTier(ctx context.Context, q Querier, planID int64) (Tier, error) {
	t, err := scanTier(q.QueryRow(ctx, tierSelect+` WHERE id = $1 AND tier <> ''`, planID))
	if errors.Is(err, pgx.ErrNoRows) {
		return t, core.NotFound("PLAN_NOT_FOUND", "会员档位不存在")
	}
	return t, err
}

// PaymentView 是可用的在线支付渠道。
type PaymentView struct {
	Enabled   bool     `json:"enabled"`
	Providers []string `json:"providers"`
}

func (s *Service) paymentInfo() PaymentView {
	s.payMu.RLock()
	defer s.payMu.RUnlock()
	names := make([]string, 0, len(s.payments))
	for n := range s.payments {
		names = append(names, n)
	}
	sort.Strings(names)
	return PaymentView{Enabled: len(names) > 0, Providers: names}
}

// TierRef 是用户当前的有效档位。
type TierRef struct {
	PlanID int64  `json:"planId"`
	Tier   string `json:"tier"`
	Name   string `json:"name"`
	Rank   int    `json:"rank"`
}

// PoolView 是一个用量池本周期的套餐内用量。
type PoolView struct {
	IncludedMicros  int64 `json:"includedMicros"`
	UsedMicros      int64 `json:"usedMicros"`
	RemainingMicros int64 `json:"remainingMicros"`
}

func poolView(included, used int64) PoolView {
	return PoolView{IncludedMicros: included, UsedMicros: used, RemainingMicros: max(included-used, 0)}
}

type CycleView struct {
	Start time.Time `json:"start"`
	End   time.Time `json:"end"`
}

type OnDemandView struct {
	Enabled     bool  `json:"enabled"`
	LimitMicros int64 `json:"limitMicros"`
	UsedMicros  int64 `json:"usedMicros"`
}

// Membership 是 GET /me/membership 的响应。
type Membership struct {
	Currency      string              `json:"currency"`
	Tier          TierRef             `json:"tier"`
	Subscription  *Subscription       `json:"subscription"`
	Scheduled     []Subscription      `json:"scheduled"`
	Packs         []Subscription      `json:"packs"`
	Cycle         CycleView           `json:"cycle"`
	Pools         map[string]PoolView `json:"pools"`
	OnDemand      OnDemandView        `json:"onDemand"`
	BalanceMicros int64               `json:"balanceMicros"`
	Payment       PaymentView         `json:"payment"`
	PendingOrder  *Order              `json:"pendingOrder"`
}

// GetMembership 汇总用户的有效档位、本周期两池用量、按量付费与待支付订单。
func (s *Service) GetMembership(ctx context.Context, userID int64) (*Membership, error) {
	now := time.Now()
	st, err := loadBillingState(ctx, s.db, userID, now, false)
	if errors.Is(err, errBillingUserNotFound) {
		return nil, core.NotFound("USER_NOT_FOUND", "用户不存在")
	}
	if err != nil {
		return nil, err
	}
	settings, err := s.settings.Get(ctx)
	if err != nil {
		return nil, err
	}
	out := &Membership{
		Currency:      settings.Currency,
		Scheduled:     []Subscription{},
		Packs:         []Subscription{},
		Cycle:         CycleView{Start: st.cycleStart, End: st.cycleEnd},
		BalanceMicros: st.balance,
		Payment:       s.paymentInfo(),
		OnDemand:      OnDemandView{Enabled: st.onDemand, LimitMicros: st.onDemandLimit},
	}
	active, err := ListSubscriptions(ctx, s.db, userID, true)
	if err != nil {
		return nil, err
	}
	for i := range active {
		sub := active[i]
		if st.tier != nil && sub.ID == st.tier.subID {
			out.Subscription = &sub
			continue
		}
		out.Packs = append(out.Packs, sub)
	}
	rows, err := s.db.Query(ctx, subscriptionSelect+`
		WHERE s.user_id = $1 AND s.status = 'active' AND s.starts_at > $2 AND p.tier <> ''
		ORDER BY s.starts_at, s.id`, userID, now)
	if err != nil {
		return nil, err
	}
	for rows.Next() {
		sub, err := scanSubscription(rows)
		if err != nil {
			rows.Close()
			return nil, err
		}
		out.Scheduled = append(out.Scheduled, sub)
	}
	rows.Close()
	if err := rows.Err(); err != nil {
		return nil, err
	}

	switch {
	case out.Subscription != nil:
		sub := out.Subscription
		out.Tier = TierRef{PlanID: sub.PlanID, Tier: sub.Tier, Name: sub.PlanName, Rank: st.tier.rank}
		out.Pools = map[string]PoolView{
			core.PoolAPI:   poolView(sub.QuotaMicros, sub.UsedMicros),
			core.PoolForge: poolView(sub.ForgeQuotaMicros, sub.ForgeUsedMicros),
		}
	default:
		out.Tier = TierRef{Tier: TierHobby, Name: "Hobby"}
		if err := s.db.QueryRow(ctx, `SELECT id, name, tier_rank FROM plans WHERE tier = $1`, TierHobby).
			Scan(&out.Tier.PlanID, &out.Tier.Name, &out.Tier.Rank); err != nil && !errors.Is(err, pgx.ErrNoRows) {
			return nil, err
		}
		h := st.hobby
		if h == nil {
			h = &bucket{}
		}
		out.Pools = map[string]PoolView{
			core.PoolAPI:   poolView(h.api, h.apiUsed),
			core.PoolForge: poolView(h.forge, h.forgeUsed),
		}
	}
	if out.OnDemand.UsedMicros, err = usage.OnDemandSpent(ctx, s.db, userID, st.cycleStart, st.cycleEnd); err != nil {
		return nil, err
	}
	pending, err := s.pendingSubscriptionOrder(ctx, userID)
	if err != nil {
		return nil, err
	}
	out.PendingOrder = pending
	return out, nil
}

func (s *Service) handleTiers(w http.ResponseWriter, r *http.Request) {
	ctx := r.Context()
	items, err := ListTiers(ctx, s.db)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	st, err := s.settings.Get(ctx)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	httpx.WriteJSON(w, http.StatusOK, map[string]any{"currency": st.Currency, "payment": s.paymentInfo(), "items": items})
}

func (s *Service) handleMembership(w http.ResponseWriter, r *http.Request) {
	c, err := claims(r)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	m, err := s.GetMembership(r.Context(), c.UserID)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	httpx.WriteJSON(w, http.StatusOK, m)
}

// SetOnDemand 更新按量付费开关与每周期上限（nil 表示不改），返回最新状态。
func (s *Service) SetOnDemand(ctx context.Context, userID int64, enabled *bool, limit *int64) (OnDemandView, error) {
	if limit != nil && (*limit < 0 || *limit > maxOnDemandLimit) {
		return OnDemandView{}, core.BadRequest("INVALID_REQUEST", "limitMicros 范围 0–1e12（0 表示不设上限）")
	}
	var out OnDemandView
	err := s.db.QueryRow(ctx,
		`UPDATE users SET on_demand_enabled = COALESCE($2, on_demand_enabled),
		        on_demand_limit_micros = COALESCE($3, on_demand_limit_micros), updated_at = now()
		 WHERE id = $1 RETURNING on_demand_enabled, on_demand_limit_micros`, userID, enabled, limit).
		Scan(&out.Enabled, &out.LimitMicros)
	if errors.Is(err, pgx.ErrNoRows) {
		return out, core.NotFound("USER_NOT_FOUND", "用户不存在")
	}
	if err != nil {
		return out, err
	}
	st, err := loadBillingState(ctx, s.db, userID, time.Now(), false)
	if err != nil {
		return out, err
	}
	out.UsedMicros, err = usage.OnDemandSpent(ctx, s.db, userID, st.cycleStart, st.cycleEnd)
	return out, err
}

func (s *Service) handleOnDemand(w http.ResponseWriter, r *http.Request) {
	c, err := claims(r)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	var req struct {
		Enabled     *bool  `json:"enabled"`
		LimitMicros *int64 `json:"limitMicros"`
	}
	if err := httpx.DecodeJSON(r, &req, 0); err != nil {
		httpx.WriteError(w, err)
		return
	}
	out, err := s.SetOnDemand(r.Context(), c.UserID, req.Enabled, req.LimitMicros)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	httpx.WriteJSON(w, http.StatusOK, out)
}

// handleMembershipUsage：按模型汇总 [from, to) 的用量；缺省为当前用量周期，跨度最多 400 天。
func (s *Service) handleMembershipUsage(w http.ResponseWriter, r *http.Request) {
	c, err := claims(r)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	ctx := r.Context()
	from, to := httpx.QueryTime(r, "from"), httpx.QueryTime(r, "to")
	if from.IsZero() || to.IsZero() {
		st, err := loadBillingState(ctx, s.db, c.UserID, time.Now(), false)
		if err != nil {
			httpx.WriteError(w, err)
			return
		}
		if from.IsZero() {
			from = st.cycleStart
		}
		if to.IsZero() {
			to = st.cycleEnd
		}
	}
	if !to.After(from) || to.Sub(from) > 400*24*time.Hour {
		httpx.WriteError(w, core.BadRequest("INVALID_REQUEST", "时间范围非法（to 须晚于 from，跨度不超过 400 天）"))
		return
	}
	items, total, err := usage.ByModel(ctx, s.db, c.UserID, from, to)
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
		"from": from.UTC(), "to": to.UTC(), "currency": st.Currency, "items": items, "totals": total,
	})
}
