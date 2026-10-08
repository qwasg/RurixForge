// Package server 组装各模块并挂路由（15_CLOUD_SERVICE.md §3–§7）。
package server

import (
	"context"
	"log/slog"
	"net/http"
	"time"

	"github.com/go-chi/chi/v5"
	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/redis/go-redis/v9"

	"forge-cloud/internal/accounts"
	"forge-cloud/internal/admin"
	"forge-cloud/internal/apikeys"
	"forge-cloud/internal/auth"
	"forge-cloud/internal/billing"
	"forge-cloud/internal/config"
	"forge-cloud/internal/gateway"
	"forge-cloud/internal/httpx"
	"forge-cloud/internal/syssettings"
	"forge-cloud/internal/userdata"
	"forge-cloud/internal/users"
	"forge-cloud/internal/vault"
	"forge-cloud/internal/webui"
)

// Server 持有全部模块。
type Server struct {
	Config   *config.Config
	Log      *slog.Logger
	DB       *pgxpool.Pool
	Redis    *redis.Client
	Settings *syssettings.Store
	Vault    *vault.Vault

	Auth     *auth.Service
	Users    *users.Service
	APIKeys  *apikeys.Service
	UserData *userdata.Service
	Billing  *billing.Service
	Admin    *admin.Service
	Accounts *accounts.Service
	Gateway  *gateway.Service

	handler http.Handler
}

// New 构造全部模块并组装路由。
func New(cfg *config.Config, log *slog.Logger, pool *pgxpool.Pool, rdb *redis.Client) (*Server, error) {
	v, err := vault.New(cfg.MasterKey)
	if err != nil {
		return nil, err
	}
	s := &Server{Config: cfg, Log: log, DB: pool, Redis: rdb, Vault: v}
	s.Settings = syssettings.New(pool)
	s.Auth = auth.New(pool, rdb, cfg, log.With("mod", "auth"), s.Settings)
	s.Users = users.New(pool, cfg, log.With("mod", "users"), s.Auth)
	s.APIKeys = apikeys.New(pool, log.With("mod", "apikeys"), s.Settings)
	s.UserData = userdata.New(pool, log.With("mod", "userdata"))
	s.Billing = billing.New(pool, rdb, log.With("mod", "billing"), s.Settings)
	s.Admin = admin.New(admin.Deps{
		DB: pool, Redis: rdb, Config: cfg, Log: log.With("mod", "admin"),
		Settings: s.Settings, Auth: s.Auth, Billing: s.Billing,
	})
	s.Accounts = accounts.New(pool, rdb, cfg, log.With("mod", "accounts"), v, s.Settings)
	s.Gateway = gateway.New(gateway.Deps{
		DB: pool, Redis: rdb, Config: cfg, Log: log.With("mod", "gateway"), Settings: s.Settings,
		Accounts: s.Accounts, Principals: s.APIKeys, Biller: s.Billing,
	})
	s.handler = s.routes()
	return s, nil
}

func (s *Server) Handler() http.Handler { return s.handler }

// StartWorkers 启动后台任务（token 刷新等）。
func (s *Server) StartWorkers(ctx context.Context) {
	s.Accounts.StartWorkers(ctx)
}

func (s *Server) routes() http.Handler {
	r := chi.NewRouter()
	r.Use(httpx.RequestID, httpx.Recoverer(s.Log), httpx.AccessLog(s.Log))

	r.Get("/healthz", s.health)
	r.Get("/", func(w http.ResponseWriter, r *http.Request) {
		http.Redirect(w, r, "/admin/", http.StatusFound)
	})

	r.Route("/api/v1", func(r chi.Router) {
		s.Auth.MountPublic(r)
		s.Billing.MountPublic(r)
		r.Group(func(r chi.Router) {
			r.Use(s.Auth.RequireUser)
			s.Auth.MountUser(r)
			s.Users.Mount(r)
			s.APIKeys.Mount(r)
			s.Billing.MountUser(r)
			s.UserData.Mount(r)
			s.Gateway.MountCatalog(r)
		})
	})

	r.Route("/api/admin", func(r chi.Router) {
		r.Use(s.Auth.RequireUser, s.Auth.RequireAdmin)
		s.Admin.Mount(r)
		s.Accounts.MountAdmin(r)
	})

	r.Route("/v1", s.Gateway.MountGateway)

	ui := webui.Handler(s.Config, s.Log)
	r.Get("/admin", func(w http.ResponseWriter, r *http.Request) {
		http.Redirect(w, r, "/admin/", http.StatusMovedPermanently)
	})
	r.Handle("/admin/*", ui)
	return r
}

func (s *Server) health(w http.ResponseWriter, r *http.Request) {
	ctx, cancel := context.WithTimeout(r.Context(), 2*time.Second)
	defer cancel()
	out := map[string]any{"status": "ok", "db": "ok", "redis": "ok", "version": config.Version}
	status := http.StatusOK
	if err := s.DB.Ping(ctx); err != nil {
		out["db"] = "error"
		out["status"] = "degraded"
		status = http.StatusServiceUnavailable
	}
	if err := s.Redis.Ping(ctx).Err(); err != nil {
		out["redis"] = "error"
		out["status"] = "degraded"
		status = http.StatusServiceUnavailable
	}
	httpx.WriteJSON(w, status, out)
}
