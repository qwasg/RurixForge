package auth

import (
	"context"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5"

	"forge-cloud/internal/apikeys"
	"forge-cloud/internal/core"
)

// DeviceInfo 是登录/注册请求里的设备信息。
type DeviceInfo struct {
	ID         string `json:"id"`
	Name       string `json:"name"`
	Platform   string `json:"platform"`
	AppVersion string `json:"appVersion"`
}

func clip(s string, n int) string {
	s = strings.TrimSpace(s)
	if utf8.RuneCountInString(s) <= n {
		return s
	}
	return string([]rune(s)[:n])
}

func (d DeviceInfo) normalized() DeviceInfo {
	return DeviceInfo{ID: clip(d.ID, 128), Name: clip(d.Name, 128), Platform: clip(d.Platform, 32), AppVersion: clip(d.AppVersion, 32)}
}

// DeviceKey 是登录时签发的设备 Key（明文仅此一次）。
type DeviceKey struct {
	ID     int64  `json:"id"`
	Key    string `json:"key"`
	Prefix string `json:"prefix"`
}

type session struct {
	ID               string
	RefreshToken     string
	RefreshExpiresAt time.Time
	DeviceKey        *DeviceKey
}

// createSession 在事务里新建登录会话：同一用户同一 device.id 的旧会话与旧设备 Key 先吊销。
func createSession(ctx context.Context, tx pgx.Tx, userID int64, dev DeviceInfo, ip string, issueKey bool) (*session, error) {
	if dev.ID != "" {
		if _, err := tx.Exec(ctx,
			`WITH s AS (
				UPDATE refresh_sessions SET revoked_at = now()
				WHERE user_id = $1 AND device_id = $2 AND revoked_at IS NULL RETURNING id
			)
			UPDATE api_keys SET status = 'revoked', revoked_at = now()
			WHERE kind = 'device' AND status = 'active' AND session_id IN (SELECT id FROM s)`,
			userID, dev.ID); err != nil {
			return nil, err
		}
	}
	out := &session{
		ID:               uuid.NewString(),
		RefreshToken:     newRefreshToken(),
		RefreshExpiresAt: time.Now().UTC().Add(refreshTTL).Truncate(time.Second),
	}
	if _, err := tx.Exec(ctx,
		`INSERT INTO refresh_sessions (id, user_id, device_id, device_name, platform, app_version, token_hash, ip, expires_at)
		 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)`,
		out.ID, userID, dev.ID, dev.Name, dev.Platform, dev.AppVersion, core.SHA256Hex(out.RefreshToken), ip, out.RefreshExpiresAt); err != nil {
		return nil, err
	}
	if issueKey {
		name := dev.Name
		if name == "" {
			name = "设备"
		}
		id, raw, prefix, err := apikeys.InsertDeviceKey(ctx, tx, userID, out.ID, name)
		if err != nil {
			return nil, err
		}
		out.DeviceKey = &DeviceKey{ID: id, Key: raw, Prefix: prefix}
	}
	return out, nil
}

// RevokeSession 吊销用户的一个登录会话及其设备 Key（幂等）；返回会话是否属于该用户。
func RevokeSession(ctx context.Context, q Querier, userID int64, sessionID string) (bool, error) {
	var n int
	err := q.QueryRow(ctx,
		`WITH s AS (
			UPDATE refresh_sessions SET revoked_at = COALESCE(revoked_at, now())
			WHERE id = $1 AND user_id = $2 RETURNING id
		), k AS (
			UPDATE api_keys SET status = 'revoked', revoked_at = now()
			WHERE kind = 'device' AND status = 'active' AND session_id IN (SELECT id FROM s) RETURNING 1
		)
		SELECT count(*) FROM s`, sessionID, userID).Scan(&n)
	return n > 0, err
}

// RevokeUserSessions 吊销用户全部会话（exceptSessionID 非空时保留该会话）及其余全部设备 Key。
func RevokeUserSessions(ctx context.Context, q Querier, userID int64, exceptSessionID string) error {
	_, err := q.Exec(ctx,
		`WITH s AS (
			UPDATE refresh_sessions SET revoked_at = now()
			WHERE user_id = $1 AND revoked_at IS NULL AND id <> $2 RETURNING id
		)
		UPDATE api_keys SET status = 'revoked', revoked_at = now()
		WHERE user_id = $1 AND kind = 'device' AND status = 'active' AND session_id IS DISTINCT FROM $2`,
		userID, exceptSessionID)
	return err
}

// Device 是登录设备（有效的 refresh 会话）。
type Device struct {
	ID         string    `json:"id"`
	DeviceID   string    `json:"deviceId"`
	DeviceName string    `json:"deviceName"`
	Platform   string    `json:"platform"`
	AppVersion string    `json:"appVersion"`
	IP         string    `json:"ip"`
	CreatedAt  time.Time `json:"createdAt"`
	LastSeenAt time.Time `json:"lastSeenAt"`
	ExpiresAt  time.Time `json:"expiresAt"`
	Current    bool      `json:"current"`
}

// ListDevices 列出用户未吊销、未过期的会话；currentSessionID 对应的条目 current=true。
func ListDevices(ctx context.Context, q Querier, userID int64, currentSessionID string) ([]Device, error) {
	rows, err := q.Query(ctx,
		`SELECT id, device_id, device_name, platform, app_version, ip, created_at, last_seen_at, expires_at
		 FROM refresh_sessions
		 WHERE user_id = $1 AND revoked_at IS NULL AND expires_at > now()
		 ORDER BY last_seen_at DESC, created_at DESC`, userID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := []Device{}
	for rows.Next() {
		var d Device
		if err := rows.Scan(&d.ID, &d.DeviceID, &d.DeviceName, &d.Platform, &d.AppVersion, &d.IP,
			&d.CreatedAt, &d.LastSeenAt, &d.ExpiresAt); err != nil {
			return nil, err
		}
		d.CreatedAt, d.LastSeenAt, d.ExpiresAt = d.CreatedAt.UTC(), d.LastSeenAt.UTC(), d.ExpiresAt.UTC()
		d.Current = currentSessionID != "" && d.ID == currentSessionID
		out = append(out, d)
	}
	return out, rows.Err()
}
