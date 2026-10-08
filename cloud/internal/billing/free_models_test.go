package billing_test

import (
	"context"
	"net/http"
	"testing"

	"forge-cloud/internal/cloudtest"
	"forge-cloud/internal/core"
)

func TestFreeModelPrecheckWithoutFunds(t *testing.T) {
	a := cloudtest.New(t)
	s := a.NewUser()
	p := a.Principal(s.DeviceKey)
	ctx := context.Background()
	a.Exec(`UPDATE plans SET quota_micros = 0, forge_quota_micros = 0 WHERE tier = 'hobby'`)
	for _, pool := range []string{core.PoolAPI, core.PoolForge} {
		for _, onDemand := range []bool{false, true} {
			for _, balance := range []int64{0, -1} {
				a.Exec(`UPDATE users SET balance_micros = $2, on_demand_enabled = $3 WHERE id = $1`, s.UserID, balance, onDemand)
				m := &core.Model{ID: "free-test", Pool: pool}
				if err := a.Billing.Precheck(ctx, p, m); err != nil {
					t.Errorf("free model pool=%s onDemand=%t balance=%d: %v", pool, onDemand, balance, err)
				}
			}
		}
	}
}

func TestFreeModelPreservesUserAndKeyRestrictions(t *testing.T) {
	a := cloudtest.New(t)
	s := a.NewUser()
	p := a.Principal(s.DeviceKey)
	ctx := context.Background()
	m := &core.Model{ID: "free-test"}
	expect := func(p *core.Principal, status int, code string) {
		t.Helper()
		e := core.AsError(a.Billing.Precheck(ctx, p, m))
		if e == nil || e.Status != status || e.Code != code {
			t.Fatalf("got %v, want HTTP %d %s", e, status, code)
		}
	}
	expect(nil, http.StatusUnauthorized, core.CodeInvalidAPIKey)
	p.KeyQuota, p.KeyUsed = 100, 100
	expect(p, http.StatusPaymentRequired, core.CodeKeyQuotaExceeded)
	p.KeyQuota, p.KeyUsed = 0, 0
	a.Exec(`UPDATE users SET status = 'disabled' WHERE id = $1`, s.UserID)
	expect(p, http.StatusForbidden, core.CodeUserDisabled)
}

func TestAnyPricedComponentStillRequiresFunds(t *testing.T) {
	a := cloudtest.New(t)
	s := a.NewUser()
	p := a.Principal(s.DeviceKey)
	a.Exec(`UPDATE users SET balance_micros = 0 WHERE id = $1`, s.UserID)
	prices := []core.Pricing{
		{InputPer1M: 1}, {OutputPer1M: 1}, {CacheReadPer1M: 1}, {CacheWritePer1M: 1},
	}
	for _, price := range prices {
		m := &core.Model{ID: "paid-test", Pricing: price}
		e := core.AsError(a.Billing.Precheck(context.Background(), p, m))
		if e == nil || e.Code != core.CodeInsufficientBalance {
			t.Errorf("pricing %+v: got %v, want INSUFFICIENT_BALANCE", price, e)
		}
	}
	if e := core.AsError(a.Billing.Precheck(context.Background(), p, nil)); e == nil || e.Code != core.CodeInsufficientBalance {
		t.Fatalf("missing model must not be treated as free: %v", e)
	}
}

func TestFreeModelAtSpendLimitSettlesWithoutCharges(t *testing.T) {
	a := cloudtest.New(t)
	s := a.NewUser()
	ctx := context.Background()
	a.Exec(`UPDATE users SET balance_micros = 20, on_demand_limit_micros = 10 WHERE id = $1`, s.UserID)
	p := a.Principal(s.DeviceKey)
	if cost, err := a.Billing.Settle(ctx, usageRecord(p, 10, 0)); err != nil || cost != 10 {
		t.Fatalf("establish spend limit: cost=%d err=%v", cost, err)
	}
	p = a.Principal(s.DeviceKey)
	if e := precheckErr(t, a, p); e == nil || e.Code != core.CodeSpendLimitReached {
		t.Fatalf("paid model should be blocked at spend limit: %v", e)
	}
	rec := usageRecord(p, 100, 20)
	rec.Model = &core.Model{ID: "free-test"}
	rec.Usage.CacheReadTokens, rec.Usage.CacheWriteTokens = 10, 5
	if err := a.Billing.Precheck(ctx, p, rec.Model); err != nil {
		t.Fatalf("free model at spend limit: %v", err)
	}
	if cost, err := a.Billing.Settle(ctx, rec); err != nil || cost != 0 {
		t.Fatalf("free settlement: cost=%d err=%v", cost, err)
	}
	if balance := a.Int64(`SELECT balance_micros FROM users WHERE id = $1`, s.UserID); balance != 10 {
		t.Fatalf("free call changed balance: %d", balance)
	}
	if used := a.Int64(`SELECT used_micros FROM api_keys WHERE id = $1`, p.APIKeyID); used != 10 {
		t.Fatalf("free call changed key usage: %d", used)
	}
	if logs := a.Int64(`SELECT count(*) FROM usage_logs WHERE request_id = $1 AND status = 'ok'
		AND input_tokens = 100 AND output_tokens = 20 AND cache_read_tokens = 10 AND cache_write_tokens = 5
		AND cost_micros = 0 AND charged_balance_micros = 0 AND charged_plan_micros = 0`, rec.RequestID); logs != 1 {
		t.Fatal("free call must still record token usage with no charge")
	}
	if ledger := a.Int64(`SELECT count(*) FROM balance_ledger WHERE user_id = $1 AND kind = 'usage'`, s.UserID); ledger != 1 {
		t.Fatalf("free call added a balance ledger entry: %d", ledger)
	}
}
