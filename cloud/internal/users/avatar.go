package users

import (
	"bytes"
	"encoding/base64"
	"net/http"
	"strings"

	"github.com/jackc/pgx/v5"

	"forge-cloud/internal/core"
	"forge-cloud/internal/httpx"
)

const (
	maxAvatarBytes = 512 << 10
	// base64 膨胀约 4/3，再留出 JSON 包装的余量。
	maxAvatarBody = 1 << 20
)

func errAvatarInvalid() error {
	return core.BadRequest("AVATAR_INVALID", "头像只支持 png/jpeg/webp/gif 图片")
}

func errAvatarTooLarge() error {
	return core.E(http.StatusRequestEntityTooLarge, "AVATAR_TOO_LARGE", "头像不能超过 512 KiB")
}

// parseDataURL 解析 data:<mime>;base64,<data>；声明的 mime 不可信，真实类型由 sniffImage 判定。
func parseDataURL(s string) ([]byte, error) {
	s = strings.TrimSpace(s)
	if !strings.HasPrefix(s, "data:") {
		return nil, errAvatarInvalid()
	}
	comma := strings.IndexByte(s, ',')
	if comma < 0 || !strings.HasSuffix(strings.ToLower(s[:comma]), ";base64") {
		return nil, errAvatarInvalid()
	}
	payload := s[comma+1:]
	if base64.StdEncoding.DecodedLen(len(payload)) > maxAvatarBytes+3 {
		return nil, errAvatarTooLarge()
	}
	data, err := base64.StdEncoding.DecodeString(payload)
	if err != nil {
		if data, err = base64.RawStdEncoding.DecodeString(payload); err != nil {
			return nil, errAvatarInvalid()
		}
	}
	return data, nil
}

// sniffImage 按魔数识别图片类型，不认识返回空串。
func sniffImage(b []byte) string {
	switch {
	case bytes.HasPrefix(b, []byte("\x89PNG\r\n\x1a\n")):
		return "image/png"
	case bytes.HasPrefix(b, []byte{0xFF, 0xD8, 0xFF}):
		return "image/jpeg"
	case bytes.HasPrefix(b, []byte("GIF87a")), bytes.HasPrefix(b, []byte("GIF89a")):
		return "image/gif"
	case len(b) >= 12 && string(b[:4]) == "RIFF" && string(b[8:12]) == "WEBP":
		return "image/webp"
	}
	return ""
}

func (s *Service) handleGetAvatar(w http.ResponseWriter, r *http.Request) {
	c, err := claims(r)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	var (
		ct   string
		data []byte
	)
	err = s.db.QueryRow(r.Context(), `SELECT content_type, data FROM user_avatars WHERE user_id = $1`, c.UserID).Scan(&ct, &data)
	if isNoRows(err) {
		httpx.WriteError(w, core.NotFound("AVATAR_NOT_FOUND", "尚未设置头像"))
		return
	}
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	w.Header().Set("Content-Type", ct)
	w.Header().Set("Cache-Control", "private, max-age=300")
	w.Header().Set("X-Content-Type-Options", "nosniff")
	w.WriteHeader(http.StatusOK)
	_, _ = w.Write(data)
}

func (s *Service) handlePutAvatar(w http.ResponseWriter, r *http.Request) {
	c, err := claims(r)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	var req struct {
		DataURL string `json:"dataUrl"`
	}
	if err := httpx.DecodeJSON(r, &req, maxAvatarBody); err != nil {
		if e := core.AsError(err); e != nil && e.Status == http.StatusRequestEntityTooLarge {
			err = errAvatarTooLarge()
		}
		httpx.WriteError(w, err)
		return
	}
	data, err := parseDataURL(req.DataURL)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	if len(data) > maxAvatarBytes {
		httpx.WriteError(w, errAvatarTooLarge())
		return
	}
	ct := sniffImage(data)
	if ct == "" {
		httpx.WriteError(w, errAvatarInvalid())
		return
	}
	ctx := r.Context()
	err = pgx.BeginFunc(ctx, s.db, func(tx pgx.Tx) error {
		if _, err := tx.Exec(ctx,
			`INSERT INTO user_avatars (user_id, content_type, data, updated_at) VALUES ($1, $2, $3, now())
			 ON CONFLICT (user_id) DO UPDATE SET content_type = EXCLUDED.content_type, data = EXCLUDED.data, updated_at = now()`,
			c.UserID, ct, data); err != nil {
			return err
		}
		_, err := tx.Exec(ctx, `UPDATE users SET avatar_updated_at = now(), updated_at = now() WHERE id = $1`, c.UserID)
		return err
	})
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	s.writeUser(w, r, c.UserID)
}

func (s *Service) handleDeleteAvatar(w http.ResponseWriter, r *http.Request) {
	c, err := claims(r)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	ctx := r.Context()
	err = pgx.BeginFunc(ctx, s.db, func(tx pgx.Tx) error {
		if _, err := tx.Exec(ctx, `DELETE FROM user_avatars WHERE user_id = $1`, c.UserID); err != nil {
			return err
		}
		_, err := tx.Exec(ctx, `UPDATE users SET avatar_updated_at = NULL, updated_at = now() WHERE id = $1`, c.UserID)
		return err
	})
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	s.writeUser(w, r, c.UserID)
}
