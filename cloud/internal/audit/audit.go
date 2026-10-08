// Package audit 记录管理写操作审计日志（失败只打日志，不影响业务）。
package audit

import (
	"context"
	"encoding/json"
	"log/slog"

	"github.com/jackc/pgx/v5/pgxpool"
)

// Record 写一条审计日志。detail 会被 JSON 序列化（nil → {}）；绝不传入密钥明文。
func Record(ctx context.Context, db *pgxpool.Pool, actorID int64, action, target string, detail any, ip string) {
	b := []byte("{}")
	if detail != nil {
		if v, err := json.Marshal(detail); err == nil {
			b = v
		}
	}
	var actor any
	if actorID > 0 {
		actor = actorID
	}
	if _, err := db.Exec(ctx,
		`INSERT INTO audit_logs (actor_id, action, target, detail, ip) VALUES ($1, $2, $3, $4, $5)`,
		actor, action, target, b, ip); err != nil {
		slog.Default().Warn("audit write failed", "action", action, "err", err)
	}
}
