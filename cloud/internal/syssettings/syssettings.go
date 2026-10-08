// Package syssettings 管理系统设置（system_settings 表 key=global 的一行 JSON），带 5 秒缓存。
package syssettings

import (
	"context"
	"encoding/json"
	"errors"
	"strings"
	"sync"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"

	"forge-cloud/internal/core"
)

const rowKey = "global"

// Settings 对应管理 API `/api/admin/settings`（smtpEnabled 为只读派生字段，不在此处）。
type Settings struct {
	SiteName           string `json:"siteName"`
	Currency           string `json:"currency"`
	RegistrationMode   string `json:"registrationMode"` // open|invite|closed
	RequireEmailVerify bool   `json:"requireEmailVerify"`
	SignupBonusMicros  int64  `json:"signupBonusMicros"`
	DefaultGroupID     int64  `json:"defaultGroupId"` // 0 = groups.is_default
	DefaultModel       string `json:"defaultModel"`
	MaxFailoverRetries int    `json:"maxFailoverRetries"`
	StickyTTLSeconds   int    `json:"stickyTtlSeconds"`
	CodexInstructions  string `json:"codexInstructions"`
}

func Defaults() Settings {
	return Settings{
		SiteName:           "RurixForge Cloud",
		Currency:           "USD",
		RegistrationMode:   "open",
		RequireEmailVerify: false,
		SignupBonusMicros:  0,
		DefaultGroupID:     0,
		DefaultModel:       "",
		MaxFailoverRetries: 3,
		StickyTTLSeconds:   3600,
		CodexInstructions:  "",
	}
}

// Validate 校验取值范围。
func (s Settings) Validate() error {
	switch s.RegistrationMode {
	case "open", "invite", "closed":
	default:
		return core.BadRequest("INVALID_SETTINGS", "registrationMode 只能是 open/invite/closed")
	}
	if strings.TrimSpace(s.Currency) == "" || len(s.Currency) > 8 {
		return core.BadRequest("INVALID_SETTINGS", "currency 不能为空且不超过 8 个字符")
	}
	if s.SignupBonusMicros < 0 {
		return core.BadRequest("INVALID_SETTINGS", "signupBonusMicros 不能为负")
	}
	if s.MaxFailoverRetries < 0 || s.MaxFailoverRetries > 10 {
		return core.BadRequest("INVALID_SETTINGS", "maxFailoverRetries 范围 0–10")
	}
	if s.StickyTTLSeconds < 60 || s.StickyTTLSeconds > 86400 {
		return core.BadRequest("INVALID_SETTINGS", "stickyTtlSeconds 范围 60–86400")
	}
	if len(s.CodexInstructions) > 200_000 {
		return core.BadRequest("INVALID_SETTINGS", "codexInstructions 过长")
	}
	return nil
}

type Store struct {
	db     *pgxpool.Pool
	mu     sync.Mutex
	cached *Settings
	at     time.Time
}

func New(db *pgxpool.Pool) *Store { return &Store{db: db} }

// Get 返回当前设置（默认值 + 库内覆盖），缓存 5 秒。
func (s *Store) Get(ctx context.Context) (Settings, error) {
	s.mu.Lock()
	if s.cached != nil && time.Since(s.at) < 5*time.Second {
		v := *s.cached
		s.mu.Unlock()
		return v, nil
	}
	s.mu.Unlock()

	out := Defaults()
	var raw []byte
	err := s.db.QueryRow(ctx, `SELECT value FROM system_settings WHERE key = $1`, rowKey).Scan(&raw)
	if err != nil && !errors.Is(err, pgx.ErrNoRows) {
		return out, err
	}
	if len(raw) > 0 {
		if err := json.Unmarshal(raw, &out); err != nil {
			return Defaults(), err
		}
	}
	s.mu.Lock()
	s.cached = &out
	s.at = time.Now()
	s.mu.Unlock()
	return out, nil
}

// Update 以 JSON 局部补丁更新设置（未知字段忽略），校验后落库并返回新值。
func (s *Store) Update(ctx context.Context, patch json.RawMessage) (Settings, error) {
	cur, err := s.Get(ctx)
	if err != nil {
		return cur, err
	}
	next := cur
	if err := json.Unmarshal(patch, &next); err != nil {
		return cur, core.BadRequest("INVALID_JSON", "设置补丁不是合法 JSON")
	}
	if err := next.Validate(); err != nil {
		return cur, err
	}
	b, _ := json.Marshal(next)
	_, err = s.db.Exec(ctx, `INSERT INTO system_settings (key, value, updated_at) VALUES ($1, $2, now())
		ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value, updated_at = now()`, rowKey, b)
	if err != nil {
		return cur, err
	}
	s.Invalidate()
	return next, nil
}

func (s *Store) Invalidate() {
	s.mu.Lock()
	s.cached = nil
	s.mu.Unlock()
}

// DefaultGroupID 返回新用户默认分组：设置里指定的，否则 groups.is_default 那一行。
func (s *Store) DefaultGroupID(ctx context.Context) (int64, error) {
	st, err := s.Get(ctx)
	if err != nil {
		return 0, err
	}
	if st.DefaultGroupID > 0 {
		return st.DefaultGroupID, nil
	}
	var id int64
	err = s.db.QueryRow(ctx, `SELECT id FROM groups WHERE is_default LIMIT 1`).Scan(&id)
	if errors.Is(err, pgx.ErrNoRows) {
		return 0, nil
	}
	return id, err
}
