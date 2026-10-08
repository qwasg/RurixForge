// Package billing：余额与流水、兑换码、套餐订阅、用量查询、计费预检与结算（实现 core.Biller）、
// 在线支付接口占位（15_CLOUD_SERVICE.md §3.2、§5）。
package billing

import (
	"context"
	"log/slog"
	"net/http"
	"sync"

	"github.com/go-chi/chi/v5"
	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/redis/go-redis/v9"

	"forge-cloud/internal/core"
	"forge-cloud/internal/syssettings"
)

// PaymentProvider 是在线支付渠道接口（一期无实现，下单返回 501 PAYMENT_NOT_CONFIGURED）。
type PaymentProvider interface {
	Name() string
	// CreateOrder 创建支付单，返回给用户跳转/扫码的地址。
	CreateOrder(ctx context.Context, orderID int64, userID int64, amountMicros int64) (payURL string, err error)
	// HandleNotify 校验渠道回调并返回订单号与是否支付成功。
	HandleNotify(r *http.Request) (orderID int64, paid bool, err error)
}

type Service struct {
	db       *pgxpool.Pool
	rdb      *redis.Client
	log      *slog.Logger
	settings *syssettings.Store

	payMu    sync.RWMutex
	payments map[string]PaymentProvider
}

var _ core.Biller = (*Service)(nil)

func New(db *pgxpool.Pool, rdb *redis.Client, log *slog.Logger, settings *syssettings.Store) *Service {
	return &Service{db: db, rdb: rdb, log: log, settings: settings, payments: map[string]PaymentProvider{}}
}

// RegisterPayment 注册在线支付渠道（一期无调用方）；名称 balance 保留给余额支付，忽略。
func (s *Service) RegisterPayment(p PaymentProvider) {
	if p.Name() == ProviderBalance {
		s.log.Warn("payment provider name reserved, ignored", "name", p.Name())
		return
	}
	s.payMu.Lock()
	s.payments[p.Name()] = p
	s.payMu.Unlock()
}

func (s *Service) payment(name string) (PaymentProvider, bool) {
	s.payMu.RLock()
	defer s.payMu.RUnlock()
	p, ok := s.payments[name]
	return p, ok
}

// MountPublic 挂 /plans、/tiers 与 /payments/{provider}/notify（无需登录）。
func (s *Service) MountPublic(r chi.Router) {
	r.Get("/plans", s.handlePlans)
	r.Get("/tiers", s.handleTiers)
	r.Post("/payments/{provider}/notify", s.handleNotify)
}

// MountUser 挂余额/订阅/用量/流水/兑换（§3.2）与会员、订单接口（§11.2）。
func (s *Service) MountUser(r chi.Router) {
	r.Get("/me/balance", s.handleBalance)
	r.Get("/me/subscription", s.handleSubscriptions)
	r.Get("/me/usage", s.handleUsage)
	r.Get("/me/usage/daily", s.handleUsageDaily)
	r.Get("/me/ledger", s.handleLedger)
	r.Post("/me/redeem", s.handleRedeem)
	r.Get("/me/membership", s.handleMembership)
	r.Patch("/me/membership/on-demand", s.handleOnDemand)
	r.Get("/me/membership/usage", s.handleMembershipUsage)
	r.Post("/me/membership/quote", s.handleQuote)
	r.Post("/me/membership/checkout", s.handleCheckout)
	r.Delete("/me/membership/scheduled/{id}", s.handleCancelScheduled)
	r.Get("/me/orders", s.handleListOrders)
	r.Post("/me/orders", s.handleCreateOrder)
	r.Post("/me/orders/{id}/cancel", s.handleCancelOrder)
}
