package billing

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"time"

	"github.com/jackc/pgx/v5"

	"forge-cloud/internal/core"
)

// 会员订单（15_CLOUD_SERVICE.md §11.3）：报价 → 下单 → 生效。充值单与会员单共用 payment_orders。
// 余额支付（provider=balance）与应付为 0 的会员单在下单事务里直接生效；在线支付渠道下单后为 pending，
// 由渠道回调或管理员「标记已支付」生效。锁顺序与结算一致：先用户行，再订阅/订单行。

const (
	OrderKindTopup        = "topup"
	OrderKindSubscription = "subscription"

	OrderPending   = "pending"
	OrderPaid      = "paid"
	OrderCancelled = "cancelled"
	OrderFailed    = "failed"

	ModeNew       = "new"
	ModeRenew     = "renew"
	ModeUpgrade   = "upgrade"
	ModeDowngrade = "downgrade"

	// ProviderBalance 表示用余额支付，不经在线支付渠道。
	ProviderBalance = "balance"
)

// Order 是订单的对外形状。amountMicros = 应付；listPriceMicros = 档位标价；creditMicros = 升级抵扣。
type Order struct {
	ID                     int64      `json:"id"`
	Kind                   string     `json:"kind"`
	Provider               string     `json:"provider"`
	Status                 string     `json:"status"`
	AmountMicros           int64      `json:"amountMicros"`
	ListPriceMicros        int64      `json:"listPriceMicros"`
	CreditMicros           int64      `json:"creditMicros"`
	PlanID                 *int64     `json:"planId"`
	PlanName               string     `json:"planName"`
	Tier                   string     `json:"tier"`
	Interval               string     `json:"interval"`
	Mode                   string     `json:"mode"`
	ReplacesSubscriptionID *int64     `json:"replacesSubscriptionId"`
	SubscriptionID         *int64     `json:"subscriptionId"`
	PayURL                 string     `json:"payUrl"`
	Note                   string     `json:"note"`
	CreatedAt              time.Time  `json:"createdAt"`
	PaidAt                 *time.Time `json:"paidAt"`
}

// AdminOrder 是管理端订单条目（多带下单用户）。
type AdminOrder struct {
	Order
	UserID    int64  `json:"userId"`
	UserEmail string `json:"userEmail"`
}

const orderCols = `o.id, o.kind, o.provider, o.status, o.amount_micros, o.list_price_micros, o.credit_micros,
	o.plan_id, COALESCE(p.name, ''), COALESCE(p.tier, ''), o.billing_interval, o.mode, o.replaces_subscription_id,
	o.subscription_id, o.pay_url, o.note, o.created_at, o.paid_at, o.user_id, COALESCE(u.email, '')`

const orderFrom = ` FROM payment_orders o LEFT JOIN plans p ON p.id = o.plan_id LEFT JOIN users u ON u.id = o.user_id`

func scanOrder(row pgx.Row) (AdminOrder, error) {
	var o AdminOrder
	err := row.Scan(&o.ID, &o.Kind, &o.Provider, &o.Status, &o.AmountMicros, &o.ListPriceMicros, &o.CreditMicros,
		&o.PlanID, &o.PlanName, &o.Tier, &o.Interval, &o.Mode, &o.ReplacesSubscriptionID, &o.SubscriptionID,
		&o.PayURL, &o.Note, &o.CreatedAt, &o.PaidAt, &o.UserID, &o.UserEmail)
	if err != nil {
		return o, err
	}
	o.CreatedAt = o.CreatedAt.UTC()
	if o.PaidAt != nil {
		t := o.PaidAt.UTC()
		o.PaidAt = &t
	}
	return o, nil
}

func errOrderNotFound() error { return core.NotFound("ORDER_NOT_FOUND", "订单不存在") }
func errOrderNotPending() error {
	return core.Conflict("ORDER_NOT_PENDING", "订单不是待支付状态")
}

// GetOrder 按 ID 读订单；userID > 0 时只读该用户的（别人的订单同样 404）。
func GetOrder(ctx context.Context, q Querier, userID, id int64) (AdminOrder, error) {
	sql := `SELECT ` + orderCols + orderFrom + ` WHERE o.id = $1`
	args := []any{id}
	if userID > 0 {
		sql += ` AND o.user_id = $2`
		args = append(args, userID)
	}
	o, err := scanOrder(q.QueryRow(ctx, sql, args...))
	if errors.Is(err, pgx.ErrNoRows) {
		return o, errOrderNotFound()
	}
	return o, err
}

// OrderFilter 为空字段表示不过滤；Q 按用户邮箱包含匹配（管理端）。
type OrderFilter struct {
	UserID int64
	Status string
	Kind   string
	Q      string
}

// containsPattern 把用户输入转成 ILIKE 的包含匹配（转义 \ % _）。
func containsPattern(q string) string {
	return "%" + strings.NewReplacer(`\`, `\\`, `%`, `\%`, `_`, `\_`).Replace(q) + "%"
}

// ListOrders 返回订单（新的在前）与总数。
func ListOrders(ctx context.Context, q Querier, f OrderFilter, limit, offset int) ([]AdminOrder, int64, error) {
	var conds []string
	var args []any
	add := func(cond string, v any) {
		args = append(args, v)
		conds = append(conds, fmt.Sprintf(cond, len(args)))
	}
	if f.UserID > 0 {
		add("o.user_id = $%d", f.UserID)
	}
	if f.Status != "" {
		add("o.status = $%d", f.Status)
	}
	if f.Kind != "" {
		add("o.kind = $%d", f.Kind)
	}
	if q := strings.TrimSpace(f.Q); q != "" {
		add("u.email ILIKE $%d", containsPattern(q))
	}
	where := ""
	if len(conds) > 0 {
		where = " WHERE " + strings.Join(conds, " AND ")
	}
	var total int64
	if err := q.QueryRow(ctx, `SELECT count(*)`+orderFrom+where, args...).Scan(&total); err != nil {
		return nil, 0, err
	}
	rows, err := q.Query(ctx, fmt.Sprintf(`SELECT %s%s%s ORDER BY o.id DESC LIMIT %d OFFSET %d`,
		orderCols, orderFrom, where, limit, offset), args...)
	if err != nil {
		return nil, 0, err
	}
	defer rows.Close()
	out := []AdminOrder{}
	for rows.Next() {
		o, err := scanOrder(rows)
		if err != nil {
			return nil, 0, err
		}
		out = append(out, o)
	}
	return out, total, rows.Err()
}

// pendingSubscriptionOrder 返回用户最新一张待支付的会员订单（没有返回 nil）。
func (s *Service) pendingSubscriptionOrder(ctx context.Context, userID int64) (*Order, error) {
	o, err := scanOrder(s.db.QueryRow(ctx, `SELECT `+orderCols+orderFrom+
		` WHERE o.user_id = $1 AND o.kind = 'subscription' AND o.status = 'pending' ORDER BY o.id DESC LIMIT 1`, userID))
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	return &o.Order, nil
}

// CancelOrder 取消一张待支付订单；userID > 0 时只能取消自己的。非 pending → 409 ORDER_NOT_PENDING。
func CancelOrder(ctx context.Context, q Querier, userID, orderID int64) (AdminOrder, error) {
	sql := `UPDATE payment_orders SET status = 'cancelled', updated_at = now() WHERE id = $1 AND status = 'pending'`
	args := []any{orderID}
	if userID > 0 {
		sql += ` AND user_id = $2`
		args = append(args, userID)
	}
	tag, err := q.Exec(ctx, sql, args...)
	if err != nil {
		return AdminOrder{}, err
	}
	o, err := GetOrder(ctx, q, userID, orderID)
	if err != nil {
		return o, err
	}
	if tag.RowsAffected() == 0 {
		return o, errOrderNotPending()
	}
	return o, nil
}
