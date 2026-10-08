package auth

import (
	"errors"
	"net/http"

	"github.com/jackc/pgx/v5"

	"forge-cloud/internal/core"
	"forge-cloud/internal/httpx"
)

// RequireUser 校验 Bearer JWT，并用一条查询确认用户未禁用、会话未吊销且未过期，把 *core.Claims
// 放进 context（role/email 取库内最新值）。失败 401 UNAUTHORIZED，用户禁用 403 USER_DISABLED。
func (s *Service) RequireUser(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		raw, ok := bearerToken(r)
		if !ok {
			httpx.WriteError(w, errUnauthorized())
			return
		}
		c, uid, err := s.parseAccess(raw)
		if err != nil {
			httpx.WriteError(w, errUnauthorized())
			return
		}
		var (
			status    string
			role      string
			email     string
			sessionOK bool
		)
		err = s.db.QueryRow(r.Context(),
			`SELECT u.status, u.role, u.email,
			        COALESCE((SELECT rs.revoked_at IS NULL AND rs.expires_at > now()
			                  FROM refresh_sessions rs WHERE rs.id = $2 AND rs.user_id = u.id), false)
			 FROM users u WHERE u.id = $1`, uid, c.SID).Scan(&status, &role, &email, &sessionOK)
		if errors.Is(err, pgx.ErrNoRows) {
			httpx.WriteError(w, errUnauthorized())
			return
		}
		if err != nil {
			httpx.WriteError(w, err)
			return
		}
		if !sessionOK {
			httpx.WriteError(w, errUnauthorized())
			return
		}
		if status != "active" {
			httpx.WriteError(w, errUserDisabled())
			return
		}
		ctx := core.WithClaims(r.Context(), &core.Claims{UserID: uid, SessionID: c.SID, Role: role, Email: email})
		next.ServeHTTP(w, r.WithContext(ctx))
	})
}

// RequireAdmin 要求 claims.Role == "admin"，否则 403 FORBIDDEN（须挂在 RequireUser 之后）。
func (s *Service) RequireAdmin(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		c, ok := core.ClaimsFrom(r.Context())
		if !ok || !c.IsAdmin() {
			httpx.WriteError(w, core.Forbidden("FORBIDDEN", "需要管理员权限"))
			return
		}
		next.ServeHTTP(w, r)
	})
}
