package billing

import (
	"context"
	"errors"
	"sort"
	"time"

	"github.com/jackc/pgx/v5"

	"forge-cloud/internal/core"
)

// bucket 是一份套餐内额度：一条生效订阅，或 Hobby 免费额度（subID = 0，记在 users.free_*）。
// 已用量已按当前用量周期修正（周期滚动时视为 0，写回时再落 cycle_start）。
type bucket struct {
	subID      int64
	tier       string
	rank       int
	end        time.Time // 先到期先扣
	cycleStart time.Time
	cycleEnd   time.Time
	api        int64
	apiUsed    int64
	forge      int64
	forgeUsed  int64
	dailyLimit int64
	dailyUsed  int64
	dirty      bool
}

func (b *bucket) quota(pool string) int64 {
	if pool == core.PoolForge {
		return b.forge
	}
	return b.api
}

func (b *bucket) used(pool string) int64 {
	if pool == core.PoolForge {
		return b.forgeUsed
	}
	return b.apiUsed
}

// avail 是该池本周期还能扣的额度（受每日上限约束）。
func (b *bucket) avail(pool string) int64 {
	a := b.quota(pool) - b.used(pool)
	if b.dailyLimit > 0 && b.dailyLimit-b.dailyUsed < a {
		a = b.dailyLimit - b.dailyUsed
	}
	return max(a, 0)
}

func (b *bucket) take(pool string, amount int64) {
	if pool == core.PoolForge {
		b.forgeUsed += amount
	} else {
		b.apiUsed += amount
	}
	b.dailyUsed += amount
	b.dirty = true
}

// billingState 是预检与结算共用的计费快照。
type billingState struct {
	status        string
	balance       int64
	onDemand      bool
	onDemandLimit int64
	today         time.Time
	buckets       []*bucket
	// tier 是有效档位订阅的额度桶；nil 表示 Hobby（此时 hobby 可能是 Hobby 免费额度桶）。
	tier  *bucket
	hobby *bucket
	// cycleStart/cycleEnd 是按量付费的统计周期：有效档位订阅的当前用量周期，Hobby 为 UTC 自然月。
	cycleStart time.Time
	cycleEnd   time.Time
}

var errBillingUserNotFound = errors.New("billing: user not found")

// loadBillingState 读取用户、生效订阅与 Hobby 额度；lock 时对用户行与订阅行加 FOR UPDATE（结算用）。
func loadBillingState(ctx context.Context, q Querier, userID int64, now time.Time, lock bool) (*billingState, error) {
	suffix := ""
	if lock {
		suffix = " FOR UPDATE"
	}
	st := &billingState{today: dateOnly(now)}
	var (
		freeStart          *time.Time
		freeAPI, freeForge int64
	)
	err := q.QueryRow(ctx,
		`SELECT status, balance_micros, on_demand_enabled, on_demand_limit_micros,
		        free_cycle_start, free_api_used_micros, free_forge_used_micros
		 FROM users WHERE id = $1`+suffix, userID).
		Scan(&st.status, &st.balance, &st.onDemand, &st.onDemandLimit, &freeStart, &freeAPI, &freeForge)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, errBillingUserNotFound
	}
	if err != nil {
		return nil, err
	}

	subSQL := `SELECT s.id, COALESCE(p.tier, ''), COALESCE(p.tier_rank, 0), s.starts_at, s.ends_at, s.usage_cycle,
		        s.cycle_start, s.quota_micros, s.used_micros, s.forge_quota_micros, s.forge_used_micros,
		        s.daily_limit_micros, s.daily_used_micros, s.daily_date
		 FROM subscriptions s LEFT JOIN plans p ON p.id = s.plan_id
		 WHERE s.user_id = $1 AND s.status = 'active' AND s.starts_at <= $2 AND s.ends_at > $2
		 ORDER BY s.ends_at, s.id`
	if lock {
		subSQL += ` FOR UPDATE OF s`
	}
	rows, err := q.Query(ctx, subSQL, userID, now)
	if err != nil {
		return nil, err
	}
	for rows.Next() {
		var (
			b                bucket
			startsAt, endsAt time.Time
			cycle            string
			storedCycle      *time.Time
			dailyDate        *time.Time
		)
		if err := rows.Scan(&b.subID, &b.tier, &b.rank, &startsAt, &endsAt, &cycle, &storedCycle,
			&b.api, &b.apiUsed, &b.forge, &b.forgeUsed, &b.dailyLimit, &b.dailyUsed, &dailyDate); err != nil {
			rows.Close()
			return nil, err
		}
		b.end = endsAt.UTC()
		b.cycleStart, b.cycleEnd = usageCycleOf(cycle, startsAt, endsAt, now)
		stored := startsAt
		if storedCycle != nil {
			stored = *storedCycle
		}
		if !stored.Equal(b.cycleStart) {
			b.apiUsed, b.forgeUsed = 0, 0
		}
		if dailyDate == nil || !sameDate(*dailyDate, st.today) {
			b.dailyUsed = 0
		}
		bb := b
		st.buckets = append(st.buckets, &bb)
		if bb.tier != "" && (st.tier == nil || bb.rank > st.tier.rank || (bb.rank == st.tier.rank && bb.end.After(st.tier.end))) {
			st.tier = &bb
		}
	}
	rows.Close()
	if err := rows.Err(); err != nil {
		return nil, err
	}

	if st.tier == nil {
		var api, forge int64
		err := q.QueryRow(ctx,
			`SELECT quota_micros, forge_quota_micros FROM plans WHERE tier = $1 AND enabled`, TierHobby).Scan(&api, &forge)
		if err != nil && !errors.Is(err, pgx.ErrNoRows) {
			return nil, err
		}
		cs, ce := calendarMonth(now)
		st.cycleStart, st.cycleEnd = cs, ce
		if err == nil && (api > 0 || forge > 0) {
			h := &bucket{tier: TierHobby, end: ce, cycleStart: cs, cycleEnd: ce, api: api, forge: forge}
			if freeStart != nil && sameDate(*freeStart, cs) {
				h.apiUsed, h.forgeUsed = freeAPI, freeForge
			}
			st.hobby = h
			st.buckets = append(st.buckets, h)
		}
	} else {
		st.cycleStart, st.cycleEnd = st.tier.cycleStart, st.tier.cycleEnd
	}
	sort.SliceStable(st.buckets, func(i, j int) bool {
		if !st.buckets[i].end.Equal(st.buckets[j].end) {
			return st.buckets[i].end.Before(st.buckets[j].end)
		}
		return st.buckets[i].subID < st.buckets[j].subID
	})
	return st, nil
}

// hasIncluded 报告该池本周期是否还有套餐内额度。
func (st *billingState) hasIncluded(pool string) bool {
	for _, b := range st.buckets {
		if b.avail(pool) > 0 {
			return true
		}
	}
	return false
}

// consume 按到期先后从额度桶扣 cost，返回（套餐内承担, 余额承担）。
// 额度不足的部分：开启按量付费 → 扣余额；关闭 → 记入最后一个有该池额度的桶（超额一次，下次预检拦截），
// 一个桶都没有时仍扣余额（并发越过预检的极端情况）。
func (st *billingState) consume(pool string, cost int64) (plan, balance int64) {
	remaining := cost
	var last *bucket
	for _, b := range st.buckets {
		if b.quota(pool) > 0 {
			last = b
		}
		if remaining <= 0 {
			continue
		}
		if a := b.avail(pool); a > 0 {
			take := min(a, remaining)
			b.take(pool, take)
			remaining -= take
			plan += take
		}
	}
	if remaining > 0 {
		if !st.onDemand && last != nil {
			last.take(pool, remaining)
			plan += remaining
		} else {
			balance = remaining
		}
	}
	return plan, balance
}

// save 把改过的额度桶写回（订阅行或用户的 Hobby 计数）。
func (st *billingState) save(ctx context.Context, q Querier, userID int64) error {
	for _, b := range st.buckets {
		if !b.dirty {
			continue
		}
		if b.subID == 0 {
			if _, err := q.Exec(ctx,
				`UPDATE users SET free_cycle_start = $2, free_api_used_micros = $3, free_forge_used_micros = $4 WHERE id = $1`,
				userID, b.cycleStart, b.apiUsed, b.forgeUsed); err != nil {
				return err
			}
			continue
		}
		if _, err := q.Exec(ctx,
			`UPDATE subscriptions SET used_micros = $2, forge_used_micros = $3, cycle_start = $4,
			        daily_used_micros = $5, daily_date = $6 WHERE id = $1`,
			b.subID, b.apiUsed, b.forgeUsed, b.cycleStart, b.dailyUsed, st.today); err != nil {
			return err
		}
	}
	return nil
}
