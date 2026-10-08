package billing

import (
	"testing"
	"time"

	"forge-cloud/internal/core"
)

func day(y int, m time.Month, d int) time.Time { return time.Date(y, m, d, 0, 0, 0, 0, time.UTC) }

func testTier(name, key string, rank int, month, year int64) Tier {
	return Tier{PlanID: int64(rank + 1), Tier: key, Name: name, Rank: rank,
		PriceMonthlyMicros: month, PriceYearlyMicros: year, Enabled: true}
}

var (
	tierHobby   = testTier("Hobby", "hobby", 0, 0, 0)
	tierPro     = testTier("Pro", "pro", 10, 20_000_000, 192_000_000)
	tierProPlus = testTier("Pro+", "pro_plus", 20, 60_000_000, 576_000_000)
)

func TestAddMonthsClampsToMonthEnd(t *testing.T) {
	cases := []struct {
		in   time.Time
		n    int
		want time.Time
	}{
		{day(2026, 1, 31), 1, day(2026, 2, 28)},
		{day(2028, 1, 31), 1, day(2028, 2, 29)},
		{day(2026, 1, 31), 2, day(2026, 3, 31)},
		{day(2026, 11, 15), 3, day(2027, 2, 15)},
		{day(2026, 9, 27), 12, day(2027, 9, 27)},
	}
	for _, c := range cases {
		if got := addMonths(c.in, c.n); !got.Equal(c.want) {
			t.Errorf("addMonths(%s, %d) = %s, want %s", c.in.Format(time.DateOnly), c.n, got, c.want)
		}
	}
}

func TestMonthlyCycle(t *testing.T) {
	start := day(2026, 1, 31)
	end := addMonths(start, 12)
	s, e := monthlyCycle(start, end, day(2026, 3, 1))
	if !s.Equal(day(2026, 2, 28)) || !e.Equal(day(2026, 3, 31)) {
		t.Fatalf("cycle containing 3/1 = [%s, %s)", s, e)
	}
	s, e = monthlyCycle(start, end, day(2025, 12, 1))
	if !s.Equal(start) || !e.Equal(day(2026, 2, 28)) {
		t.Fatalf("not-yet-started subscription should report its first cycle, got [%s, %s)", s, e)
	}
	short := start.AddDate(0, 0, 45)
	s, e = monthlyCycle(start, short, day(2026, 3, 10))
	if !s.Equal(day(2026, 2, 28)) || !e.Equal(short) {
		t.Fatalf("last cycle must be clamped to the subscription end, got [%s, %s)", s, e)
	}
}

func TestProrate(t *testing.T) {
	start, end := day(2026, 9, 1), day(2026, 10, 1) // 30 天
	for _, c := range []struct {
		now  time.Time
		want int64
	}{
		{day(2026, 8, 1), 30_000_000},  // 未开始：全额
		{day(2026, 9, 1), 30_000_000},  // 刚开始：全额
		{day(2026, 9, 16), 15_000_000}, // 剩 15/30 天
		{day(2026, 10, 1), 0},          // 已结束
	} {
		if got := prorate(30_000_000, start, end, c.now); got != c.want {
			t.Errorf("prorate at %s = %d, want %d", c.now.Format(time.DateOnly), got, c.want)
		}
	}
	if prorate(0, start, end, day(2026, 9, 16)) != 0 {
		t.Error("zero value must prorate to 0")
	}
}

func errCode(err error) string {
	if e := core.AsError(err); e != nil {
		return e.Code
	}
	return ""
}

func TestPlanQuoteNewDefaultsToMonthly(t *testing.T) {
	now := day(2026, 9, 27)
	q, err := planQuote(tierPro, "", nil, now)
	if err != nil {
		t.Fatal(err)
	}
	if q.Mode != ModeNew || q.Interval != IntervalMonth || q.AmountMicros != 20_000_000 || q.CreditMicros != 0 {
		t.Fatalf("new quote = %+v", q)
	}
	if !q.StartsAt.Equal(now) || !q.EndsAt.Equal(day(2026, 10, 27)) || q.CurrentSubscriptionID != nil {
		t.Fatalf("new quote dates = %s → %s", q.StartsAt, q.EndsAt)
	}
	y, err := planQuote(tierPro, IntervalYear, nil, now)
	if err != nil || y.AmountMicros != 192_000_000 || !y.EndsAt.Equal(day(2027, 9, 27)) {
		t.Fatalf("yearly quote = %+v err=%v", y, err)
	}
}

func TestPlanQuoteRejects(t *testing.T) {
	now := day(2026, 9, 27)
	disabled := tierPro
	disabled.Enabled = false
	noYearly := tierPro
	noYearly.PriceYearlyMicros = 0
	for _, c := range []struct {
		name     string
		tier     Tier
		interval string
		code     string
	}{
		{"hobby", tierHobby, IntervalMonth, "PLAN_NOT_PURCHASABLE"},
		{"disabled", disabled, IntervalMonth, "PLAN_NOT_PURCHASABLE"},
		{"no yearly price", noYearly, IntervalYear, "PLAN_NOT_PURCHASABLE"},
		{"bad interval", tierPro, "week", "INVALID_INTERVAL"},
	} {
		if _, err := planQuote(c.tier, c.interval, nil, now); errCode(err) != c.code {
			t.Errorf("%s: err = %v, want %s", c.name, err, c.code)
		}
	}
}

func TestPlanQuoteUpgradeCreditsPurchasedOnly(t *testing.T) {
	now := day(2026, 9, 16)
	subs := []tierSub{
		// 已购、生效中、剩一半 → 抵扣 15
		{id: 1, rank: 10, startsAt: day(2026, 9, 1), endsAt: day(2026, 10, 1), value: 30_000_000, source: SourcePurchase},
		// 已购、已预约 → 全额抵扣 20
		{id: 2, rank: 10, startsAt: day(2026, 10, 1), endsAt: day(2026, 11, 1), value: 20_000_000, source: SourcePurchase},
		// 赠送：不作废、不折算
		{id: 3, rank: 10, startsAt: day(2026, 9, 10), endsAt: day(2026, 12, 10), source: SourceGrant},
	}
	q, err := planQuote(tierProPlus, IntervalMonth, subs, now)
	if err != nil {
		t.Fatal(err)
	}
	if q.Mode != ModeUpgrade || !q.StartsAt.Equal(now) {
		t.Fatalf("mode=%s starts=%s", q.Mode, q.StartsAt)
	}
	if len(q.ReplacesSubscriptionIDs) != 2 || q.ReplacesSubscriptionIDs[0] != 1 || q.ReplacesSubscriptionIDs[1] != 2 {
		t.Fatalf("replaces = %v", q.ReplacesSubscriptionIDs)
	}
	if q.CreditMicros != 35_000_000 || q.AmountMicros != 25_000_000 || q.RefundMicros != 0 {
		t.Fatalf("credit=%d amount=%d refund=%d", q.CreditMicros, q.AmountMicros, q.RefundMicros)
	}
	// 同级取晚到期：当前档位是赠送的那条。
	if q.CurrentSubscriptionID == nil || *q.CurrentSubscriptionID != 3 {
		t.Fatalf("current = %v", q.CurrentSubscriptionID)
	}
}

func TestPlanQuoteUpgradeRefundsExcessCredit(t *testing.T) {
	now := day(2026, 9, 1)
	subs := []tierSub{{id: 7, rank: 10, startsAt: now, endsAt: day(2027, 9, 1), value: 192_000_000, source: SourcePurchase}}
	q, err := planQuote(tierProPlus, IntervalMonth, subs, now)
	if err != nil {
		t.Fatal(err)
	}
	if q.CreditMicros != 192_000_000 || q.AmountMicros != 0 || q.RefundMicros != 132_000_000 {
		t.Fatalf("credit=%d amount=%d refund=%d", q.CreditMicros, q.AmountMicros, q.RefundMicros)
	}
}

func TestPlanQuoteRenewAndDowngradeChainAfterLastEnd(t *testing.T) {
	now := day(2026, 9, 16)
	subs := []tierSub{
		{id: 1, rank: 20, startsAt: day(2026, 9, 1), endsAt: day(2026, 10, 1), value: 60_000_000, source: SourcePurchase},
		{id: 2, rank: 20, startsAt: day(2026, 10, 1), endsAt: day(2026, 11, 1), value: 60_000_000, source: SourcePurchase},
		{id: 9, rank: 20, startsAt: day(2026, 8, 1), endsAt: day(2026, 9, 1), source: SourcePurchase}, // 已结束，忽略
	}
	r, err := planQuote(tierProPlus, IntervalMonth, subs, now)
	if err != nil {
		t.Fatal(err)
	}
	if r.Mode != ModeRenew || !r.StartsAt.Equal(day(2026, 11, 1)) || !r.EndsAt.Equal(day(2026, 12, 1)) || r.CreditMicros != 0 {
		t.Fatalf("renew = %+v", r)
	}
	d, err := planQuote(tierPro, IntervalYear, subs, now)
	if err != nil {
		t.Fatal(err)
	}
	if d.Mode != ModeDowngrade || !d.StartsAt.Equal(day(2026, 11, 1)) || !d.EndsAt.Equal(day(2027, 11, 1)) ||
		d.AmountMicros != 192_000_000 || len(d.ReplacesSubscriptionIDs) != 0 {
		t.Fatalf("downgrade = %+v", d)
	}
	if d.CurrentSubscriptionID == nil || *d.CurrentSubscriptionID != 1 {
		t.Fatalf("current = %v", d.CurrentSubscriptionID)
	}
}
