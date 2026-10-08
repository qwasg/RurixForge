// Package admin：管理 API（仪表盘、用户、分组、套餐、兑换码、用量、系统设置、审计）
// （15_CLOUD_SERVICE.md §7）。上游账号与模型的管理接口在 accounts 包。
package admin

import (
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"net/http"
	"strings"

	"github.com/go-chi/chi/v5"
	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/redis/go-redis/v9"

	"forge-cloud/internal/audit"
	"forge-cloud/internal/auth"
	"forge-cloud/internal/billing"
	"forge-cloud/internal/config"
	"forge-cloud/internal/core"
	"forge-cloud/internal/httpx"
	"forge-cloud/internal/syssettings"
)

type Deps struct {
	DB       *pgxpool.Pool
	Redis    *redis.Client
	Config   *config.Config
	Log      *slog.Logger
	Settings *syssettings.Store
	Auth     *auth.Service
	Billing  *billing.Service
}

type Service struct{ d Deps }

func New(d Deps) *Service { return &Service{d: d} }

// Mount 挂 /dashboard、/users…、/groups…、/plans…、/redeem-codes…、/usage、/settings、/audit-logs
// （相对 /api/admin，已在 RequireUser+RequireAdmin 分组内）。
func (s *Service) Mount(r chi.Router) {
	r.Get("/dashboard", s.handleDashboard)
	r.Get("/users", s.handleListUsers)
	r.Post("/users", s.handleCreateUser)
	r.Get("/users/{id}", s.handleGetUser)
	r.Patch("/users/{id}", s.handlePatchUser)
	r.Post("/users/{id}/balance", s.handleAdjustBalance)
	r.Post("/users/{id}/password", s.handleSetPassword)
	r.Post("/users/{id}/subscriptions", s.handleGrantSubscription)
	r.Delete("/users/{id}/subscriptions/{subId}", s.handleCancelSubscription)
	r.Get("/groups", s.handleListGroups)
	r.Post("/groups", s.handleCreateGroup)
	r.Patch("/groups/{id}", s.handlePatchGroup)
	r.Delete("/groups/{id}", s.handleDeleteGroup)
	r.Get("/plans", s.handleListPlans)
	r.Post("/plans", s.handleCreatePlan)
	r.Patch("/plans/{id}", s.handlePatchPlan)
	r.Delete("/plans/{id}", s.handleDeletePlan)
	r.Get("/orders", s.handleListOrders)
	r.Post("/orders/{id}/mark-paid", s.handleMarkOrderPaid)
	r.Post("/orders/{id}/cancel", s.handleCancelOrder)
	r.Get("/redeem-codes", s.handleListRedeemCodes)
	r.Post("/redeem-codes", s.handleCreateRedeemCodes)
	r.Get("/redeem-codes/export", s.handleExportRedeemCodes)
	r.Post("/redeem-codes/{id}/revoke", s.handleRevokeRedeemCode)
	r.Get("/usage", s.handleUsage)
	r.Get("/settings", s.handleGetSettings)
	r.Put("/settings", s.handlePutSettings)
	r.Get("/audit-logs", s.handleAuditLogs)
}

func actorID(r *http.Request) int64 {
	if c, ok := core.ClaimsFrom(r.Context()); ok {
		return c.UserID
	}
	return 0
}

// audit 记录一次管理写操作；detail 里绝不放密钥或密码。
func (s *Service) audit(r *http.Request, action, target string, detail any) {
	audit.Record(r.Context(), s.d.DB, actorID(r), action, target, detail, httpx.ClientIP(r, s.d.Config.TrustProxy))
}

func target(kind string, id any) string { return fmt.Sprintf("%s:%v", kind, id) }

func nullID(id int64) any {
	if id <= 0 {
		return nil
	}
	return id
}

func errInvalid(msg string) error { return core.BadRequest("INVALID_REQUEST", msg) }

// fields 是 PATCH 请求体的原始字段表，用于区分「未提供」与显式 null。
type fields map[string]json.RawMessage

func decodePatch(r *http.Request, dst any) (fields, error) {
	var raw json.RawMessage
	if err := httpx.DecodeJSON(r, &raw, 0); err != nil {
		return nil, err
	}
	f := fields{}
	if err := json.Unmarshal(raw, &f); err != nil {
		return nil, core.BadRequest("INVALID_JSON", "请求体必须是 JSON 对象")
	}
	if err := json.Unmarshal(raw, dst); err != nil {
		return nil, core.BadRequest("INVALID_JSON", "字段类型不正确")
	}
	return f, nil
}

func (f fields) has(k string) bool {
	_, ok := f[k]
	return ok
}

// updateSet 累积动态 UPDATE 的 SET 子句；$1 预留给主键。
type updateSet struct {
	cols []string
	args []any
}

func newUpdateSet(id any) *updateSet { return &updateSet{args: []any{id}} }

func (u *updateSet) add(col string, v any) {
	u.args = append(u.args, v)
	u.cols = append(u.cols, fmt.Sprintf("%s = $%d", col, len(u.args)))
}

// raw 追加不带参数的赋值表达式（如 updated_at = now()）。
func (u *updateSet) raw(expr string) { u.cols = append(u.cols, expr) }

func (u *updateSet) empty() bool { return len(u.cols) == 0 }

func (u *updateSet) clause() string { return strings.Join(u.cols, ", ") }

// likePattern 把用户输入转成 ILIKE 的包含匹配（转义 % 与 _）。
func likePattern(q string) string {
	return "%" + strings.NewReplacer(`\`, `\\`, `%`, `\%`, `_`, `\_`).Replace(q) + "%"
}

func isUniqueViolation(err error) bool {
	var pg *pgconn.PgError
	return errors.As(err, &pg) && pg.Code == "23505"
}

func isFKViolation(err error) bool {
	var pg *pgconn.PgError
	return errors.As(err, &pg) && pg.Code == "23503"
}

func isNoRows(err error) bool { return errors.Is(err, pgx.ErrNoRows) }
