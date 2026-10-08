// Package users：我的账号（/me、资料、密码、头像、登录设备）（15_CLOUD_SERVICE.md §3.2）。
package users

import (
	"errors"
	"log/slog"
	"net/http"

	"github.com/go-chi/chi/v5"
	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"

	"forge-cloud/internal/auth"
	"forge-cloud/internal/billing"
	"forge-cloud/internal/config"
	"forge-cloud/internal/core"
	"forge-cloud/internal/httpx"
)

type Service struct {
	db   *pgxpool.Pool
	cfg  *config.Config
	log  *slog.Logger
	auth *auth.Service
}

func New(db *pgxpool.Pool, cfg *config.Config, log *slog.Logger, authSvc *auth.Service) *Service {
	return &Service{db: db, cfg: cfg, log: log, auth: authSvc}
}

// Mount 挂 /me、/me/profile、/me/password、/me/avatar、/me/devices（已在 RequireUser 分组内）。
func (s *Service) Mount(r chi.Router) {
	r.Get("/me", s.handleMe)
	r.Patch("/me/profile", s.handleProfile)
	r.Post("/me/password", s.handlePassword)
	r.Get("/me/avatar", s.handleGetAvatar)
	r.Put("/me/avatar", s.handlePutAvatar)
	r.Delete("/me/avatar", s.handleDeleteAvatar)
	r.Get("/me/devices", s.handleDevices)
	r.Delete("/me/devices/{id}", s.handleDeleteDevice)
}

func claims(r *http.Request) (*core.Claims, error) {
	c, ok := core.ClaimsFrom(r.Context())
	if !ok {
		return nil, core.Unauthorized("UNAUTHORIZED", "未登录")
	}
	return c, nil
}

func (s *Service) writeUser(w http.ResponseWriter, r *http.Request, userID int64) {
	u, err := auth.LoadUser(r.Context(), s.db, userID)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	httpx.WriteJSON(w, http.StatusOK, u)
}

func (s *Service) handleMe(w http.ResponseWriter, r *http.Request) {
	c, err := claims(r)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	ctx := r.Context()
	u, err := auth.LoadUser(ctx, s.db, c.UserID)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	subs, err := billing.ListSubscriptions(ctx, s.db, c.UserID, true)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	st, err := s.auth.Settings().Get(ctx)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	httpx.WriteJSON(w, http.StatusOK, map[string]any{"user": u, "subscriptions": subs, "currency": st.Currency})
}

func (s *Service) handleProfile(w http.ResponseWriter, r *http.Request) {
	c, err := claims(r)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	var req struct {
		Nickname *string `json:"nickname"`
	}
	if err := httpx.DecodeJSON(r, &req, 0); err != nil {
		httpx.WriteError(w, err)
		return
	}
	if req.Nickname != nil {
		nick, err := auth.ValidateNickname(*req.Nickname)
		if err != nil {
			httpx.WriteError(w, err)
			return
		}
		if _, err := s.db.Exec(r.Context(),
			`UPDATE users SET nickname = $2, updated_at = now() WHERE id = $1`, c.UserID, nick); err != nil {
			httpx.WriteError(w, err)
			return
		}
	}
	s.writeUser(w, r, c.UserID)
}

// handlePassword 修改密码并吊销当前会话以外的全部会话与设备 Key。
// 原密码错误返回 400（不用 401，免得客户端误判为登录失效）。
func (s *Service) handlePassword(w http.ResponseWriter, r *http.Request) {
	c, err := claims(r)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	var req struct {
		OldPassword string `json:"oldPassword"`
		NewPassword string `json:"newPassword"`
	}
	if err := httpx.DecodeJSON(r, &req, 0); err != nil {
		httpx.WriteError(w, err)
		return
	}
	ctx := r.Context()
	var hash string
	if err := s.db.QueryRow(ctx, `SELECT password_hash FROM users WHERE id = $1`, c.UserID).Scan(&hash); err != nil {
		httpx.WriteError(w, err)
		return
	}
	if !auth.VerifyPassword(req.OldPassword, hash) {
		httpx.WriteError(w, core.BadRequest("INVALID_PASSWORD", "原密码错误"))
		return
	}
	if err := auth.ValidatePassword(req.NewPassword); err != nil {
		httpx.WriteError(w, err)
		return
	}
	newHash, err := auth.HashPassword(req.NewPassword)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	err = pgx.BeginFunc(ctx, s.db, func(tx pgx.Tx) error {
		if _, err := tx.Exec(ctx, `UPDATE users SET password_hash = $2, updated_at = now() WHERE id = $1`, c.UserID, newHash); err != nil {
			return err
		}
		return auth.RevokeUserSessions(ctx, tx, c.UserID, c.SessionID)
	})
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	httpx.OK(w)
}

func (s *Service) handleDevices(w http.ResponseWriter, r *http.Request) {
	c, err := claims(r)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	items, err := auth.ListDevices(r.Context(), s.db, c.UserID, c.SessionID)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	httpx.WriteJSON(w, http.StatusOK, map[string]any{"items": items})
}

func (s *Service) handleDeleteDevice(w http.ResponseWriter, r *http.Request) {
	c, err := claims(r)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	id := chi.URLParam(r, "id")
	if id == "" || len(id) > 64 {
		httpx.WriteError(w, core.NotFound("DEVICE_NOT_FOUND", "登录设备不存在"))
		return
	}
	ok, err := auth.RevokeSession(r.Context(), s.db, c.UserID, id)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	if !ok {
		httpx.WriteError(w, core.NotFound("DEVICE_NOT_FOUND", "登录设备不存在"))
		return
	}
	httpx.OK(w)
}

var errNoRows = pgx.ErrNoRows

func isNoRows(err error) bool { return errors.Is(err, errNoRows) }
