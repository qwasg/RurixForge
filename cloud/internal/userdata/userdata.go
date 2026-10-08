// Package userdata：资料同步（设置命名空间、记忆、个人技能）（15_CLOUD_SERVICE.md §3.4）。
//
// 记忆与技能共用全局序列 userdata_change_seq 作游标。写入事务先取「按用户」的咨询锁，
// 使同一用户的 change_seq 分配与提交顺序一致，读方按游标增量拉取不会漏掉晚提交的小序号。
package userdata

import (
	"context"
	"fmt"
	"log/slog"
	"net/http"
	"time"

	"github.com/go-chi/chi/v5"
	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"

	"forge-cloud/internal/core"
)

type Service struct {
	db  *pgxpool.Pool
	log *slog.Logger
}

func New(db *pgxpool.Pool, log *slog.Logger) *Service { return &Service{db: db, log: log} }

// Mount 挂 /me/settings、/me/memories、/me/skills（已在 RequireUser 分组内）。
func (s *Service) Mount(r chi.Router) {
	r.Get("/me/settings", s.handleGetSettings)
	r.Put("/me/settings/{ns}", s.handlePutSetting)
	r.Get("/me/memories", s.handleGetMemories)
	r.Put("/me/memories", s.handlePutMemories)
	r.Get("/me/skills", s.handleGetSkills)
	r.Put("/me/skills/{name}", s.handlePutSkill)
	r.Delete("/me/skills/{name}", s.handleDeleteSkill)
}

func claims(r *http.Request) (*core.Claims, error) {
	c, ok := core.ClaimsFrom(r.Context())
	if !ok {
		return nil, core.Unauthorized("UNAUTHORIZED", "未登录")
	}
	return c, nil
}

// lockUser 取当前事务内的按用户咨询锁（见包注释）。
func lockUser(ctx context.Context, tx pgx.Tx, userID int64) error {
	_, err := tx.Exec(ctx, `SELECT pg_advisory_xact_lock(hashtextextended($1, 0))`, fmt.Sprintf("forge-userdata:%d", userID))
	return err
}

// normTime 统一到 UTC 微秒精度（与 timestamptz 一致），保证客户端回传同一时间戳时比较相等。
func normTime(t time.Time) time.Time { return t.UTC().Truncate(time.Microsecond) }
