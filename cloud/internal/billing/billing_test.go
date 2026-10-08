package billing_test

import (
	"context"
	"net/http"
	"testing"
	"time"

	"forge-cloud/internal/billing"
	"forge-cloud/internal/cloudtest"
	"forge-cloud/internal/core"
)

func testModel() *core.Model {
	return &core.Model{
		ID: "gpt-test",
		Pricing: core.Pricing{
			InputPer1M:  1_000_000,
			OutputPer1M: 1_000_000,
		},
	}
}

func usageRecord(p *core.Principal, input, output int64) *core.UsageRecord {
	return &core.UsageRecord{
		RequestID: "req-" + core.RandomString(8),
		Principal: p,
		Model:     testModel(),
		Endpoint:  "chat",
		Usage:     core.Usage{InputTokens: input, OutputTokens: output},
		Status:    "ok",
	}
}

func precheckErr(t *testing.T, a *cloudtest.App, p *core.Principal) *core.Error {
	t.Helper()
	err := a.Billing.Precheck(context.Background(), p, testModel())
	return core.AsError(err)
}

func TestPrecheckBalanceSubscriptionKeyQuota(t *testing.T) {
	a := cloudtest.New(t)
	a.UpdateSettings(`{"signupBonusMicros": 1000000}`)
	ctx := context.Background()
	s := a.NewUser()
	p := a.Principal(s.DeviceKey)

	if err := a.Billing.Precheck(ctx, p, testModel()); err != nil {
		t.Fatalf("有余额应放行: %v", err)
	}

	a.Exec(`UPDATE users SET balance_micros = 0 WHERE id = $1`, s.UserID)
	if e := precheckErr(t, a, p); e == nil || e.Status != 402 || e.Code != core.CodeInsufficientBalance {
		t.Fatalf("零余额无订阅应 402: %v", e)
	}

	planID := a.CreatePlan("Basic", 30, 10_000_000, 0, 0)
	if _, err := billing.CreateSubscription(ctx, a.DB, s.UserID, planID, 0); err != nil {
		t.Fatal(err)
	}
	if err := a.Billing.Precheck(ctx, p, testModel()); err != nil {
		t.Fatalf("有套餐额度应放行: %v", err)
	}

	a.Exec(`UPDATE subscriptions SET used_micros = quota_micros WHERE user_id = $1`, s.UserID)
	if e := precheckErr(t, a, p); e == nil || e.Code != core.CodeInsufficientBalance {
		t.Fatalf("套餐额度用尽且无余额应 402: %v", e)
	}

	a.Exec(`UPDATE users SET balance_micros = 1 WHERE id = $1`, s.UserID)
	if err := a.Billing.Precheck(ctx, p, testModel()); err != nil {
		t.Fatalf("余额 > 0 应放行: %v", err)
	}

	var keyOut struct {
		Key string `json:"key"`
	}
	a.Do(http.MethodPost, "/api/v1/me/api-keys", s.AccessToken, map[string]any{"name": "q", "quotaMicros": 100}).OK(t).Decode(t, &keyOut)
	qp := a.Principal(keyOut.Key)
	a.Exec(`UPDATE api_keys SET used_micros = 100 WHERE user_id = $1 AND kind = 'user' AND name = 'q'`, s.UserID)
	qp = a.Principal(keyOut.Key)
	if e := precheckErr(t, a, qp); e == nil || e.Code != core.CodeKeyQuotaExceeded {
		t.Fatalf("Key 额度用尽应 KEY_QUOTA_EXCEEDED: %v", e)
	}
}

func TestPrecheckDailyLimit(t *testing.T) {
	a := cloudtest.New(t)
	ctx := context.Background()
	s := a.NewUser()
	p := a.Principal(s.DeviceKey)
	a.Exec(`UPDATE users SET balance_micros = 0 WHERE id = $1`, s.UserID)

	planID := a.CreatePlan("Daily", 30, 1_000_000, 50_000, 0)
	if _, err := billing.CreateSubscription(ctx, a.DB, s.UserID, planID, 0); err != nil {
		t.Fatal(err)
	}
	a.Exec(`UPDATE subscriptions SET daily_used_micros = 50000, daily_date = (now() AT TIME ZONE 'UTC')::date WHERE user_id = $1`, s.UserID)
	if e := precheckErr(t, a, p); e == nil || e.Code != core.CodeInsufficientBalance {
		t.Fatalf("每日上限用尽应视为无可用套餐: %v", e)
	}
}

func TestSettleSubscriptionThenBalanceAndError(t *testing.T) {
	a := cloudtest.New(t)
	ctx := context.Background()
	s := a.NewUser()
	p := a.Principal(s.DeviceKey)
	a.Exec(`UPDATE users SET balance_micros = 0 WHERE id = $1`, s.UserID)

	planID := a.CreatePlan("Settle", 30, 1_000_000, 0, 0)
	sub, err := billing.CreateSubscription(ctx, a.DB, s.UserID, planID, 0)
	if err != nil {
		t.Fatal(err)
	}

	// 1M input @ 1 micro/token × multiplier 1 → cost 1_000_000，应全扣订阅。
	rec := usageRecord(p, 1_000_000, 0)
	rec.Principal.APIKeyID = p.APIKeyID
	cost, err := a.Billing.Settle(ctx, rec)
	if err != nil || cost != 1_000_000 {
		t.Fatalf("Settle: cost=%d err=%v", cost, err)
	}
	used := a.Int64(`SELECT used_micros FROM subscriptions WHERE id = $1`, sub.ID)
	if used != 1_000_000 {
		t.Fatalf("订阅 used = %d", used)
	}
	if a.Int64(`SELECT balance_micros FROM users WHERE id = $1`, s.UserID) != 0 {
		t.Fatal("余额不应变动")
	}

	// 再扣 500k：订阅剩 0，余下扣余额（允许负数）。
	rec2 := usageRecord(p, 500_000, 0)
	rec2.RequestID = "req2"
	cost, err = a.Billing.Settle(ctx, rec2)
	if err != nil || cost != 500_000 {
		t.Fatalf("Settle2: %v", err)
	}
	if a.Int64(`SELECT used_micros FROM subscriptions WHERE id = $1`, sub.ID) != 1_000_000 {
		t.Fatal("订阅应已满额")
	}
	if bal := a.Int64(`SELECT balance_micros FROM users WHERE id = $1`, s.UserID); bal != -500_000 {
		t.Fatalf("余额应扣成负数: %d", bal)
	}
	if a.Int64(`SELECT count(*) FROM balance_ledger WHERE user_id = $1 AND kind = 'usage'`, s.UserID) != 1 {
		t.Fatal("应写 usage 流水")
	}

	errRec := usageRecord(p, 100, 0)
	errRec.Status = "error"
	errRec.RequestID = "req-err"
	cost, err = a.Billing.Settle(ctx, errRec)
	if err != nil || cost != 0 {
		t.Fatalf("error 请求费用应为 0: cost=%d err=%v", cost, err)
	}
	if a.Int64(`SELECT count(*) FROM usage_logs WHERE user_id = $1 AND request_id = 'req-err' AND status = 'error' AND cost_micros = 0`, s.UserID) != 1 {
		t.Fatal("应写 error usage_log")
	}
}

func TestRedeemOutcomes(t *testing.T) {
	a := cloudtest.New(t)
	s := a.NewUser()
	planID := a.CreatePlan("RedeemPlan", 30, 8_000_000, 0, 0)

	redeem := func(code string) *cloudtest.Response {
		return a.Do(http.MethodPost, "/api/v1/me/redeem", s.AccessToken, map[string]any{"code": code})
	}

	redeem("NO-SUCH").Expect(t, 404, "REDEEM_CODE_INVALID")

	balCode := "BAL-" + core.RandomString(6)
	a.CreateRedeemCode(balCode, "balance", 3_000_000, 0, 1, nil)
	m := redeem(balCode).OK(t).Map(t)
	if m["kind"] != "balance" || int64(m["valueMicros"].(float64)) != 3_000_000 {
		t.Fatalf("balance redeem = %v", m)
	}
	if int64(m["balanceMicros"].(float64)) < 3_000_000 {
		t.Fatal("balanceMicros 应增加")
	}
	redeem(balCode).Expect(t, 409, "REDEEM_CODE_USED")

	expired := time.Now().Add(-time.Hour)
	expCode := "EXP-" + core.RandomString(6)
	a.CreateRedeemCode(expCode, "balance", 1, 0, 1, &expired)
	redeem(expCode).Expect(t, 410, "REDEEM_CODE_EXPIRED")

	planCode := "PLAN-" + core.RandomString(6)
	a.CreateRedeemCode(planCode, "plan", 0, planID, 1, nil)
	pm := redeem(planCode).OK(t).Map(t)
	if pm["kind"] != "plan" || pm["subscription"] == nil {
		t.Fatalf("plan redeem = %v", pm)
	}

	invCode := "INV-" + core.RandomString(6)
	a.CreateRedeemCode(invCode, "invite", 0, 0, 1, nil)
	redeem(invCode).Expect(t, 400, "REDEEM_CODE_INVALID")

	a.Exec(`UPDATE redeem_codes SET status = 'revoked' WHERE code = $1`, invCode)
	redeem(invCode).Expect(t, 404, "REDEEM_CODE_INVALID")
}

func TestPaymentNotConfigured(t *testing.T) {
	a := cloudtest.New(t)
	s := a.NewUser()
	a.Do(http.MethodPost, "/api/v1/me/orders", s.AccessToken, map[string]any{
		"amountMicros": 1_000_000, "provider": "stripe",
	}).Expect(t, 501, "PAYMENT_NOT_CONFIGURED")
}
