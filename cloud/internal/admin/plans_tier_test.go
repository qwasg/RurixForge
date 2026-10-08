package admin_test

import (
	"fmt"
	"net/http"
	"strings"
	"testing"

	"forge-cloud/internal/admin"
	"forge-cloud/internal/billing"
	"forge-cloud/internal/cloudtest"
)

func listPlans(t *testing.T, a *cloudtest.App, token string) map[string]admin.Plan {
	t.Helper()
	var out struct {
		Items []admin.Plan `json:"items"`
	}
	a.Do(http.MethodGet, "/api/admin/plans", token, nil).OK(t).Decode(t, &out)
	byName := map[string]admin.Plan{}
	for _, p := range out.Items {
		byName[p.Name] = p
	}
	return byName
}

func publicTiers(t *testing.T, a *cloudtest.App) []billing.Tier {
	t.Helper()
	var out struct {
		Items []billing.Tier `json:"items"`
	}
	a.Do(http.MethodGet, "/api/v1/tiers", "", nil).OK(t).Decode(t, &out)
	return out.Items
}

func TestPlanTierFields(t *testing.T) {
	a := cloudtest.New(t)
	adm := a.NewAdmin()
	tok := adm.AccessToken

	plans := listPlans(t, a, tok)
	pro, ok := plans["Pro"]
	if !ok || pro.Tier != "pro" || pro.TierRank != 10 || pro.PriceYearlyMicros != 192_000_000 ||
		pro.ForgeQuotaMicros != 60_000_000 || len(pro.Features) == 0 || pro.SubscriberCount != 0 {
		t.Fatalf("seeded Pro = %+v", pro)
	}

	a.Do(http.MethodPost, "/api/admin/plans", tok, map[string]any{"name": "Bad", "tier": "9team"}).
		Expect(t, 400, "INVALID_REQUEST")
	tooMany := make([]string, 13)
	for i := range tooMany {
		tooMany[i] = fmt.Sprintf("权益 %d", i)
	}
	a.Do(http.MethodPost, "/api/admin/plans", tok, map[string]any{"name": "Many", "features": tooMany}).
		Expect(t, 400, "INVALID_REQUEST")
	a.Do(http.MethodPost, "/api/admin/plans", tok, map[string]any{"name": "Long", "tagline": strings.Repeat("长", 65)}).
		Expect(t, 400, "INVALID_REQUEST")

	var team admin.Plan
	a.Do(http.MethodPost, "/api/admin/plans", tok, map[string]any{
		"name": "Team", "tier": " Team ", "tierRank": 40, "tagline": "团队协作",
		"priceMicros": 40_000_000, "priceYearlyMicros": 384_000_000,
		"quotaMicros": 50_000_000, "forgeQuotaMicros": 150_000_000,
		"features": []string{" 统一账单 ", "", "SSO"}, "highlight": true,
	}).OK(t).Decode(t, &team)
	if team.Tier != "team" || team.TierRank != 40 || team.Tagline != "团队协作" || !team.Highlight ||
		len(team.Features) != 2 || team.Features[0] != "统一账单" || team.ForgeQuotaMicros != 150_000_000 {
		t.Fatalf("created tier = %+v", team)
	}
	a.Do(http.MethodPatch, fmt.Sprintf("/api/admin/plans/%d", team.ID), tok, map[string]any{"tier": "pro"}).
		Expect(t, 409, "TIER_TAKEN")
	a.Do(http.MethodPost, "/api/admin/plans", tok, map[string]any{"name": "Dup", "tier": "ultra"}).
		Expect(t, 409, "TIER_TAKEN")
	a.Do(http.MethodPatch, fmt.Sprintf("/api/admin/plans/%d", team.ID), tok, map[string]any{"tierRank": 1001}).
		Expect(t, 400, "INVALID_REQUEST")

	tiers := publicTiers(t, a)
	if last := tiers[len(tiers)-1]; last.Tier != "team" || last.PriceYearlyMicros != 384_000_000 || last.IncludedForgeMicros != 150_000_000 {
		t.Fatalf("public tiers should end with team: %+v", tiers)
	}

	u := a.NewUser()
	a.Do(http.MethodPost, fmt.Sprintf("/api/admin/users/%d/subscriptions", u.UserID), tok,
		map[string]any{"planId": team.ID}).OK(t)
	if got := listPlans(t, a, tok)["Team"]; got.SubscriberCount != 1 {
		t.Fatalf("subscriberCount = %d", got.SubscriberCount)
	}
	if sub := a.String(`SELECT usage_cycle FROM subscriptions WHERE user_id = $1`, u.UserID); sub != "month" {
		t.Fatalf("档位订阅应按月重置，usage_cycle=%s", sub)
	}

	// 清空 tier → 变回普通额度包，不再出现在档位列表。
	var pack admin.Plan
	a.Do(http.MethodPatch, fmt.Sprintf("/api/admin/plans/%d", team.ID), tok, map[string]any{"tier": ""}).OK(t).Decode(t, &pack)
	if pack.Tier != "" {
		t.Fatalf("tier should be cleared: %+v", pack)
	}
	for _, it := range publicTiers(t, a) {
		if it.PlanID == team.ID {
			t.Fatal("清空 tier 后不应出现在 /tiers")
		}
	}
}
