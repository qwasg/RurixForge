// Package apikeys：平台 API Key（sk-rf-…）生成/哈希、用户 Key 管理接口，以及网关鉴权
// （实现 core.PrincipalResolver）（15_CLOUD_SERVICE.md §3.2、§4）。
//
// 本包不得 import auth（auth 会 import 本包签发设备 Key）。
package apikeys

import (
	"context"
	"log/slog"
	"sync"
	"time"

	"github.com/go-chi/chi/v5"
	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
	"github.com/jackc/pgx/v5/pgxpool"

	"forge-cloud/internal/core"
	"forge-cloud/internal/syssettings"
)

// KeyPrefix 是平台 Key 前缀。
const KeyPrefix = "sk-rf-"

const (
	keyRandLen = 40
	// DisplayPrefixLen 是展示前缀长度（sk-rf- 加 6 位）。
	DisplayPrefixLen = 12
	// MaxUserKeys 是每个用户同时有效的自建 Key 上限。
	MaxUserKeys = 20

	touchInterval = time.Minute
	touchMapLimit = 50_000
)

// Querier 是 *pgxpool.Pool 与 pgx.Tx 的公共子集。
type Querier interface {
	Exec(ctx context.Context, sql string, args ...any) (pgconn.CommandTag, error)
	Query(ctx context.Context, sql string, args ...any) (pgx.Rows, error)
	QueryRow(ctx context.Context, sql string, args ...any) pgx.Row
}

type Service struct {
	db       *pgxpool.Pool
	log      *slog.Logger
	settings *syssettings.Store

	touchMu sync.Mutex
	touched map[int64]time.Time
}

var _ core.PrincipalResolver = (*Service)(nil)

func New(db *pgxpool.Pool, log *slog.Logger, settings *syssettings.Store) *Service {
	return &Service{db: db, log: log, settings: settings, touched: map[int64]time.Time{}}
}

// Mount 挂 /me/api-keys（已在 RequireUser 分组内）。
func (s *Service) Mount(r chi.Router) {
	r.Get("/me/api-keys", s.handleList)
	r.Post("/me/api-keys", s.handleCreate)
	r.Delete("/me/api-keys/{id}", s.handleDelete)
}

// GenerateKey 生成平台 Key：raw 为明文（只在创建响应里出现一次），hash 落库，prefix 用于展示。
func GenerateKey() (raw, hash, prefix string) {
	raw = KeyPrefix + core.RandomString(keyRandLen)
	return raw, core.SHA256Hex(raw), raw[:DisplayPrefixLen]
}

// InsertDeviceKey 在调用方事务里为登录会话签发设备 Key（kind=device，绑定 session_id）。
func InsertDeviceKey(ctx context.Context, q Querier, userID int64, sessionID, name string) (id int64, raw, prefix string, err error) {
	raw, hash, prefix := GenerateKey()
	err = q.QueryRow(ctx,
		`INSERT INTO api_keys (user_id, name, kind, session_id, key_hash, key_prefix)
		 VALUES ($1, $2, 'device', $3, $4, $5) RETURNING id`,
		userID, name, sessionID, hash, prefix).Scan(&id)
	if err != nil {
		return 0, "", "", err
	}
	return id, raw, prefix, nil
}

// touch 更新 last_used_at：同一 Key 每分钟最多写一次库。
func (s *Service) touch(ctx context.Context, keyID int64) {
	now := time.Now()
	s.touchMu.Lock()
	if last, ok := s.touched[keyID]; ok && now.Sub(last) < touchInterval {
		s.touchMu.Unlock()
		return
	}
	if len(s.touched) >= touchMapLimit {
		for id, at := range s.touched {
			if now.Sub(at) >= touchInterval {
				delete(s.touched, id)
			}
		}
	}
	s.touched[keyID] = now
	s.touchMu.Unlock()
	if _, err := s.db.Exec(ctx, `UPDATE api_keys SET last_used_at = now() WHERE id = $1`, keyID); err != nil {
		s.log.Warn("touch api key failed", "key", keyID, "err", err)
	}
}

func utcPtr(t *time.Time) *time.Time {
	if t == nil {
		return nil
	}
	v := t.UTC()
	return &v
}
