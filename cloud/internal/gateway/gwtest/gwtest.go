// Package gwtest 是 gateway / accounts 模块的 HTTP 测试夹具（独立库 + miniredis + 假 Principal/Biller）。
package gwtest

import (
	"bytes"
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/alicebob/miniredis/v2"
	"github.com/go-chi/chi/v5"
	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/redis/go-redis/v9"

	"forge-cloud/internal/accounts"
	"forge-cloud/internal/auth"
	"forge-cloud/internal/config"
	"forge-cloud/internal/core"
	"forge-cloud/internal/devupstream"
	"forge-cloud/internal/gateway"
	"forge-cloud/internal/httpx"
	"forge-cloud/internal/syssettings"
	"forge-cloud/internal/testutil"
	"forge-cloud/internal/vault"
)

const TestAPIKey = "sk-rf-test-gateway-key-000000000000000000"

type App struct {
	T testing.TB

	DB       *pgxpool.Pool
	Redis    *redis.Client
	MR       *miniredis.Miniredis
	Config   *config.Config
	Settings *syssettings.Store
	Vault    *vault.Vault
	Auth     *auth.Service
	Accounts *accounts.Service
	Gateway  *gateway.Service

	Principals *FakePrincipal
	Biller     *FakeBiller

	Upstream *devupstream.Server
	UpstreamURL string

	Handler http.Handler
}

func New(t testing.TB, opts ...func(*config.Config)) *App {
	t.Helper()
	pool := testutil.NewDB(t)
	rdb, mr := testutil.NewRedis(t)
	cfg := testutil.Config()
	for _, o := range opts {
		o(cfg)
	}
	log := testutil.Logger()
	v, err := vault.New(cfg.MasterKey)
	if err != nil {
		t.Fatalf("vault: %v", err)
	}
	a := &App{
		T: t, DB: pool, Redis: rdb, MR: mr, Config: cfg, Vault: v,
		Principals: &FakePrincipal{APIKey: TestAPIKey, P: DefaultPrincipal()},
		Biller:     &FakeBiller{},
	}
	a.Settings = syssettings.New(pool)
	a.Auth = auth.New(pool, rdb, cfg, log.With("mod", "auth"), a.Settings)
	a.Accounts = accounts.New(pool, rdb, cfg, log.With("mod", "accounts"), v, a.Settings)
	a.Gateway = gateway.New(gateway.Deps{
		DB: pool, Redis: rdb, Config: cfg, Log: log.With("mod", "gateway"),
		Settings: a.Settings, Accounts: a.Accounts,
		Principals: a.Principals, Biller: a.Biller,
	})
	a.Handler = a.routes()
	return a
}

func (a *App) routes() http.Handler {
	r := chi.NewRouter()
	r.Use(httpx.RequestID)
	r.Route("/api/v1", func(r chi.Router) {
		a.Auth.MountPublic(r)
		r.Group(func(r chi.Router) {
			r.Use(a.Auth.RequireUser)
			a.Gateway.MountCatalog(r)
		})
	})
	r.Route("/api/admin", func(r chi.Router) {
		r.Use(a.Auth.RequireUser, a.Auth.RequireAdmin)
		a.Accounts.MountAdmin(r)
	})
	r.Route("/v1", a.Gateway.MountGateway)
	return r
}

// MountFakeUpstream 把 Codex / OpenAI / OAuth 上游指到 devupstream httptest。
func (a *App) MountFakeUpstream(t testing.TB) *httptest.Server {
	t.Helper()
	a.Upstream = devupstream.New(testutil.Logger())
	srv := httptest.NewServer(a.Upstream)
	t.Cleanup(srv.Close)
	base := strings.TrimRight(srv.URL, "/")
	a.UpstreamURL = base
	a.Config.CodexBaseURL = base + "/codex"
	a.Config.ChatGPTBaseURL = base
	a.Config.OpenAIAuthURL = base
	return srv
}

func (a *App) OpenAIBase() string {
	if a.UpstreamURL == "" {
		a.T.Fatal("先调用 MountFakeUpstream")
	}
	return a.UpstreamURL + "/v1"
}

type Response struct {
	Status int
	Header http.Header
	Body   []byte
}

func (r *Response) Decode(t testing.TB, v any) {
	t.Helper()
	if err := json.Unmarshal(r.Body, v); err != nil {
		t.Fatalf("decode HTTP %d: %v\n%s", r.Status, err, r.Body)
	}
}

func (r *Response) GatewayCode() string {
	var v struct {
		Error struct {
			Code string `json:"code"`
		} `json:"error"`
	}
	_ = json.Unmarshal(r.Body, &v)
	return v.Error.Code
}

func (r *Response) Expect(t testing.TB, status int, gatewayCode string) *Response {
	t.Helper()
	if r.Status != status {
		t.Fatalf("HTTP %d want %d: %s", r.Status, status, r.Body)
	}
	if gatewayCode != "" && r.GatewayCode() != gatewayCode {
		t.Fatalf("error.code = %q want %q: %s", r.GatewayCode(), gatewayCode, r.Body)
	}
	return r
}

func (a *App) DoV1(method, path string, body any, headers map[string]string) *Response {
	a.T.Helper()
	return a.do(method, "/v1"+path, TestAPIKey, body, headers)
}

func (a *App) DoAdmin(method, path, adminToken string, body any) *Response {
	a.T.Helper()
	return a.do(method, "/api/admin"+path, adminToken, body, nil)
}

func (a *App) DoCatalog(userToken string) *Response {
	a.T.Helper()
	req := httptest.NewRequest(http.MethodGet, "/api/v1/models/catalog", nil)
	req.Header.Set("Authorization", "Bearer "+userToken)
	return a.serve(req)
}

func (a *App) do(method, path, token string, body any, headers map[string]string) *Response {
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
			a.T.Fatalf("marshal body: %v", err)
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
	for k, v := range headers {
		req.Header.Set(k, v)
	}
	return a.serve(req)
}

func (a *App) serve(req *http.Request) *Response {
	rec := httptest.NewRecorder()
	a.Handler.ServeHTTP(rec, req)
	return &Response{Status: rec.Code, Header: rec.Header(), Body: rec.Body.Bytes()}
}

func (a *App) Exec(sql string, args ...any) {
	a.T.Helper()
	if _, err := a.DB.Exec(context.Background(), sql, args...); err != nil {
		a.T.Fatalf("sql: %v\n%s", err, sql)
	}
}

func (a *App) Int64(sql string, args ...any) int64 {
	a.T.Helper()
	var v int64
	if err := a.DB.QueryRow(context.Background(), sql, args...).Scan(&v); err != nil {
		a.T.Fatalf("sql: %v", err)
	}
	return v
}

func (a *App) SeedModel(m core.Model) {
	a.T.Helper()
	if m.Platform == "" {
		m.Platform = accounts.PlatformOpenAI
	}
	m.Enabled = true
	if _, err := a.Accounts.CreateModel(context.Background(), m); err != nil {
		a.T.Fatalf("CreateModel: %v", err)
	}
}

func (a *App) SeedOpenAIKeyAccount(name, apiKey string, groupID int64, opts ...func(*accounts.APIKeyAccountParams)) int64 {
	a.T.Helper()
	p := accounts.APIKeyAccountParams{
		Name: name, Platform: accounts.PlatformOpenAI, BaseURL: a.OpenAIBase(), APIKey: apiKey,
		SupportsResponses: true, Priority: 10, Weight: 1, GroupIDs: []int64{groupID},
	}
	for _, o := range opts {
		o(&p)
	}
	acc, _, err := a.Accounts.CreateAPIKeyAccount(context.Background(), p)
	if err != nil {
		a.T.Fatalf("CreateAPIKeyAccount: %v", err)
	}
	return acc.ID
}

func (a *App) SeedCodexAccount(name, accountID, access, refresh string, groupID int64) int64 {
	a.T.Helper()
	acc, _, err := a.Accounts.UpsertOAuthAccount(context.Background(), accounts.OAuthTokens{
		AccessToken: access, RefreshToken: refresh, AccountID: accountID,
	}, accounts.ImportOptions{Name: name, GroupIDs: []int64{groupID}, Priority: 10})
	if err != nil {
		a.T.Fatalf("UpsertOAuthAccount: %v", err)
	}
	return acc.ID
}

func (a *App) RegisterUser(email, password string) string {
	a.T.Helper()
	const pass = "password-123"
	if password == "" {
		password = pass
	}
	res := a.do(http.MethodPost, "/api/v1/auth/register", "", map[string]any{
		"email": email, "password": password,
		"device": map[string]any{"id": "dev-" + core.RandomString(6), "name": "PC", "platform": "windows", "appVersion": "0.1"},
	}, nil)
	var lr struct {
		AccessToken string `json:"accessToken"`
	}
	res.Decode(a.T, &lr)
	if lr.AccessToken == "" {
		a.T.Fatalf("register failed: %s", res.Body)
	}
	return lr.AccessToken
}

func (a *App) NewAdminToken() string {
	a.T.Helper()
	email := "admin-" + core.RandomString(8) + "@example.com"
	const adminPass = "password-123"
	if err := a.Auth.CreateAdmin(context.Background(), email, adminPass); err != nil {
		a.T.Fatalf("CreateAdmin: %v", err)
	}
	res := a.do(http.MethodPost, "/api/v1/auth/login", "", map[string]any{
		"email": email, "password": adminPass, "device": map[string]any{
			"id": "admin", "name": "Admin", "platform": "windows", "appVersion": "0.1.0",
		},
		"issueDeviceKey": false,
	}, nil)
	var lr struct {
		AccessToken string `json:"accessToken"`
	}
	res.Decode(a.T, &lr)
	if lr.AccessToken == "" {
		a.T.Fatalf("admin login failed: %s", res.Body)
	}
	return lr.AccessToken
}
