package auth

import (
	"context"
	"errors"
	"net/http"
	"strings"
	"time"

	"github.com/jackc/pgx/v5"

	"forge-cloud/internal/core"
	"forge-cloud/internal/httpx"
)

const (
	loginLimit  = 10
	loginWindow = 15 * time.Minute
)

// LoginResponse 对应 §3.1 LoginResponse。
type LoginResponse struct {
	TokenPair
	User      User       `json:"user"`
	DeviceKey *DeviceKey `json:"deviceKey"`
}

type loginRequest struct {
	Email          string     `json:"email"`
	Password       string     `json:"password"`
	Device         DeviceInfo `json:"device"`
	IssueDeviceKey *bool      `json:"issueDeviceKey"`
}

func issueKeyFlag(v *bool) bool { return v == nil || *v }

func (s *Service) buildLoginResponse(ctx context.Context, userID int64, sess *session) (*LoginResponse, error) {
	u, err := LoadUser(ctx, s.db, userID)
	if err != nil {
		return nil, err
	}
	access, exp, err := s.issueAccess(userID, sess.ID, u.Role, u.Email)
	if err != nil {
		return nil, err
	}
	return &LoginResponse{
		TokenPair: TokenPair{AccessToken: access, AccessExpiresAt: exp, RefreshToken: sess.RefreshToken, RefreshExpiresAt: sess.RefreshExpiresAt},
		User:      u,
		DeviceKey: sess.DeviceKey,
	}, nil
}

// loginBlocked：同 IP+邮箱 15 分钟内失败 10 次后拒绝；Redis 故障时放行。
func (s *Service) loginBlocked(ctx context.Context, key string) error {
	n, err := s.rdb.Get(ctx, key).Int()
	if err != nil {
		return nil
	}
	if n >= loginLimit {
		ttl, _ := s.rdb.TTL(ctx, key).Result()
		if ttl <= 0 {
			ttl = loginWindow
		}
		return errTooManyAttempts(ttl)
	}
	return nil
}

func (s *Service) loginFailed(ctx context.Context, key string) {
	n, err := s.rdb.Incr(ctx, key).Result()
	if err != nil {
		s.log.Warn("login rate limit unavailable", "err", err)
		return
	}
	if n == 1 {
		_ = s.rdb.Expire(ctx, key, loginWindow).Err()
	}
}

func (s *Service) handleLogin(w http.ResponseWriter, r *http.Request) {
	var req loginRequest
	if err := httpx.DecodeJSON(r, &req, 0); err != nil {
		httpx.WriteError(w, err)
		return
	}
	ctx := r.Context()
	email := strings.ToLower(strings.TrimSpace(req.Email))
	ip := httpx.ClientIP(r, s.cfg.TrustProxy)
	rlKey := "rl:login:" + ip + ":" + email
	if err := s.loginBlocked(ctx, rlKey); err != nil {
		httpx.WriteError(w, err)
		return
	}

	var (
		userID int64
		hash   string
		status string
	)
	err := s.db.QueryRow(ctx, `SELECT id, password_hash, status FROM users WHERE email = $1`, email).Scan(&userID, &hash, &status)
	if errors.Is(err, pgx.ErrNoRows) {
		burnPasswordCheck(req.Password)
		s.loginFailed(ctx, rlKey)
		httpx.WriteError(w, core.Unauthorized("INVALID_CREDENTIALS", "邮箱或密码错误"))
		return
	}
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	if !VerifyPassword(req.Password, hash) {
		s.loginFailed(ctx, rlKey)
		httpx.WriteError(w, core.Unauthorized("INVALID_CREDENTIALS", "邮箱或密码错误"))
		return
	}
	_ = s.rdb.Del(ctx, rlKey).Err()
	if status != "active" {
		httpx.WriteError(w, errUserDisabled())
		return
	}

	var sess *session
	err = pgx.BeginFunc(ctx, s.db, func(tx pgx.Tx) error {
		var err error
		if sess, err = createSession(ctx, tx, userID, req.Device.normalized(), ip, issueKeyFlag(req.IssueDeviceKey)); err != nil {
			return err
		}
		_, err = tx.Exec(ctx, `UPDATE users SET last_login_at = now() WHERE id = $1`, userID)
		return err
	})
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	resp, err := s.buildLoginResponse(ctx, userID, sess)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	httpx.WriteJSON(w, http.StatusOK, resp)
}

func errRefreshInvalid() error {
	return core.Unauthorized("REFRESH_INVALID", "登录已失效，请重新登录")
}

// handleRefresh 轮换 refresh token：旧哈希移入 prev_token_hash；出示某会话的 prev_token_hash
// 视为复用，吊销该会话及其设备 Key 后返回 REFRESH_REUSED。每次轮换把会话有效期顺延 30 天。
func (s *Service) handleRefresh(w http.ResponseWriter, r *http.Request) {
	var req struct {
		RefreshToken string `json:"refreshToken"`
	}
	if err := httpx.DecodeJSON(r, &req, 0); err != nil {
		httpx.WriteError(w, err)
		return
	}
	tok := strings.TrimSpace(req.RefreshToken)
	if !strings.HasPrefix(tok, refreshPrefix) || len(tok) > 128 {
		httpx.WriteError(w, errRefreshInvalid())
		return
	}
	ctx := r.Context()
	hash := core.SHA256Hex(tok)
	ip := httpx.ClientIP(r, s.cfg.TrustProxy)

	var (
		reused  bool
		userID  int64
		sid     string
		role    string
		email   string
		newTok  string
		expires time.Time
	)
	err := pgx.BeginFunc(ctx, s.db, func(tx pgx.Tx) error {
		var (
			status    string
			expiresAt time.Time
			revokedAt *time.Time
		)
		err := tx.QueryRow(ctx,
			`SELECT s.id, s.user_id, s.expires_at, s.revoked_at, u.role, u.email, u.status
			 FROM refresh_sessions s JOIN users u ON u.id = s.user_id
			 WHERE s.token_hash = $1 FOR UPDATE OF s`, hash).
			Scan(&sid, &userID, &expiresAt, &revokedAt, &role, &email, &status)
		if errors.Is(err, pgx.ErrNoRows) {
			var prevSID string
			var prevUser int64
			err := tx.QueryRow(ctx,
				`SELECT id, user_id FROM refresh_sessions WHERE prev_token_hash = $1 LIMIT 1 FOR UPDATE`, hash).
				Scan(&prevSID, &prevUser)
			if errors.Is(err, pgx.ErrNoRows) {
				return errRefreshInvalid()
			}
			if err != nil {
				return err
			}
			if _, err := RevokeSession(ctx, tx, prevUser, prevSID); err != nil {
				return err
			}
			reused = true
			return nil
		}
		if err != nil {
			return err
		}
		if revokedAt != nil || !expiresAt.After(time.Now()) {
			return errRefreshInvalid()
		}
		if status != "active" {
			return errUserDisabled()
		}
		newTok = newRefreshToken()
		expires = time.Now().UTC().Add(refreshTTL).Truncate(time.Second)
		_, err = tx.Exec(ctx,
			`UPDATE refresh_sessions
			 SET prev_token_hash = token_hash, token_hash = $2, rotated_at = now(), last_seen_at = now(), ip = $3, expires_at = $4
			 WHERE id = $1`, sid, core.SHA256Hex(newTok), ip, expires)
		return err
	})
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	if reused {
		s.log.Warn("refresh token reuse detected; session revoked", "ip", ip)
		httpx.WriteError(w, core.Unauthorized("REFRESH_REUSED", "检测到登录凭据被重复使用，该设备已被登出"))
		return
	}
	access, accessExp, err := s.issueAccess(userID, sid, role, email)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	httpx.WriteJSON(w, http.StatusOK, TokenPair{
		AccessToken: access, AccessExpiresAt: accessExp, RefreshToken: newTok, RefreshExpiresAt: expires,
	})
}

func (s *Service) handleLogout(w http.ResponseWriter, r *http.Request) {
	c, ok := core.ClaimsFrom(r.Context())
	if !ok {
		httpx.WriteError(w, errUnauthorized())
		return
	}
	if _, err := RevokeSession(r.Context(), s.db, c.UserID, c.SessionID); err != nil {
		httpx.WriteError(w, err)
		return
	}
	httpx.OK(w)
}
