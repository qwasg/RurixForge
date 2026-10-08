package billing

import (
	"context"
	"errors"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"

	"forge-cloud/internal/core"
)

// Querier 是 *pgxpool.Pool 与 pgx.Tx 的公共子集。
type Querier interface {
	Exec(ctx context.Context, sql string, args ...any) (pgconn.CommandTag, error)
	Query(ctx context.Context, sql string, args ...any) (pgx.Rows, error)
	QueryRow(ctx context.Context, sql string, args ...any) pgx.Row
}

// Subscription 是订阅的对外形状。已过期但未被惰性标记的订阅显示为 expired；
// usedMicros / forgeUsedMicros 是当前用量周期（cycleStart–cycleEnd）的已用量，dailyUsedMicros 只统计当天（UTC）。
// quotaMicros = api 池、forgeQuotaMicros = forge 池的每周期额度（§11.1）。
type Subscription struct {
	ID               int64     `json:"id"`
	PlanID           int64     `json:"planId"`
	PlanName         string    `json:"planName"`
	Tier             string    `json:"tier"`
	Status           string    `json:"status"`
	StartsAt         time.Time `json:"startsAt"`
	EndsAt           time.Time `json:"endsAt"`
	QuotaMicros      int64     `json:"quotaMicros"`
	UsedMicros       int64     `json:"usedMicros"`
	ForgeQuotaMicros int64     `json:"forgeQuotaMicros"`
	ForgeUsedMicros  int64     `json:"forgeUsedMicros"`
	DailyLimitMicros int64     `json:"dailyLimitMicros"`
	DailyUsedMicros  int64     `json:"dailyUsedMicros"`
	UsageCycle       string    `json:"usageCycle"`
	CycleStart       time.Time `json:"cycleStart"`
	CycleEnd         time.Time `json:"cycleEnd"`
	BillingInterval  string    `json:"billingInterval"`
	ValueMicros      int64     `json:"valueMicros"`
	Source           string    `json:"source"`
	GroupID          *int64    `json:"groupId"`
}

const subscriptionSelect = `SELECT s.id, s.plan_id, COALESCE(p.name, ''), COALESCE(p.tier, ''),
	CASE WHEN s.status = 'active' AND s.ends_at <= now() THEN 'expired' ELSE s.status END,
	s.starts_at, s.ends_at, s.quota_micros, s.used_micros, s.forge_quota_micros, s.forge_used_micros,
	s.daily_limit_micros,
	CASE WHEN s.daily_date = (now() AT TIME ZONE 'UTC')::date THEN s.daily_used_micros ELSE 0 END,
	s.usage_cycle, s.cycle_start, s.billing_interval, s.value_micros, s.source, s.group_id
	FROM subscriptions s LEFT JOIN plans p ON p.id = s.plan_id`

func scanSubscription(row pgx.Row) (Subscription, error) {
	var s Subscription
	var storedCycle *time.Time
	err := row.Scan(&s.ID, &s.PlanID, &s.PlanName, &s.Tier, &s.Status, &s.StartsAt, &s.EndsAt,
		&s.QuotaMicros, &s.UsedMicros, &s.ForgeQuotaMicros, &s.ForgeUsedMicros,
		&s.DailyLimitMicros, &s.DailyUsedMicros, &s.UsageCycle, &storedCycle, &s.BillingInterval, &s.ValueMicros,
		&s.Source, &s.GroupID)
	if err != nil {
		return s, err
	}
	s.StartsAt = s.StartsAt.UTC()
	s.EndsAt = s.EndsAt.UTC()
	s.CycleStart, s.CycleEnd = usageCycleOf(s.UsageCycle, s.StartsAt, s.EndsAt, time.Now())
	stored := s.StartsAt
	if storedCycle != nil {
		stored = *storedCycle
	}
	if !stored.Equal(s.CycleStart) {
		s.UsedMicros, s.ForgeUsedMicros = 0, 0
	}
	return s, nil
}

// ListSubscriptions 返回用户订阅：activeOnly 时只含生效中的（按到期先后），否则全部（生效中在前，最多 100 条）。
func ListSubscriptions(ctx context.Context, q Querier, userID int64, activeOnly bool) ([]Subscription, error) {
	sql := subscriptionSelect + ` WHERE s.user_id = $1`
	if activeOnly {
		sql += ` AND s.status = 'active' AND s.starts_at <= now() AND s.ends_at > now() ORDER BY s.ends_at, s.id`
	} else {
		sql += ` ORDER BY (s.status = 'active' AND s.ends_at > now()) DESC, s.ends_at DESC, s.id DESC LIMIT 100`
	}
	rows, err := q.Query(ctx, sql, userID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := []Subscription{}
	for rows.Next() {
		sub, err := scanSubscription(rows)
		if err != nil {
			return nil, err
		}
		out = append(out, sub)
	}
	return out, rows.Err()
}

// GetSubscription 按 ID 读取订阅。
func GetSubscription(ctx context.Context, q Querier, id int64) (Subscription, error) {
	sub, err := scanSubscription(q.QueryRow(ctx, subscriptionSelect+` WHERE s.id = $1`, id))
	if errors.Is(err, pgx.ErrNoRows) {
		return sub, core.NotFound("SUBSCRIPTION_NOT_FOUND", "订阅不存在")
	}
	return sub, err
}

// CreateSubscription 按套餐为用户新建订阅（来源 grant）：从现在起 days 天（days<=0 用套餐周期），
// 复制两池额度、每日上限与分组；档位套餐的额度按月重置（§11.1）。
func CreateSubscription(ctx context.Context, q Querier, userID, planID int64, days int) (Subscription, error) {
	return CreateSubscriptionFrom(ctx, q, userID, planID, days, SourceGrant)
}

// CreateSubscriptionFrom 同 CreateSubscription，指定来源（grant|redeem）。
func CreateSubscriptionFrom(ctx context.Context, q Querier, userID, planID int64, days int, source string) (Subscription, error) {
	var id int64
	err := q.QueryRow(ctx,
		`INSERT INTO subscriptions (user_id, plan_id, status, starts_at, ends_at, quota_micros, forge_quota_micros,
		                            daily_limit_micros, group_id, usage_cycle, source)
		 SELECT $1, p.id, 'active', now(),
		        now() + make_interval(days => CASE WHEN $3::int > 0 THEN $3::int ELSE p.period_days END),
		        p.quota_micros, p.forge_quota_micros, p.daily_limit_micros, p.group_id,
		        CASE WHEN p.tier <> '' THEN 'month' ELSE 'period' END, $4
		 FROM plans p WHERE p.id = $2
		 RETURNING id`, userID, planID, days, source).Scan(&id)
	if errors.Is(err, pgx.ErrNoRows) {
		return Subscription{}, core.NotFound("PLAN_NOT_FOUND", "套餐不存在")
	}
	if err != nil {
		return Subscription{}, err
	}
	return GetSubscription(ctx, q, id)
}

// LedgerEntry 是余额流水的对外形状。
type LedgerEntry struct {
	ID                 int64     `json:"id"`
	DeltaMicros        int64     `json:"deltaMicros"`
	BalanceAfterMicros int64     `json:"balanceAfterMicros"`
	Kind               string    `json:"kind"`
	Note               string    `json:"note"`
	CreatedAt          time.Time `json:"createdAt"`
}

// ListLedger 返回用户余额流水（新的在前）与总数。
func ListLedger(ctx context.Context, q Querier, userID int64, limit, offset int) ([]LedgerEntry, int64, error) {
	var total int64
	if err := q.QueryRow(ctx, `SELECT count(*) FROM balance_ledger WHERE user_id = $1`, userID).Scan(&total); err != nil {
		return nil, 0, err
	}
	rows, err := q.Query(ctx,
		`SELECT id, delta_micros, balance_after, kind, note, created_at FROM balance_ledger
		 WHERE user_id = $1 ORDER BY id DESC LIMIT $2 OFFSET $3`, userID, limit, offset)
	if err != nil {
		return nil, 0, err
	}
	defer rows.Close()
	out := []LedgerEntry{}
	for rows.Next() {
		var e LedgerEntry
		if err := rows.Scan(&e.ID, &e.DeltaMicros, &e.BalanceAfterMicros, &e.Kind, &e.Note, &e.CreatedAt); err != nil {
			return nil, 0, err
		}
		e.CreatedAt = e.CreatedAt.UTC()
		out = append(out, e)
	}
	return out, total, rows.Err()
}

// AdjustBalance 在调用方事务里改余额并写一条流水，返回改后余额。
// kind：redeem|admin_adjust|usage|signup_bonus|payment|refund。
func AdjustBalance(ctx context.Context, q Querier, userID, delta int64, kind, ref, note string) (int64, error) {
	var after int64
	err := q.QueryRow(ctx,
		`UPDATE users SET balance_micros = balance_micros + $2, updated_at = now() WHERE id = $1 RETURNING balance_micros`,
		userID, delta).Scan(&after)
	if errors.Is(err, pgx.ErrNoRows) {
		return 0, core.NotFound("USER_NOT_FOUND", "用户不存在")
	}
	if err != nil {
		return 0, err
	}
	_, err = q.Exec(ctx,
		`INSERT INTO balance_ledger (user_id, delta_micros, balance_after, kind, ref, note) VALUES ($1, $2, $3, $4, $5, $6)`,
		userID, delta, after, kind, ref, note)
	return after, err
}

// PublicPlan 是公开套餐列表的条目（tier 非空的是会员档位，见 §11）。
type PublicPlan struct {
	ID                int64    `json:"id"`
	Name              string   `json:"name"`
	Description       string   `json:"description"`
	PriceMicros       int64    `json:"priceMicros"`
	PeriodDays        int      `json:"periodDays"`
	QuotaMicros       int64    `json:"quotaMicros"`
	DailyLimitMicros  int64    `json:"dailyLimitMicros"`
	Tier              string   `json:"tier"`
	TierRank          int      `json:"tierRank"`
	PriceYearlyMicros int64    `json:"priceYearlyMicros"`
	ForgeQuotaMicros  int64    `json:"forgeQuotaMicros"`
	Tagline           string   `json:"tagline"`
	Features          []string `json:"features"`
	Highlight         bool     `json:"highlight"`
}

// ListPublicPlans 返回启用中的套餐（按价格升序）。
func ListPublicPlans(ctx context.Context, q Querier) ([]PublicPlan, error) {
	rows, err := q.Query(ctx,
		`SELECT id, name, description, price_micros, period_days, quota_micros, daily_limit_micros,
		        tier, tier_rank, price_yearly_micros, forge_quota_micros, tagline, features, highlight
		 FROM plans WHERE enabled ORDER BY price_micros, tier_rank, id`)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := []PublicPlan{}
	for rows.Next() {
		var p PublicPlan
		if err := rows.Scan(&p.ID, &p.Name, &p.Description, &p.PriceMicros, &p.PeriodDays, &p.QuotaMicros, &p.DailyLimitMicros,
			&p.Tier, &p.TierRank, &p.PriceYearlyMicros, &p.ForgeQuotaMicros, &p.Tagline, &p.Features, &p.Highlight); err != nil {
			return nil, err
		}
		if p.Features == nil {
			p.Features = []string{}
		}
		out = append(out, p)
	}
	return out, rows.Err()
}
