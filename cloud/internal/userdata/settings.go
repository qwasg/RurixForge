package userdata

import (
	"bytes"
	"encoding/json"
	"errors"
	"net/http"
	"regexp"
	"time"

	"github.com/go-chi/chi/v5"
	"github.com/jackc/pgx/v5"

	"forge-cloud/internal/core"
	"forge-cloud/internal/httpx"
)

const maxSettingBytes = 64 << 10

var namespaceRe = regexp.MustCompile(`^[a-z][a-z0-9_-]{0,31}$`)

type settingEntry struct {
	Value     json.RawMessage `json:"value"`
	Version   int64           `json:"version"`
	UpdatedAt *time.Time      `json:"updatedAt"`
}

func (s *Service) handleGetSettings(w http.ResponseWriter, r *http.Request) {
	c, err := claims(r)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	rows, err := s.db.Query(r.Context(),
		`SELECT namespace, value, version, updated_at FROM user_settings WHERE user_id = $1 ORDER BY namespace`, c.UserID)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	defer rows.Close()
	items := map[string]settingEntry{}
	for rows.Next() {
		var (
			ns string
			e  settingEntry
			at time.Time
		)
		if err := rows.Scan(&ns, &e.Value, &e.Version, &at); err != nil {
			httpx.WriteError(w, err)
			return
		}
		at = at.UTC()
		e.UpdatedAt = &at
		items[ns] = e
	}
	if err := rows.Err(); err != nil {
		httpx.WriteError(w, err)
		return
	}
	httpx.WriteJSON(w, http.StatusOK, map[string]any{"items": items})
}

type putSettingRequest struct {
	Value       json.RawMessage `json:"value"`
	BaseVersion int64           `json:"baseVersion"`
	Force       bool            `json:"force"`
}

// handlePutSetting 整块覆盖一个命名空间：baseVersion 与服务端版本不一致且未 force → 409，附 current。
// 不存在的命名空间版本视为 0。
func (s *Service) handlePutSetting(w http.ResponseWriter, r *http.Request) {
	c, err := claims(r)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	ns := chi.URLParam(r, "ns")
	if !namespaceRe.MatchString(ns) {
		httpx.WriteError(w, core.BadRequest("INVALID_NAMESPACE", "命名空间需匹配 ^[a-z][a-z0-9_-]{0,31}$"))
		return
	}
	var req putSettingRequest
	if err := httpx.DecodeJSON(r, &req, 0); err != nil {
		if e := core.AsError(err); e != nil && e.Status == http.StatusRequestEntityTooLarge {
			err = errSettingsTooLarge()
		}
		httpx.WriteError(w, err)
		return
	}
	value := bytes.TrimSpace(req.Value)
	if len(value) == 0 || bytes.Equal(value, []byte("null")) {
		httpx.WriteError(w, core.BadRequest("INVALID_REQUEST", "value 不能为空"))
		return
	}
	if len(value) > maxSettingBytes {
		httpx.WriteError(w, errSettingsTooLarge())
		return
	}

	ctx := r.Context()
	var out settingEntry
	err = pgx.BeginFunc(ctx, s.db, func(tx pgx.Tx) error {
		var (
			cur     json.RawMessage
			version int64
			at      time.Time
		)
		err := tx.QueryRow(ctx,
			`SELECT value, version, updated_at FROM user_settings WHERE user_id = $1 AND namespace = $2 FOR UPDATE`,
			c.UserID, ns).Scan(&cur, &version, &at)
		exists := err == nil
		if err != nil && !errors.Is(err, pgx.ErrNoRows) {
			return err
		}
		if !req.Force && req.BaseVersion != version {
			current := settingEntry{Value: json.RawMessage("null"), Version: 0}
			if exists {
				at = at.UTC()
				current = settingEntry{Value: cur, Version: version, UpdatedAt: &at}
			}
			e := core.Conflict("SETTINGS_VERSION_CONFLICT", "设置已在其它设备上修改")
			e.Extra = map[string]any{"current": current}
			return e
		}
		var newAt time.Time
		err = tx.QueryRow(ctx,
			`INSERT INTO user_settings (user_id, namespace, value, version, updated_at) VALUES ($1, $2, $3, 1, now())
			 ON CONFLICT (user_id, namespace) DO UPDATE
			 SET value = EXCLUDED.value, version = user_settings.version + 1, updated_at = now()
			 RETURNING value, version, updated_at`, c.UserID, ns, []byte(value)).Scan(&out.Value, &out.Version, &newAt)
		newAt = newAt.UTC()
		out.UpdatedAt = &newAt
		return err
	})
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	httpx.WriteJSON(w, http.StatusOK, map[string]any{
		"namespace": ns, "value": out.Value, "version": out.Version, "updatedAt": out.UpdatedAt,
	})
}

func errSettingsTooLarge() error {
	return core.E(http.StatusRequestEntityTooLarge, "SETTINGS_TOO_LARGE", "设置内容不能超过 64 KiB")
}
