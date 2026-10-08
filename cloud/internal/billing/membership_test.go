package billing_test

import (
	"context"
	"net/http"
	"testing"
	"time"

	"forge-cloud/internal/billing"
	"forge-cloud/internal/cloudtest"
	"forge-cloud/internal/core"
	"forge-cloud/internal/usage"
)

func forgeModel() *core.Model {
	return &core.Model{
		ID:      "forge-test",
		Pool:    core.PoolForge,
		Pricing: core.Pricing{InputPer1M: 1_000_000, OutputPer1M: 1_000_000},
	}
}

func tierPlanID(t *testing.T, a *cloudtest.App, tier string) int64 {
	t.Helper()
	return a.Int64(`SELECT id FROM plans WHERE tier = $1`, tier)
}

func getMembership(t *testing.T, a *cloudtest.App, token string) billing.Membership {
	t.Helper()
	var m billing.Membership
	a.Do(http.MethodGet, "/api/v1/me/membership", token, nil).OK(t).Decode(t, &m)
	return m
}

func patchOnDemand(a *cloudtest.App, token string, body map[string]any) *cloudtest.Response {
	return a.Do(http.MethodPatch, "/api/v1/me/membership/on-demand", token, body)
}

func TestTiersSeededInCursorOrder(t *testing.T) {
	a := cloudtest.New(t)
	var out struct {
		Currency string         `json:"currency"`
		Items    []billing.Tier `json:"items"`
	}
	a.Do(http.MethodGet, "/api/v1/tiers", "", nil).OK(t).Decode(t, &out)
	want := []struct {
		tier                     string
		month, year, api, forgeQ int64
	}{
		{"hobby", 0, 0, 0, 1_000_000},
		{"pro", 20_000_000, 192_000_000, 20_000_000, 60_000_000},
		{"pro_plus", 60_000_000, 576_000_000, 70_000_000, 180_000_000},
		{"ultra", 200_000_000, 1_920_000_000, 400_000_000, 1_200_000_000},
	}
	if len(out.Items) != len(want) {
		t.Fatalf("tiers = %+v", out.Items)
	}
	for i, w := range want {
		it := out.Items[i]
		if it.Tier != w.tier || it.PriceMonthlyMicros != w.month || it.PriceYearlyMicros != w.year ||
			it.IncludedAPIMicros != w.api || it.IncludedForgeMicros != w.forgeQ || len(it.Features) == 0 {
			t.Errorf("tier[%d] = %+v, want %+v", i, it, w)
		}
	}
	if !out.Items[2].Highlight || out.Currency == "" {
		t.Errorf("Pro+ should be highlighted and currency set: %+v", out)
	}
}

func TestHobbyFreeForgePoolAndOnDemand(t *testing.T) {
	a := cloudtest.New(t)
	ctx := context.Background()
	s := a.NewUser()
	p := a.Principal(s.DeviceKey)
	a.Exec(`UPDATE users SET balance_micros = 0 WHERE id = $1`, s.UserID)

	// Hobby：forge 池每月 1 额度，api 池没有。
	if err := a.Billing.Precheck(ctx, p, forgeModel()); err != nil {
		t.Fatalf("Hobby forge 额度应放行: %v", err)
	}
	if e := precheckErr(t, a, p); e == nil || e.Code != core.CodeInsufficientBalance {
		t.Fatalf("api 池无额度且无余额应 402 INSUFFICIENT_BALANCE: %v", e)
	}
	rec := usageRecord(p, 400_000, 0)
	rec.Model = forgeModel()
	if cost, err := a.Billing.Settle(ctx, rec); err != nil || cost != 400_000 {
		t.Fatalf("Settle forge: cost=%d err=%v", cost, err)
	}
	if got := a.Int64(`SELECT free_forge_used_micros FROM users WHERE id = $1`, s.UserID); got != 400_000 {
		t.Fatalf("free_forge_used_micros = %d", got)
	}
	if a.Int64(`SELECT balance_micros FROM users WHERE id = $1`, s.UserID) != 0 {
		t.Fatal("Hobby 额度内不应扣余额")
	}
	m := getMembership(t, a, s.AccessToken)
	if m.Tier.Tier != billing.TierHobby || m.Subscription != nil ||
		m.Pools[core.PoolForge].UsedMicros != 400_000 || m.Pools[core.PoolForge].RemainingMicros != 600_000 {
		t.Fatalf("membership = %+v", m)
	}

	// 关闭按量付费 → INCLUDED_USAGE_EXHAUSTED；开启并设上限 → 达上限后 SPEND_LIMIT_REACHED。
	a.Exec(`UPDATE users SET balance_micros = 5000000 WHERE id = $1`, s.UserID)
	patchOnDemand(a, s.AccessToken, map[string]any{"enabled": false}).OK(t)
	if e := precheckErr(t, a, p); e == nil || e.Status != 402 || e.Code != core.CodeIncludedUsageExhausted {
		t.Fatalf("关闭按量付费应 INCLUDED_USAGE_EXHAUSTED: %v", e)
	}
	patchOnDemand(a, s.AccessToken, map[string]any{"enabled": true, "limitMicros": 1_000_000}).OK(t)
	if err := a.Billing.Precheck(ctx, p, testModel()); err != nil {
		t.Fatalf("未达上限应放行: %v", err)
	}
	if _, err := a.Billing.Settle(ctx, usageRecord(p, 1_000_000, 0)); err != nil {
		t.Fatal(err)
	}
	if e := precheckErr(t, a, p); e == nil || e.Code != core.CodeSpendLimitReached {
		t.Fatalf("达到上限应 SPEND_LIMIT_REACHED: %v", e)
	}
	var od billing.OnDemandView
	patchOnDemand(a, s.AccessToken, map[string]any{"limitMicros": 0}).OK(t).Decode(t, &od)
	if !od.Enabled || od.LimitMicros != 0 || od.UsedMicros != 1_000_000 {
		t.Fatalf("on-demand = %+v", od)
	}
	if err := a.Billing.Precheck(ctx, p, testModel()); err != nil {
		t.Fatalf("取消上限后应放行: %v", err)
	}
	patchOnDemand(a, s.AccessToken, map[string]any{"limitMicros": -1}).Expect(t, 400, "INVALID_REQUEST")
}

func TestMonthlyQuotaResetsLazily(t *testing.T) {
	a := cloudtest.New(t)
	ctx := context.Background()
	s := a.NewUser()
	p := a.Principal(s.DeviceKey)
	a.Exec(`UPDATE users SET balance_micros = 0 WHERE id = $1`, s.UserID)
	pro := tierPlanID(t, a, "pro")
	// 40 天前开始的月付 Pro：上一个用量周期的 api 额度已用满。
	subID := a.Int64(`INSERT INTO subscriptions (user_id, plan_id, status, starts_at, ends_at, quota_micros,
		forge_quota_micros, used_micros, usage_cycle, cycle_start, source)
		VALUES ($1, $2, 'active', now() - interval '40 days', now() + interval '300 days', 20000000, 60000000,
		        20000000, 'month', now() - interval '40 days', 'grant') RETURNING id`, s.UserID, pro)

	if err := a.Billing.Precheck(ctx, p, testModel()); err != nil {
		t.Fatalf("进入新周期后额度应已重置: %v", err)
	}
	m := getMembership(t, a, s.AccessToken)
	if m.Tier.Tier != "pro" || m.Subscription == nil || m.Subscription.ID != subID || m.Pools[core.PoolAPI].UsedMicros != 0 {
		t.Fatalf("membership = %+v", m)
	}
	if _, err := a.Billing.Settle(ctx, usageRecord(p, 2_000_000, 0)); err != nil {
		t.Fatal(err)
	}
	if got := a.Int64(`SELECT used_micros FROM subscriptions WHERE id = $1`, subID); got != 2_000_000 {
		t.Fatalf("新周期 used_micros = %d", got)
	}
	var cycleStart time.Time
	if err := a.DB.QueryRow(ctx, `SELECT cycle_start FROM subscriptions WHERE id = $1`, subID).Scan(&cycleStart); err != nil {
		t.Fatal(err)
	}
	if age := time.Since(cycleStart); age < 5*24*time.Hour || age > 15*24*time.Hour {
		t.Fatalf("cycle_start 应写成当前周期开始（约 10 天前），实际 %s 前", age)
	}
	if a.Int64(`SELECT balance_micros FROM users WHERE id = $1`, s.UserID) != 0 {
		t.Fatal("额度内不应扣余额")
	}
}

func TestMembershipUsageByModel(t *testing.T) {
	a := cloudtest.New(t)
	ctx := context.Background()
	s := a.NewUser()
	p := a.Principal(s.DeviceKey)
	a.Exec(`UPDATE users SET balance_micros = 10000000 WHERE id = $1`, s.UserID)

	fr := usageRecord(p, 300_000, 0)
	fr.Model = forgeModel()
	if _, err := a.Billing.Settle(ctx, fr); err != nil { // Hobby forge 额度承担
		t.Fatal(err)
	}
	if _, err := a.Billing.Settle(ctx, usageRecord(p, 2_000_000, 0)); err != nil { // api 池无额度 → 按量付费
		t.Fatal(err)
	}
	var out struct {
		Items  []usage.ModelUsage `json:"items"`
		Totals usage.ModelUsage   `json:"totals"`
	}
	a.Do(http.MethodGet, "/api/v1/me/membership/usage", s.AccessToken, nil).OK(t).Decode(t, &out)
	if len(out.Items) != 2 {
		t.Fatalf("items = %+v", out.Items)
	}
	if it := out.Items[0]; it.Model != "gpt-test" || it.Pool != core.PoolAPI || it.OnDemandMicros != 2_000_000 || it.IncludedMicros != 0 {
		t.Fatalf("api row = %+v", it)
	}
	if it := out.Items[1]; it.Model != "forge-test" || it.Pool != core.PoolForge || it.IncludedMicros != 300_000 || it.OnDemandMicros != 0 {
		t.Fatalf("forge row = %+v", it)
	}
	if out.Totals.CostMicros != 2_300_000 || out.Totals.Requests != 2 {
		t.Fatalf("totals = %+v", out.Totals)
	}
	a.Do(http.MethodGet, "/api/v1/me/membership/usage?from=2026-01-02T00:00:00Z&to=2026-01-01T00:00:00Z",
		s.AccessToken, nil).Expect(t, 400, "INVALID_REQUEST")
}
