package billing_test

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"strings"
	"testing"

	"forge-cloud/internal/billing"
	"forge-cloud/internal/cloudtest"
	"forge-cloud/internal/core"
)

// fakePay 是测试用在线支付渠道：支付地址按订单号生成，回调体 {"orderId","paid"}。
type fakePay struct{ name string }

func (f fakePay) Name() string { return f.name }

func (f fakePay) CreateOrder(_ context.Context, orderID, _ int64, _ int64) (string, error) {
	return fmt.Sprintf("https://pay.example.test/o/%d", orderID), nil
}

func (f fakePay) HandleNotify(r *http.Request) (int64, bool, error) {
	var body struct {
		OrderID int64 `json:"orderId"`
		Paid    bool  `json:"paid"`
	}
	err := json.NewDecoder(r.Body).Decode(&body)
	return body.OrderID, body.Paid, err
}

type checkoutResp struct {
	Order      billing.Order      `json:"order"`
	Membership billing.Membership `json:"membership"`
}

func checkout(a *cloudtest.App, token string, planID int64, interval, provider string) *cloudtest.Response {
	return a.Do(http.MethodPost, "/api/v1/me/membership/checkout", token,
		map[string]any{"planId": planID, "interval": interval, "provider": provider})
}

func quote(t *testing.T, a *cloudtest.App, token string, planID int64, interval string) billing.Quote {
	t.Helper()
	var q billing.Quote
	a.Do(http.MethodPost, "/api/v1/me/membership/quote", token,
		map[string]any{"planId": planID, "interval": interval}).OK(t).Decode(t, &q)
	return q
}

func TestCheckoutWithBalanceUpgradeDowngradeAndCancel(t *testing.T) {
	a := cloudtest.New(t)
	s := a.NewUser()
	tok := s.AccessToken
	a.Exec(`UPDATE users SET balance_micros = 300000000 WHERE id = $1`, s.UserID)
	pro, proPlus := tierPlanID(t, a, "pro"), tierPlanID(t, a, "pro_plus")

	if q := quote(t, a, tok, pro, "month"); q.Mode != billing.ModeNew || q.AmountMicros != 20_000_000 || q.BalanceMicros != 300_000_000 {
		t.Fatalf("new quote = %+v", q)
	}
	var r1 checkoutResp
	checkout(a, tok, pro, "month", "balance").OK(t).Decode(t, &r1)
	if r1.Order.Status != billing.OrderPaid || r1.Order.Mode != billing.ModeNew || r1.Order.SubscriptionID == nil {
		t.Fatalf("order = %+v", r1.Order)
	}
	if m := r1.Membership; m.Tier.Tier != "pro" || m.BalanceMicros != 280_000_000 ||
		m.Pools[core.PoolAPI].IncludedMicros != 20_000_000 || m.Pools[core.PoolForge].IncludedMicros != 60_000_000 ||
		m.Subscription == nil || m.Subscription.Source != billing.SourcePurchase || m.Subscription.BillingInterval != "month" {
		t.Fatalf("membership after new = %+v", m)
	}
	proSub := *r1.Order.SubscriptionID

	// 升级：立即生效，抵扣刚买的 Pro 的剩余价值（几乎全额）。
	up := quote(t, a, tok, proPlus, "month")
	if up.Mode != billing.ModeUpgrade || up.CreditMicros < 19_900_000 || up.CreditMicros > 20_000_000 ||
		up.AmountMicros != 60_000_000-up.CreditMicros || len(up.ReplacesSubscriptionIDs) != 1 {
		t.Fatalf("upgrade quote = %+v", up)
	}
	var r2 checkoutResp
	checkout(a, tok, proPlus, "month", "balance").OK(t).Decode(t, &r2)
	if r2.Membership.Tier.Tier != "pro_plus" || r2.Membership.BalanceMicros != 280_000_000-r2.Order.AmountMicros {
		t.Fatalf("membership after upgrade = %+v (order %+v)", r2.Membership, r2.Order)
	}
	if st := a.String(`SELECT status FROM subscriptions WHERE id = $1`, proSub); st != "cancelled" {
		t.Fatalf("被替换的 Pro 应作废，status=%s", st)
	}
	proPlusEnd := r2.Membership.Subscription.EndsAt

	// 降级：接在 Pro+ 到期之后，当前档位不变。
	var r3 checkoutResp
	checkout(a, tok, pro, "month", "balance").OK(t).Decode(t, &r3)
	if r3.Order.Mode != billing.ModeDowngrade || r3.Membership.Tier.Tier != "pro_plus" || len(r3.Membership.Scheduled) != 1 {
		t.Fatalf("membership after downgrade = %+v", r3.Membership)
	}
	sched := r3.Membership.Scheduled[0]
	if sched.Tier != "pro" || !sched.StartsAt.Equal(proPlusEnd) {
		t.Fatalf("scheduled = %+v, want start %s", sched, proPlusEnd)
	}

	// 取消预约：全额退回余额；生效中的订阅不能按预约取消。
	var m billing.Membership
	a.Do(http.MethodDelete, fmt.Sprintf("/api/v1/me/membership/scheduled/%d", sched.ID), tok, nil).OK(t).Decode(t, &m)
	if len(m.Scheduled) != 0 || m.BalanceMicros != r3.Membership.BalanceMicros+20_000_000 {
		t.Fatalf("membership after cancel = %+v", m)
	}
	a.Do(http.MethodDelete, fmt.Sprintf("/api/v1/me/membership/scheduled/%d", sched.ID), tok, nil).Expect(t, 409, "NOT_SCHEDULED")
	a.Do(http.MethodDelete, fmt.Sprintf("/api/v1/me/membership/scheduled/%d", *r2.Order.SubscriptionID), tok, nil).
		Expect(t, 409, "NOT_SCHEDULED")

	if n := a.Int64(`SELECT count(*) FROM balance_ledger WHERE user_id = $1 AND kind = 'subscription'`, s.UserID); n != 3 {
		t.Fatalf("subscription 流水 = %d", n)
	}
	if n := a.Int64(`SELECT count(*) FROM balance_ledger WHERE user_id = $1 AND kind = 'refund'`, s.UserID); n != 1 {
		t.Fatalf("refund 流水 = %d", n)
	}
	var orders struct {
		Items []billing.Order `json:"items"`
		Total int64           `json:"total"`
	}
	a.Do(http.MethodGet, "/api/v1/me/orders", tok, nil).OK(t).Decode(t, &orders)
	if orders.Total != 3 || orders.Items[0].Mode != billing.ModeDowngrade || orders.Items[2].Mode != billing.ModeNew {
		t.Fatalf("orders = %+v", orders)
	}
}

func TestCheckoutErrors(t *testing.T) {
	a := cloudtest.New(t)
	s := a.NewUser()
	tok := s.AccessToken
	a.Exec(`UPDATE users SET balance_micros = 1000000 WHERE id = $1`, s.UserID)
	pro := tierPlanID(t, a, "pro")

	m := checkout(a, tok, pro, "month", "balance").Expect(t, 402, core.CodeInsufficientBalance).Map(t)
	if int64(m["balanceMicros"].(float64)) != 1_000_000 || int64(m["amountMicros"].(float64)) != 20_000_000 {
		t.Fatalf("402 extras = %v", m)
	}
	checkout(a, tok, pro, "month", "stripe").Expect(t, 501, "PAYMENT_NOT_CONFIGURED")
	checkout(a, tok, tierPlanID(t, a, "hobby"), "month", "balance").Expect(t, 400, "PLAN_NOT_PURCHASABLE")
	checkout(a, tok, pro, "week", "balance").Expect(t, 400, "INVALID_INTERVAL")
	checkout(a, tok, a.CreatePlan("Pack", 30, 1_000_000, 0, 0), "month", "balance").Expect(t, 404, "PLAN_NOT_FOUND")
	a.Do(http.MethodPost, "/api/v1/me/membership/quote", tok, map[string]any{}).Expect(t, 400, "INVALID_REQUEST")
	if n := a.Int64(`SELECT count(*) FROM payment_orders WHERE user_id = $1`, s.UserID); n != 0 {
		t.Fatalf("失败的下单不应留下订单，实际 %d 张", n)
	}
}

func TestOnlinePaymentNotifyAndAdminMarkPaid(t *testing.T) {
	a := cloudtest.New(t)
	a.Billing.RegisterPayment(fakePay{name: "fakepay"})
	s := a.NewUser()
	tok := s.AccessToken
	a.Exec(`UPDATE users SET balance_micros = 0 WHERE id = $1`, s.UserID)
	pro, ultra := tierPlanID(t, a, "pro"), tierPlanID(t, a, "ultra")
	notify := func(id int64, paid bool) {
		t.Helper()
		a.Do(http.MethodPost, "/api/v1/payments/fakepay/notify", "", map[string]any{"orderId": id, "paid": paid}).OK(t)
	}

	var r1 checkoutResp
	checkout(a, tok, pro, "year", "fakepay").OK(t).Decode(t, &r1)
	if r1.Order.Status != billing.OrderPending || r1.Order.Provider != "fakepay" || r1.Order.AmountMicros != 192_000_000 ||
		!strings.HasPrefix(r1.Order.PayURL, "https://pay.example.test/o/") {
		t.Fatalf("pending order = %+v", r1.Order)
	}
	if m := r1.Membership; m.PendingOrder == nil || m.PendingOrder.ID != r1.Order.ID || m.Tier.Tier != billing.TierHobby ||
		!m.Payment.Enabled || len(m.Payment.Providers) != 1 {
		t.Fatalf("membership with pending order = %+v", m)
	}
	notify(r1.Order.ID, true)
	notify(r1.Order.ID, true) // 重复回调幂等
	m := getMembership(t, a, tok)
	if m.Tier.Tier != "pro" || m.BalanceMicros != 0 || m.PendingOrder != nil || m.Subscription.BillingInterval != "year" {
		t.Fatalf("membership after notify = %+v", m)
	}
	if n := a.Int64(`SELECT count(*) FROM subscriptions WHERE user_id = $1 AND source = 'purchase'`, s.UserID); n != 1 {
		t.Fatalf("重复回调不应重复开通，purchase 订阅 = %d", n)
	}

	// 新下单取消旧的待支付单；管理员线下确认收款后生效。
	var r2, r3 checkoutResp
	checkout(a, tok, ultra, "month", "fakepay").OK(t).Decode(t, &r2)
	checkout(a, tok, ultra, "month", "fakepay").OK(t).Decode(t, &r3)
	if r3.Order.Mode != billing.ModeUpgrade || r3.Order.CreditMicros <= 0 {
		t.Fatalf("upgrade order = %+v", r3.Order)
	}
	if st := a.String(`SELECT status FROM payment_orders WHERE id = $1`, r2.Order.ID); st != billing.OrderCancelled {
		t.Fatalf("旧待支付单应被取消，status=%s", st)
	}
	adm := a.NewAdmin()
	var listed struct {
		Items []billing.AdminOrder `json:"items"`
		Total int64                `json:"total"`
	}
	a.Do(http.MethodGet, "/api/admin/orders?status=pending&kind=subscription", adm.AccessToken, nil).OK(t).Decode(t, &listed)
	if listed.Total != 1 || listed.Items[0].ID != r3.Order.ID || listed.Items[0].UserEmail != s.Email {
		t.Fatalf("admin orders = %+v", listed)
	}
	var paid billing.AdminOrder
	a.Do(http.MethodPost, fmt.Sprintf("/api/admin/orders/%d/mark-paid", r3.Order.ID), adm.AccessToken,
		map[string]any{"note": "线下转账"}).OK(t).Decode(t, &paid)
	if paid.Status != billing.OrderPaid || paid.SubscriptionID == nil || !strings.Contains(paid.Note, "线下转账") {
		t.Fatalf("mark-paid = %+v", paid)
	}
	a.Do(http.MethodPost, fmt.Sprintf("/api/admin/orders/%d/mark-paid", r3.Order.ID), adm.AccessToken, nil).
		Expect(t, 409, "ORDER_NOT_PENDING")
	a.Do(http.MethodPost, fmt.Sprintf("/api/admin/orders/%d/cancel", r3.Order.ID), adm.AccessToken, nil).
		Expect(t, 409, "ORDER_NOT_PENDING")
	if m := getMembership(t, a, tok); m.Tier.Tier != "ultra" || m.BalanceMicros != 0 {
		t.Fatalf("membership after mark-paid = %+v", m)
	}
	if a.Int64(`SELECT count(*) FROM audit_logs WHERE action = 'order.mark_paid'`) != 1 {
		t.Fatal("mark-paid 应写审计")
	}

	// 已取消的订单事后到账 → 款项转入余额。
	notify(r2.Order.ID, true)
	if bal := a.Int64(`SELECT balance_micros FROM users WHERE id = $1`, s.UserID); bal != r2.Order.AmountMicros {
		t.Fatalf("关闭后到账应转入余额：balance=%d want %d", bal, r2.Order.AmountMicros)
	}

	// 用户取消自己的待支付单；别人的单 404。
	var r4 checkoutResp
	checkout(a, tok, ultra, "year", "fakepay").OK(t).Decode(t, &r4)
	if r4.Order.Mode != billing.ModeRenew || r4.Order.Status != billing.OrderPending {
		t.Fatalf("renew order = %+v", r4.Order)
	}
	other := a.NewUser()
	a.Do(http.MethodPost, fmt.Sprintf("/api/v1/me/orders/%d/cancel", r4.Order.ID), other.AccessToken, nil).
		Expect(t, 404, "ORDER_NOT_FOUND")
	a.Do(http.MethodPost, fmt.Sprintf("/api/v1/me/orders/%d/cancel", r4.Order.ID), tok, nil).OK(t)
	a.Do(http.MethodPost, fmt.Sprintf("/api/v1/me/orders/%d/cancel", r4.Order.ID), tok, nil).Expect(t, 409, "ORDER_NOT_PENDING")
}
