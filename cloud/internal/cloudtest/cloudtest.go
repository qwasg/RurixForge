// Package cloudtest 是用户侧模块（auth、users、apikeys、userdata、billing、admin）的 HTTP 测试夹具：
// 路由分组与 server.go 一致，但不挂 accounts/gateway（它们由各自的测试覆盖）。只供 _test.go 使用。
package cloudtest

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/alicebob/miniredis/v2"
	"github.com/go-chi/chi/v5"
	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/redis/go-redis/v9"

	"forge-cloud/internal/admin"
	"forge-cloud/internal/apikeys"
	"forge-cloud/internal/auth"
	"forge-cloud/internal/billing"
	"forge-cloud/internal/config"
	"forge-cloud/internal/core"
	"forge-cloud/internal/httpx"
	"forge-cloud/internal/syssettings"
	"forge-cloud/internal/testutil"
	"forge-cloud/internal/userdata"
	"forge-cloud/internal/users"
)

// DefaultPassword 是 NewUser/NewAdmin 使用的密码。
const DefaultPassword = "password-123"

type App struct {
	T        testing.TB
	DB       *pgxpool.Pool
	Redis    *redis.Client
	MR       *miniredis.Miniredis
	Config   *config.Config
	Settings *syssettings.Store
	Auth     *auth.Service
	Users    *users.Service
	APIKeys  *apikeys.Service
	UserData *userdata.Service
	Billing  *billing.Service
	Admin    *admin.Service
	Handler  http.Handler

	seq int
}

// New 建一个隔离的测试应用（独立 PostgreSQL 库 + miniredis）；opts 可在构造服务前修改配置。
func New(t testing.TB, opts ...func(*config.Config)) *App {
	t.Helper()
	pool := testutil.NewDB(t)
	rdb, mr := testutil.NewRedis(t)
	cfg := testutil.Config()
	for _, o := range opts {
		o(cfg)
	}
	log := testutil.Logger()
	a := &App{T: t, DB: pool, Redis: rdb, MR: mr, Config: cfg}
	a.Settings = syssettings.New(pool)
	a.Auth = auth.New(pool, rdb, cfg, log.With("mod", "auth"), a.Settings)
	a.Users = users.New(pool, cfg, log.With("mod", "users"), a.Auth)
	a.APIKeys = apikeys.New(pool, log.With("mod", "apikeys"), a.Settings)
	a.UserData = userdata.New(pool, log.With("mod", "userdata"))
	a.Billing = billing.New(pool, rdb, log.With("mod", "billing"), a.Settings)
	a.Admin = admin.New(admin.Deps{
		DB: pool, Redis: rdb, Config: cfg, Log: log.With("mod", "admin"),
		Settings: a.Settings, Auth: a.Auth, Billing: a.Billing,
	})
	a.Handler = a.routes()
	return a
}

func (a *App) routes() http.Handler {
	r := chi.NewRouter()
	r.Use(httpx.RequestID)
	r.Route("/api/v1", func(r chi.Router) {
		a.Auth.MountPublic(r)
		a.Billing.MountPublic(r)
		r.Group(func(r chi.Router) {
			r.Use(a.Auth.RequireUser)
			a.Auth.MountUser(r)
			a.Users.Mount(r)
			a.APIKeys.Mount(r)
			a.Billing.MountUser(r)
			a.UserData.Mount(r)
		})
	})
	r.Route("/api/admin", func(r chi.Router) {
		r.Use(a.Auth.RequireUser, a.Auth.RequireAdmin)
		a.Admin.Mount(r)
	})
	return r
}

// Response 是一次请求的结果。
type Response struct {
	Status int
	Header http.Header
	Body   []byte
}

// Decode 把 JSON 响应体解到 v。
func (r *Response) Decode(t testing.TB, v any) {
	t.Helper()
	if err := json.Unmarshal(r.Body, v); err != nil {
		t.Fatalf("解析响应失败（HTTP %d）: %v\n%s", r.Status, err, r.Body)
	}
}

// Map 把 JSON 对象响应解成 map。
func (r *Response) Map(t testing.TB) map[string]any {
	t.Helper()
	var m map[string]any
	r.Decode(t, &m)
	return m
}

// Code 返回错误响应里的 error.code（非错误响应返回空串）。
func (r *Response) Code() string {
	var e struct {
		Error struct {
			Code string `json:"code"`
		} `json:"error"`
	}
	_ = json.Unmarshal(r.Body, &e)
	return e.Error.Code
}

// Expect 断言状态码（错误响应时同时断言 error.code，code 为空则不检查）。
func (r *Response) Expect(t testing.TB, status int, code string) *Response {
	t.Helper()
	if r.Status != status {
		t.Fatalf("HTTP %d，期望 %d：%s", r.Status, status, r.Body)
	}
	if code != "" && r.Code() != code {
		t.Fatalf("error.code = %q，期望 %q：%s", r.Code(), code, r.Body)
	}
	return r
}

// OK 断言 200。
func (r *Response) OK(t testing.TB) *Response {
	t.Helper()
	return r.Expect(t, http.StatusOK, "")
}

// Do 发一个 JSON 请求（body 为 nil 时不带请求体；string/[]byte 原样发送）。
func (a *App) Do(method, path, token string, body any) *Response {
	a.T.Helper()
	var rd io.Reader
	switch b := body.(type) {
	case nil:
	case string:
		rd = strings.NewReader(b)
	case []byte:
		rd = bytes.NewReader(b)
	default:
		raw, err := json.Marshal(b)
		if err != nil {
			a.T.Fatalf("序列化请求体: %v", err)
		}
		rd = bytes.NewReader(raw)
	}
	req := httptest.NewRequest(method, path, rd)
	if rd != nil {
		req.Header.Set("Content-Type", "application/json")
	}
	if token != "" {
		req.Header.Set("Authorization", "Bearer "+token)
	}
	return a.Serve(req)
}

// Serve 直接执行一个请求。
func (a *App) Serve(req *http.Request) *Response {
	rec := httptest.NewRecorder()
	a.Handler.ServeHTTP(rec, req)
	return &Response{Status: rec.Code, Header: rec.Header(), Body: rec.Body.Bytes()}
}

// Session 是一次登录的结果。
type Session struct {
	UserID       int64
	Email        string
	AccessToken  string
	RefreshToken string
	DeviceKey    string
	DeviceKeyID  int64
	DeviceID     string
}

type loginResponse struct {
	AccessToken  string `json:"accessToken"`
	RefreshToken string `json:"refreshToken"`
	User         struct {
		ID    int64  `json:"id"`
		Email string `json:"email"`
	} `json:"user"`
	DeviceKey *struct {
		ID  int64  `json:"id"`
		Key string `json:"key"`
	} `json:"deviceKey"`
}

func toSession(t testing.TB, res *Response, deviceID string) *Session {
	t.Helper()
	res.OK(t)
	var lr loginResponse
	res.Decode(t, &lr)
	s := &Session{UserID: lr.User.ID, Email: lr.User.Email, AccessToken: lr.AccessToken, RefreshToken: lr.RefreshToken, DeviceID: deviceID}
	if lr.DeviceKey != nil {
		s.DeviceKey, s.DeviceKeyID = lr.DeviceKey.Key, lr.DeviceKey.ID
	}
	return s
}

// Device 构造登录用的设备信息。
func Device(id string) map[string]any {
	return map[string]any{"id": id, "name": "PC-" + id, "platform": "windows", "appVersion": "0.1.0"}
}

// Register 注册并返回会话（非 200 直接失败）。
func (a *App) Register(email, password, deviceID string) *Session {
	a.T.Helper()
	res := a.Do(http.MethodPost, "/api/v1/auth/register", "", map[string]any{
		"email": email, "password": password, "device": Device(deviceID),
	})
	return toSession(a.T, res, deviceID)
}

// Login 登录并返回会话（非 200 直接失败）。
func (a *App) Login(email, password, deviceID string) *Session {
	a.T.Helper()
	res := a.Do(http.MethodPost, "/api/v1/auth/login", "", map[string]any{
		"email": email, "password": password, "device": Device(deviceID),
	})
	return toSession(a.T, res, deviceID)
}

// UniqueEmail 返回本应用内不重复的邮箱。
func (a *App) UniqueEmail(prefix string) string {
	a.seq++
	return fmt.Sprintf("%s%d@example.com", prefix, a.seq)
}

// NewUser 注册一个普通用户（密码 DefaultPassword）。
func (a *App) NewUser() *Session {
	a.T.Helper()
	return a.Register(a.UniqueEmail("user"), DefaultPassword, "dev-"+core.RandomString(8))
}

// NewAdmin 创建管理员并以 issueDeviceKey=false 登录。
func (a *App) NewAdmin() *Session {
	a.T.Helper()
	email := a.UniqueEmail("admin")
	if err := a.Auth.CreateAdmin(context.Background(), email, DefaultPassword); err != nil {
		a.T.Fatalf("创建管理员: %v", err)
	}
	res := a.Do(http.MethodPost, "/api/v1/auth/login", "", map[string]any{
		"email": email, "password": DefaultPassword, "device": Device("admin-console"), "issueDeviceKey": false,
	})
	return toSession(a.T, res, "admin-console")
}

// Exec 执行 SQL（失败直接终止测试）。
func (a *App) Exec(sql string, args ...any) {
	a.T.Helper()
	if _, err := a.DB.Exec(context.Background(), sql, args...); err != nil {
		a.T.Fatalf("SQL 失败: %v\n%s", err, sql)
	}
}

// Int64 执行返回单个整数的查询。
func (a *App) Int64(sql string, args ...any) int64 {
	a.T.Helper()
	var v int64
	if err := a.DB.QueryRow(context.Background(), sql, args...).Scan(&v); err != nil {
		a.T.Fatalf("SQL 失败: %v\n%s", err, sql)
	}
	return v
}

// String 执行返回单个字符串的查询。
func (a *App) String(sql string, args ...any) string {
	a.T.Helper()
	var v string
	if err := a.DB.QueryRow(context.Background(), sql, args...).Scan(&v); err != nil {
		a.T.Fatalf("SQL 失败: %v\n%s", err, sql)
	}
	return v
}

// UpdateSettings 用 JSON 补丁改系统设置。
func (a *App) UpdateSettings(patch string) {
	a.T.Helper()
	if _, err := a.Settings.Update(context.Background(), json.RawMessage(patch)); err != nil {
		a.T.Fatalf("更新设置: %v", err)
	}
}

// CreateGroup 直接插入分组，返回 ID。
func (a *App) CreateGroup(name string, multiplier float64, concurrency int) int64 {
	a.T.Helper()
	return a.Int64(`INSERT INTO groups (name, rate_multiplier, concurrency_limit) VALUES ($1, $2, $3) RETURNING id`,
		name, multiplier, concurrency)
}

// CreatePlan 直接插入套餐，返回 ID（groupID=0 表示不绑定分组）。
func (a *App) CreatePlan(name string, periodDays int, quota, dailyLimit, groupID int64) int64 {
	a.T.Helper()
	var group any
	if groupID > 0 {
		group = groupID
	}
	return a.Int64(`INSERT INTO plans (name, period_days, quota_micros, daily_limit_micros, group_id)
		VALUES ($1, $2, $3, $4, $5) RETURNING id`, name, periodDays, quota, dailyLimit, group)
}

// CreateRedeemCode 直接插入兑换码，返回 ID（planID=0 表示无套餐；expiresAt 为 nil 表示不过期）。
func (a *App) CreateRedeemCode(code, kind string, value, planID int64, maxUses int, expiresAt *time.Time) int64 {
	a.T.Helper()
	var plan any
	if planID > 0 {
		plan = planID
	}
	return a.Int64(`INSERT INTO redeem_codes (code, kind, value_micros, plan_id, max_uses, expires_at)
		VALUES ($1, $2, $3, $4, $5, $6) RETURNING id`, code, kind, value, plan, maxUses, expiresAt)
}

// Principal 用平台 Key 鉴权得到调用方（失败直接终止测试）。
func (a *App) Principal(rawKey string) *core.Principal {
	a.T.Helper()
	p, err := a.APIKeys.AuthenticateAPIKey(context.Background(), rawKey)
	if err != nil {
		a.T.Fatalf("AuthenticateAPIKey: %v", err)
	}
	return p
}
