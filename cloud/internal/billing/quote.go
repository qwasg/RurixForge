package billing

import (
	"context"
	"errors"
	"math/big"
	"net/http"
	"strings"
	"time"

	"github.com/jackc/pgx/v5"

	"forge-cloud/internal/core"
)

// tierSub 是报价用的档位订阅（生效中或已预约，任意来源）。
type tierSub struct {
	id       int64
	rank     int
	startsAt time.Time
	endsAt   time.Time
	value    int64
	source   string
}

// Quote 是一次购买的报价（§11.3）。amountMicros = max(标价 − 抵扣, 0)；抵扣超出标价的部分 refundMicros 生效时退回余额。
type Quote struct {
	PlanID                  int64       `json:"planId"`
	Tier                    string      `json:"tier"`
	PlanName                string      `json:"planName"`
	Interval                string      `json:"interval"`
	Mode                    string      `json:"mode"`
	ListPriceMicros         int64       `json:"listPriceMicros"`
	CreditMicros            int64       `json:"creditMicros"`
	AmountMicros            int64       `json:"amountMicros"`
	RefundMicros            int64       `json:"refundMicros"`
	StartsAt                time.Time   `json:"startsAt"`
	EndsAt                  time.Time   `json:"endsAt"`
	CurrentSubscriptionID   *int64      `json:"currentSubscriptionId"`
	ReplacesSubscriptionIDs []int64     `json:"replacesSubscriptionIds"`
	BalanceMicros           int64       `json:"balanceMicros"`
	Currency                string      `json:"currency"`
	Payment                 PaymentView `json:"payment"`
}

// prorate 返回 value 在 [start, end) 里 now 之后剩余部分的折算值：未开始 = 全额，已结束 = 0（秒级，向下取整）。
func prorate(value int64, start, end, now time.Time) int64 {
	if value <= 0 || !end.After(now) {
		return 0
	}
	if !now.After(start) {
		return value
	}
	total := int64(end.Sub(start) / time.Second)
	if total <= 0 {
		return 0
	}
	rem := int64(end.Sub(now) / time.Second)
	v := new(big.Int).Mul(big.NewInt(value), big.NewInt(rem))
	return v.Quo(v, big.NewInt(total)).Int64()
}

// planQuote 是报价的纯计算（不碰数据库）：
//   - 没有生效中的档位订阅 → new，立即开始；
//   - 目标档位更高 → upgrade，立即开始；作废全部「已购」档位订阅（生效中的按剩余时间折算、已预约的全额）抵扣标价，
//     赠送/兑换来的档位不作废也不折算，照常用到期；
//   - 同档（含换付费周期）→ renew，更低 → downgrade：都接在当前档位订阅链末尾开始（降级下个周期生效，同 Cursor）。
func planQuote(t Tier, interval string, subs []tierSub, now time.Time) (*Quote, error) {
	interval = strings.TrimSpace(interval)
	if interval == "" {
		interval = IntervalMonth
	}
	if interval != IntervalMonth && interval != IntervalYear {
		return nil, core.BadRequest("INVALID_INTERVAL", "interval 只能是 month 或 year")
	}
	if !t.Enabled || t.Tier == TierHobby {
		return nil, core.BadRequest("PLAN_NOT_PURCHASABLE", "该档位无需购买或已下架")
	}
	price := t.PriceMonthlyMicros
	if interval == IntervalYear {
		price = t.PriceYearlyMicros
	}
	if price <= 0 {
		msg := "该档位不支持按月付费"
		if interval == IntervalYear {
			msg = "该档位不支持按年付费"
		}
		return nil, core.BadRequest("PLAN_NOT_PURCHASABLE", msg)
	}

	var current *tierSub
	chainEnd := now
	for i := range subs {
		ts := &subs[i]
		if !ts.endsAt.After(now) {
			continue
		}
		if ts.endsAt.After(chainEnd) {
			chainEnd = ts.endsAt
		}
		if !ts.startsAt.After(now) && (current == nil || ts.rank > current.rank ||
			(ts.rank == current.rank && ts.endsAt.After(current.endsAt))) {
			current = ts
		}
	}

	q := &Quote{PlanID: t.PlanID, Tier: t.Tier, PlanName: t.Name, Interval: interval, ListPriceMicros: price,
		ReplacesSubscriptionIDs: []int64{}}
	switch {
	case current == nil:
		q.Mode, q.StartsAt = ModeNew, now
	case t.Rank > current.rank:
		q.Mode, q.StartsAt = ModeUpgrade, now
		for _, ts := range subs {
			if ts.source != SourcePurchase || !ts.endsAt.After(now) {
				continue
			}
			q.ReplacesSubscriptionIDs = append(q.ReplacesSubscriptionIDs, ts.id)
			q.CreditMicros += prorate(ts.value, ts.startsAt, ts.endsAt, now)
		}
	case t.Rank == current.rank:
		q.Mode, q.StartsAt = ModeRenew, chainEnd
	default:
		q.Mode, q.StartsAt = ModeDowngrade, chainEnd
	}
	if current != nil {
		id := current.id
		q.CurrentSubscriptionID = &id
	}
	q.StartsAt = q.StartsAt.UTC()
	q.EndsAt = addMonths(q.StartsAt, termMonths(interval))
	q.AmountMicros = max(price-q.CreditMicros, 0)
	q.RefundMicros = max(q.CreditMicros-price, 0)
	return q, nil
}

// loadTierSubs 读取用户未结束的档位订阅（生效中与已预约），按开始时间排序；lock 时对订阅行加锁。
func loadTierSubs(ctx context.Context, q Querier, userID int64, now time.Time, lock bool) ([]tierSub, error) {
	sql := `SELECT s.id, p.tier_rank, s.starts_at, s.ends_at, s.value_micros, s.source
		FROM subscriptions s JOIN plans p ON p.id = s.plan_id
		WHERE s.user_id = $1 AND s.status = 'active' AND s.ends_at > $2 AND p.tier <> ''
		ORDER BY s.starts_at, s.id`
	if lock {
		sql += ` FOR UPDATE OF s`
	}
	rows, err := q.Query(ctx, sql, userID, now)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	var out []tierSub
	for rows.Next() {
		var t tierSub
		if err := rows.Scan(&t.id, &t.rank, &t.startsAt, &t.endsAt, &t.value, &t.source); err != nil {
			return nil, err
		}
		t.startsAt, t.endsAt = t.startsAt.UTC(), t.endsAt.UTC()
		out = append(out, t)
	}
	return out, rows.Err()
}

// buildQuote 读取余额、档位与档位订阅后报价；lock 时先锁用户行再锁订阅行（下单用，与结算同序）。
func (s *Service) buildQuote(ctx context.Context, q Querier, userID, planID int64, interval string, now time.Time, lock bool) (*Quote, error) {
	suffix := ""
	if lock {
		suffix = " FOR UPDATE"
	}
	var balance int64
	err := q.QueryRow(ctx, `SELECT balance_micros FROM users WHERE id = $1`+suffix, userID).Scan(&balance)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, core.NotFound("USER_NOT_FOUND", "用户不存在")
	}
	if err != nil {
		return nil, err
	}
	t, err := loadTier(ctx, q, planID)
	if err != nil {
		return nil, err
	}
	subs, err := loadTierSubs(ctx, q, userID, now, lock)
	if err != nil {
		return nil, err
	}
	quote, err := planQuote(t, interval, subs, now)
	if err != nil {
		return nil, err
	}
	settings, err := s.settings.Get(ctx)
	if err != nil {
		return nil, err
	}
	quote.BalanceMicros, quote.Currency, quote.Payment = balance, settings.Currency, s.paymentInfo()
	return quote, nil
}

// Quote 返回购买某档位的报价（只读，不下单）。
func (s *Service) Quote(ctx context.Context, userID, planID int64, interval string) (*Quote, error) {
	return s.buildQuote(ctx, s.db, userID, planID, interval, time.Now().UTC().Truncate(time.Second), false)
}

func insufficientBalance(balance, amount int64) error {
	e := core.E(http.StatusPaymentRequired, core.CodeInsufficientBalance, "余额不足，请先充值或兑换")
	e.Extra = map[string]any{"balanceMicros": balance, "amountMicros": amount}
	return e
}
