package apikeys

import (
	"context"
	"errors"
	"net/http"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/jackc/pgx/v5"

	"forge-cloud/internal/core"
	"forge-cloud/internal/httpx"
)

// APIKey 是 Key 的对外形状（不含明文与哈希）。
type APIKey struct {
	ID          int64      `json:"id"`
	Name        string     `json:"name"`
	Kind        string     `json:"kind"`
	Prefix      string     `json:"prefix"`
	Status      string     `json:"status"`
	QuotaMicros int64      `json:"quotaMicros"`
	UsedMicros  int64      `json:"usedMicros"`
	ExpiresAt   *time.Time `json:"expiresAt"`
	LastUsedAt  *time.Time `json:"lastUsedAt"`
	CreatedAt   time.Time  `json:"createdAt"`
	DeviceName  string     `json:"deviceName,omitempty"`
}

const keySelect = `SELECT id, name, kind, key_prefix, status, quota_micros, used_micros, expires_at, last_used_at, created_at
	FROM api_keys`

func scanKey(row pgx.Row) (APIKey, error) {
	var k APIKey
	if err := row.Scan(&k.ID, &k.Name, &k.Kind, &k.Prefix, &k.Status, &k.QuotaMicros, &k.UsedMicros,
		&k.ExpiresAt, &k.LastUsedAt, &k.CreatedAt); err != nil {
		return k, err
	}
	k.ExpiresAt = utcPtr(k.ExpiresAt)
	k.LastUsedAt = utcPtr(k.LastUsedAt)
	k.CreatedAt = k.CreatedAt.UTC()
	if k.Kind == "device" {
		k.DeviceName = k.Name
	}
	return k, nil
}

// ListKeys 列出用户的 Key（用户 Key 与设备 Key）；includeRevoked=false 时只含未吊销的。
func ListKeys(ctx context.Context, q Querier, userID int64, includeRevoked bool) ([]APIKey, error) {
	sql := keySelect + ` WHERE user_id = $1`
	if !includeRevoked {
		sql += ` AND status = 'active'`
	}
	sql += ` ORDER BY status, id DESC LIMIT 200`
	rows, err := q.Query(ctx, sql, userID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := []APIKey{}
	for rows.Next() {
		k, err := scanKey(rows)
		if err != nil {
			return nil, err
		}
		out = append(out, k)
	}
	return out, rows.Err()
}

func claims(r *http.Request) (*core.Claims, error) {
	c, ok := core.ClaimsFrom(r.Context())
	if !ok {
		return nil, core.Unauthorized("UNAUTHORIZED", "未登录")
	}
	return c, nil
}

func (s *Service) handleList(w http.ResponseWriter, r *http.Request) {
	c, err := claims(r)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	items, err := ListKeys(r.Context(), s.db, c.UserID, false)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	httpx.WriteJSON(w, http.StatusOK, map[string]any{"items": items})
}

type createRequest struct {
	Name        string     `json:"name"`
	QuotaMicros int64      `json:"quotaMicros"`
	ExpiresAt   *time.Time `json:"expiresAt"`
}

func (s *Service) handleCreate(w http.ResponseWriter, r *http.Request) {
	c, err := claims(r)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	var req createRequest
	if err := httpx.DecodeJSON(r, &req, 0); err != nil {
		httpx.WriteError(w, err)
		return
	}
	name := strings.TrimSpace(req.Name)
	if name == "" {
		name = "API Key"
	}
	if utf8.RuneCountInString(name) > 64 {
		httpx.WriteError(w, core.BadRequest("INVALID_REQUEST", "名称不能超过 64 个字符"))
		return
	}
	if req.QuotaMicros < 0 {
		httpx.WriteError(w, core.BadRequest("INVALID_REQUEST", "quotaMicros 不能为负"))
		return
	}
	if req.ExpiresAt != nil && !req.ExpiresAt.After(time.Now()) {
		httpx.WriteError(w, core.BadRequest("INVALID_REQUEST", "过期时间必须晚于当前时间"))
		return
	}

	ctx := r.Context()
	var raw string
	var key APIKey
	err = pgx.BeginFunc(ctx, s.db, func(tx pgx.Tx) error {
		// 锁用户行，串行化同一用户的并发创建，保证上限判断准确。
		if _, err := tx.Exec(ctx, `SELECT 1 FROM users WHERE id = $1 FOR UPDATE`, c.UserID); err != nil {
			return err
		}
		var n int
		if err := tx.QueryRow(ctx,
			`SELECT count(*) FROM api_keys WHERE user_id = $1 AND kind = 'user' AND status = 'active'
			 AND (expires_at IS NULL OR expires_at > now())`, c.UserID).Scan(&n); err != nil {
			return err
		}
		if n >= MaxUserKeys {
			return core.BadRequest("API_KEY_LIMIT", "有效 API Key 数量已达上限（20 个）")
		}
		var hash, prefix string
		raw, hash, prefix = GenerateKey()
		var id int64
		if err := tx.QueryRow(ctx,
			`INSERT INTO api_keys (user_id, name, kind, key_hash, key_prefix, quota_micros, expires_at)
			 VALUES ($1, $2, 'user', $3, $4, $5, $6) RETURNING id`,
			c.UserID, name, hash, prefix, req.QuotaMicros, req.ExpiresAt).Scan(&id); err != nil {
			return err
		}
		var err error
		key, err = scanKey(tx.QueryRow(ctx, keySelect+` WHERE id = $1`, id))
		return err
	})
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	httpx.WriteJSON(w, http.StatusOK, map[string]any{"apiKey": key, "key": raw})
}

func (s *Service) handleDelete(w http.ResponseWriter, r *http.Request) {
	c, err := claims(r)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	id, err := httpx.PathInt64(r, "id")
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	ctx := r.Context()
	var exists bool
	err = s.db.QueryRow(ctx,
		`WITH upd AS (
			UPDATE api_keys SET status = 'revoked', revoked_at = now()
			WHERE id = $1 AND user_id = $2 AND status = 'active' RETURNING id
		)
		SELECT EXISTS (SELECT 1 FROM upd) OR EXISTS (SELECT 1 FROM api_keys WHERE id = $1 AND user_id = $2)`,
		id, c.UserID).Scan(&exists)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	if !exists {
		httpx.WriteError(w, core.NotFound("API_KEY_NOT_FOUND", "API Key 不存在"))
		return
	}
	httpx.OK(w)
}

var errNoRows = pgx.ErrNoRows

func isNoRows(err error) bool { return errors.Is(err, errNoRows) }
