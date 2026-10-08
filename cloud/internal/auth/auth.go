// Package auth：注册、登录（签发设备 API Key）、refresh 轮换与复用检测、登出、邮箱验证码、
// JWT 中间件与管理员引导（15_CLOUD_SERVICE.md §3.1）。
package auth

import (
	"context"
	"errors"
	"log/slog"
	"net/http"
	"time"

	"github.com/go-chi/chi/v5"
	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/redis/go-redis/v9"

	"forge-cloud/internal/config"
	"forge-cloud/internal/core"
	"forge-cloud/internal/httpx"
	"forge-cloud/internal/syssettings"
)

const (
	accessTTL     = 15 * time.Minute
	refreshTTL    = 30 * 24 * time.Hour
	refreshPrefix = "rt_"
	issuer        = "forge-cloud"
)

// Querier 是 *pgxpool.Pool 与 pgx.Tx 的公共子集。
type Querier interface {
	Exec(ctx context.Context, sql string, args ...any) (pgconn.CommandTag, error)
	Query(ctx context.Context, sql string, args ...any) (pgx.Rows, error)
	QueryRow(ctx context.Context, sql string, args ...any) pgx.Row
}

// Mailer 发送纯文本邮件（默认走 net/smtp；测试可替换）。
type Mailer func(ctx context.Context, to, subject, body string) error

type Service struct {
	db       *pgxpool.Pool
	rdb      *redis.Client
	cfg      *config.Config
	log      *slog.Logger
	settings *syssettings.Store
	mailer   Mailer
}

func New(db *pgxpool.Pool, rdb *redis.Client, cfg *config.Config, log *slog.Logger, settings *syssettings.Store) *Service {
	s := &Service{db: db, rdb: rdb, cfg: cfg, log: log, settings: settings}
	s.mailer = s.sendSMTP
	return s
}

// SetMailer 替换发信实现。
func (s *Service) SetMailer(m Mailer) { s.mailer = m }

// Settings 返回系统设置存储（users 等模块共用同一份缓存）。
func (s *Service) Settings() *syssettings.Store { return s.settings }

// MountPublic 挂 /auth/config、/auth/register、/auth/login、/auth/refresh、/auth/email-code、
// /auth/password/reset（相对 /api/v1，无需登录）。
func (s *Service) MountPublic(r chi.Router) {
	r.Get("/auth/config", s.handleConfig)
	r.Post("/auth/register", s.handleRegister)
	r.Post("/auth/login", s.handleLogin)
	r.Post("/auth/refresh", s.handleRefresh)
	r.Post("/auth/email-code", s.handleEmailCode)
	r.Post("/auth/password/reset", s.handlePasswordReset)
}

// MountUser 挂需要登录的 /auth/logout。
func (s *Service) MountUser(r chi.Router) {
	r.Post("/auth/logout", s.handleLogout)
}

// EnsureBootstrapAdmin：配置了 FORGE_CLOUD_ADMIN_EMAIL/PASSWORD 且库里没有任何管理员时创建
// （邮箱已存在则提升为 admin）。
func (s *Service) EnsureBootstrapAdmin(ctx context.Context) error {
	if s.cfg.AdminEmail == "" || s.cfg.AdminPassword == "" {
		return nil
	}
	var exists bool
	if err := s.db.QueryRow(ctx, `SELECT EXISTS (SELECT 1 FROM users WHERE role = 'admin')`).Scan(&exists); err != nil {
		return err
	}
	if exists {
		return nil
	}
	if err := s.CreateAdmin(ctx, s.cfg.AdminEmail, s.cfg.AdminPassword); err != nil {
		return err
	}
	s.log.Info("bootstrap admin ready", "email", s.cfg.AdminEmail)
	return nil
}

// CreateAdmin 创建管理员（邮箱已存在则提升为 admin 并重置密码，同时吊销其全部会话）；CLI `admin create` 用。
func (s *Service) CreateAdmin(ctx context.Context, email, password string) error {
	email, err := NormalizeEmail(email)
	if err != nil {
		return err
	}
	if err := ValidatePassword(password); err != nil {
		return err
	}
	hash, err := HashPassword(password)
	if err != nil {
		return err
	}
	groupID, err := s.settings.DefaultGroupID(ctx)
	if err != nil {
		return err
	}
	return pgx.BeginFunc(ctx, s.db, func(tx pgx.Tx) error {
		var id int64
		err := tx.QueryRow(ctx, `SELECT id FROM users WHERE email = $1 FOR UPDATE`, email).Scan(&id)
		if errors.Is(err, pgx.ErrNoRows) {
			_, err = CreateUser(ctx, tx, NewUser{Email: email, PasswordHash: hash, Role: "admin", GroupID: groupID, EmailVerified: true})
			return err
		}
		if err != nil {
			return err
		}
		if _, err := tx.Exec(ctx,
			`UPDATE users SET role = 'admin', status = 'active', password_hash = $2, updated_at = now() WHERE id = $1`,
			id, hash); err != nil {
			return err
		}
		return RevokeUserSessions(ctx, tx, id, "")
	})
}

func (s *Service) handleConfig(w http.ResponseWriter, r *http.Request) {
	st, err := s.settings.Get(r.Context())
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	smtp := s.cfg.SMTP.Enabled()
	httpx.WriteJSON(w, http.StatusOK, map[string]any{
		"registrationMode":   st.RegistrationMode,
		"requireEmailVerify": st.RequireEmailVerify && smtp,
		"smtpEnabled":        smtp,
		"siteName":           st.SiteName,
		"currency":           st.Currency,
	})
}

func errUnauthorized() error { return core.Unauthorized("UNAUTHORIZED", "未登录或登录已过期") }

func errUserDisabled() error { return core.Forbidden("USER_DISABLED", "账号已被禁用") }

func errTooManyAttempts(retryAfter time.Duration) error {
	e := core.E(http.StatusTooManyRequests, "TOO_MANY_ATTEMPTS", "尝试次数过多，请稍后再试")
	e.RetryAfter = max(1, int((retryAfter+time.Second-1)/time.Second))
	return e
}
