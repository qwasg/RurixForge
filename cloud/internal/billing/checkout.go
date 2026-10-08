package billing

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"strconv"
	"strings"
	"time"

	"github.com/jackc/pgx/v5"

	"forge-cloud/internal/core"
)

func intervalLabel(v string) string {
	if v == IntervalYear {
		return "按年"
	}
	return "按月"
}

func modeLabel(m string) string {
	switch m {
	case ModeUpgrade:
		return "升级"
	case ModeDowngrade:
		return "降级"
	case ModeRenew:
		return "续订"
	default:
		return "开通"
	}
}

func lockUser(ctx context.Context, tx pgx.Tx, userID int64) error {
	var id int64
	err := tx.QueryRow(ctx, `SELECT id FROM users WHERE id = $1 FOR UPDATE`, userID).Scan(&id)
	if errors.Is(err, pgx.ErrNoRows) {
		return core.NotFound("USER_NOT_FOUND", "用户不存在")
	}
	return err
}

// Checkout 按报价下单（§11.1）：余额支付或应付为 0 → 同一事务里生效；在线渠道 → pending，再向渠道要支付地址。
func (s *Service) Checkout(ctx context.Context, userID, planID int64, interval, provider string) (*Order, error) {
	provider = strings.TrimSpace(provider)
	if provider == "" {
		provider = ProviderBalance
	}
	var online PaymentProvider
	if provider != ProviderBalance {
		p, ok := s.payment(provider)
		if !ok {
			return nil, errPaymentNotConfigured()
		}
		online = p
	}
	now := time.Now().UTC().Truncate(time.Second)
	var orderID, amount int64
	needPay := false
	err := pgx.BeginFunc(ctx, s.db, func(tx pgx.Tx) error {
		q, err := s.buildQuote(ctx, tx, userID, planID, interval, now, true)
		if err != nil {
			return err
		}
		payNow := online == nil || q.AmountMicros == 0
		if online == nil && q.BalanceMicros < q.AmountMicros {
			return insufficientBalance(q.BalanceMicros, q.AmountMicros)
		}
		// 同一用户只保留一张待支付的会员单。
		if _, err := tx.Exec(ctx, `UPDATE payment_orders SET status = 'cancelled', note = note || '（已被新订单取代）',
			updated_at = now() WHERE user_id = $1 AND kind = 'subscription' AND status = 'pending'`, userID); err != nil {
			return err
		}
		prov := provider
		if payNow {
			prov = ProviderBalance
		}
		payload, err := json.Marshal(map[string]any{"replaces": q.ReplacesSubscriptionIDs})
		if err != nil {
			return err
		}
		var replaces *int64
		if len(q.ReplacesSubscriptionIDs) > 0 {
			replaces = &q.ReplacesSubscriptionIDs[0]
		}
		note := q.PlanName + " · " + intervalLabel(q.Interval) + " · " + modeLabel(q.Mode)
		if err := tx.QueryRow(ctx, `INSERT INTO payment_orders (user_id, provider, amount_micros, status, kind, plan_id,
			billing_interval, mode, list_price_micros, credit_micros, replaces_subscription_id, payload, note)
			VALUES ($1, $2, $3, 'pending', 'subscription', $4, $5, $6, $7, $8, $9, $10, $11) RETURNING id`,
			userID, prov, q.AmountMicros, q.PlanID, q.Interval, q.Mode, q.ListPriceMicros, q.CreditMicros,
			replaces, string(payload), note).Scan(&orderID); err != nil {
			return err
		}
		if payNow {
			return s.activateOrderTx(ctx, tx, orderID, now, "")
		}
		needPay, amount = true, q.AmountMicros
		return nil
	})
	if err != nil {
		return nil, err
	}
	if needPay {
		payURL, err := online.CreateOrder(ctx, orderID, userID, amount)
		if err != nil {
			s.log.Warn("create payment order failed", "provider", online.Name(), "order", orderID, "err", err)
			_, _ = s.db.Exec(ctx, `UPDATE payment_orders SET status = 'failed', updated_at = now() WHERE id = $1`, orderID)
			return nil, core.E(http.StatusBadGateway, "PAYMENT_FAILED", "创建支付单失败")
		}
		if _, err := s.db.Exec(ctx, `UPDATE payment_orders SET pay_url = $2, updated_at = now() WHERE id = $1`,
			orderID, payURL); err != nil {
			return nil, err
		}
	}
	o, err := GetOrder(ctx, s.db, userID, orderID)
	if err != nil {
		return nil, err
	}
	return &o.Order, nil
}

// activateOrderTx 在调用方事务里让一张 pending 订单生效（非 pending → ORDER_NOT_PENDING）。先锁用户行再锁订单行。
// 充值单加余额；会员单：升级作废被替换的已购订阅，按模式定开始时间新建订阅（value = 标价、source = purchase），
// 余额支付扣余额（流水 subscription），抵扣超出标价的部分退回余额（流水 refund）。
func (s *Service) activateOrderTx(ctx context.Context, tx pgx.Tx, orderID int64, now time.Time, adminNote string) error {
	var userID int64
	err := tx.QueryRow(ctx, `SELECT user_id FROM payment_orders WHERE id = $1`, orderID).Scan(&userID)
	if errors.Is(err, pgx.ErrNoRows) {
		return errOrderNotFound()
	}
	if err != nil {
		return err
	}
	if err := lockUser(ctx, tx, userID); err != nil {
		return err
	}
	var (
		kind, provider, status, interval, mode, note string
		amount, list, credit                         int64
		planID                                       *int64
		payload                                      []byte
	)
	if err := tx.QueryRow(ctx, `SELECT kind, provider, status, amount_micros, list_price_micros, credit_micros,
		plan_id, billing_interval, mode, payload, note FROM payment_orders WHERE id = $1 FOR UPDATE`, orderID).
		Scan(&kind, &provider, &status, &amount, &list, &credit, &planID, &interval, &mode, &payload, &note); err != nil {
		return err
	}
	if status != OrderPending {
		return errOrderNotPending()
	}
	ref := "order:" + strconv.FormatInt(orderID, 10)
	var subID *int64
	if kind == OrderKindSubscription {
		if planID == nil {
			return core.Conflict("ORDER_INVALID", "订单缺少会员档位")
		}
		var extra struct {
			Replaces []int64 `json:"replaces"`
		}
		_ = json.Unmarshal(payload, &extra)
		start := now
		if mode == ModeRenew || mode == ModeDowngrade {
			// 接在当前档位订阅链末尾（下单到生效之间链可能变长）。
			var chainEnd *time.Time
			if err := tx.QueryRow(ctx, `SELECT max(s.ends_at) FROM subscriptions s JOIN plans p ON p.id = s.plan_id
				WHERE s.user_id = $1 AND s.status = 'active' AND s.ends_at > $2 AND p.tier <> ''`, userID, now).
				Scan(&chainEnd); err != nil {
				return err
			}
			if chainEnd != nil && chainEnd.After(start) {
				start = chainEnd.UTC()
			}
		}
		if mode == ModeUpgrade && len(extra.Replaces) > 0 {
			if _, err := tx.Exec(ctx, `UPDATE subscriptions SET status = 'cancelled'
				WHERE user_id = $1 AND id = ANY($2) AND status = 'active'`, userID, extra.Replaces); err != nil {
				return err
			}
		}
		var id int64
		err := tx.QueryRow(ctx, `INSERT INTO subscriptions (user_id, plan_id, status, starts_at, ends_at, quota_micros,
			forge_quota_micros, daily_limit_micros, group_id, usage_cycle, billing_interval, value_micros, source, order_id)
			SELECT $1, p.id, 'active', $3, $4, p.quota_micros, p.forge_quota_micros, p.daily_limit_micros, p.group_id,
			       'month', $5, $6, 'purchase', $7
			FROM plans p WHERE p.id = $2 RETURNING id`,
			userID, *planID, start, addMonths(start, termMonths(interval)), interval, list, orderID).Scan(&id)
		if errors.Is(err, pgx.ErrNoRows) {
			return core.NotFound("PLAN_NOT_FOUND", "会员档位不存在")
		}
		if err != nil {
			return err
		}
		subID = &id
		if provider == ProviderBalance && amount > 0 {
			var bal int64
			if err := tx.QueryRow(ctx, `SELECT balance_micros FROM users WHERE id = $1`, userID).Scan(&bal); err != nil {
				return err
			}
			if bal < amount {
				return insufficientBalance(bal, amount)
			}
			if _, err := AdjustBalance(ctx, tx, userID, -amount, "subscription", ref, note); err != nil {
				return err
			}
		}
		if refund := credit - list; refund > 0 {
			if _, err := AdjustBalance(ctx, tx, userID, refund, "refund", ref, "升级抵扣超出标价，差额退回余额"); err != nil {
				return err
			}
		}
	} else if _, err := AdjustBalance(ctx, tx, userID, amount, "payment", ref, provider); err != nil {
		return err
	}
	_, err = tx.Exec(ctx, `UPDATE payment_orders SET status = 'paid', paid_at = $2, subscription_id = $3,
		note = CASE WHEN $4::text = '' THEN note ELSE note || '；' || $4::text END, updated_at = now() WHERE id = $1`,
		orderID, now, subID, adminNote)
	return err
}

// applyNotify 处理渠道回调（幂等）：pending + 成功 → 生效；pending + 失败 → failed；
// 已取消的订单事后到账 → 款项转入余额并标记 paid；其余状态（含重复回调）忽略。
func (s *Service) applyNotify(ctx context.Context, provider string, orderID int64, paid bool) error {
	now := time.Now().UTC().Truncate(time.Second)
	return pgx.BeginFunc(ctx, s.db, func(tx pgx.Tx) error {
		var userID int64
		err := tx.QueryRow(ctx, `SELECT user_id FROM payment_orders WHERE id = $1 AND provider = $2`, orderID, provider).
			Scan(&userID)
		if errors.Is(err, pgx.ErrNoRows) {
			return errOrderNotFound()
		}
		if err != nil {
			return err
		}
		if err := lockUser(ctx, tx, userID); err != nil {
			return err
		}
		var status string
		var amount int64
		if err := tx.QueryRow(ctx, `SELECT status, amount_micros FROM payment_orders WHERE id = $1 FOR UPDATE`, orderID).
			Scan(&status, &amount); err != nil {
			return err
		}
		switch {
		case status == OrderPending && paid:
			return s.activateOrderTx(ctx, tx, orderID, now, "")
		case status == OrderPending:
			_, err := tx.Exec(ctx, `UPDATE payment_orders SET status = 'failed', updated_at = now() WHERE id = $1`, orderID)
			return err
		case status == OrderCancelled && paid && amount > 0:
			if _, err := AdjustBalance(ctx, tx, userID, amount, "payment", "order:"+strconv.FormatInt(orderID, 10),
				"订单关闭后到账，款项转入余额"); err != nil {
				return err
			}
			_, err := tx.Exec(ctx, `UPDATE payment_orders SET status = 'paid', paid_at = $2,
				note = note || '（关闭后到账，款项已转入余额）', updated_at = now() WHERE id = $1`, orderID, now)
			return err
		}
		return nil
	})
}

// MarkOrderPaid 管理员确认线下收款：pending 订单按 §11.1 生效。
func (s *Service) MarkOrderPaid(ctx context.Context, orderID int64, note string) (AdminOrder, error) {
	now := time.Now().UTC().Truncate(time.Second)
	if err := pgx.BeginFunc(ctx, s.db, func(tx pgx.Tx) error {
		return s.activateOrderTx(ctx, tx, orderID, now, note)
	}); err != nil {
		return AdminOrder{}, err
	}
	return GetOrder(ctx, s.db, 0, orderID)
}

// CancelScheduled 取消排在最后、尚未开始的档位订阅；已购价值全额退回余额（流水 refund）。
func (s *Service) CancelScheduled(ctx context.Context, userID, subID int64) error {
	now := time.Now()
	return pgx.BeginFunc(ctx, s.db, func(tx pgx.Tx) error {
		if err := lockUser(ctx, tx, userID); err != nil {
			return err
		}
		var (
			startsAt             time.Time
			value                int64
			source, status, name string
		)
		err := tx.QueryRow(ctx, `SELECT s.starts_at, s.value_micros, s.source, s.status, p.name
			FROM subscriptions s JOIN plans p ON p.id = s.plan_id
			WHERE s.id = $1 AND s.user_id = $2 AND p.tier <> '' FOR UPDATE OF s`, subID, userID).
			Scan(&startsAt, &value, &source, &status, &name)
		if errors.Is(err, pgx.ErrNoRows) {
			return core.NotFound("SUBSCRIPTION_NOT_FOUND", "订阅不存在")
		}
		if err != nil {
			return err
		}
		if status != "active" || !startsAt.After(now) {
			return core.Conflict("NOT_SCHEDULED", "该订阅已生效或已取消，不能取消预约")
		}
		var later bool
		if err := tx.QueryRow(ctx, `SELECT EXISTS (SELECT 1 FROM subscriptions s JOIN plans p ON p.id = s.plan_id
			WHERE s.user_id = $1 AND s.status = 'active' AND p.tier <> '' AND s.starts_at > $2)`, userID, startsAt).
			Scan(&later); err != nil {
			return err
		}
		if later {
			return core.Conflict("SCHEDULE_NOT_LAST", "请先取消排在后面的预约")
		}
		if _, err := tx.Exec(ctx, `UPDATE subscriptions SET status = 'cancelled' WHERE id = $1`, subID); err != nil {
			return err
		}
		if source == SourcePurchase && value > 0 {
			if _, err := AdjustBalance(ctx, tx, userID, value, "refund", "subscription:"+strconv.FormatInt(subID, 10),
				"取消预约的 "+name+"，退回余额"); err != nil {
				return err
			}
		}
		return nil
	})
}
