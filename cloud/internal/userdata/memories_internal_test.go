package userdata

import (
	"context"
	"testing"
	"time"

	"github.com/jackc/pgx/v5"

	"forge-cloud/internal/testutil"
)

func TestLockUserAndMemoryInsert(t *testing.T) {
	pool := testutil.NewDB(t)
	ctx := context.Background()
	var uid int64
	if err := pool.QueryRow(ctx, `INSERT INTO users (email, password_hash, nickname, role, group_id)
		VALUES ('m@test.com', 'x', '', 'user', (SELECT id FROM groups WHERE is_default LIMIT 1)) RETURNING id`).Scan(&uid); err != nil {
		t.Fatal(err)
	}
	now := time.Now().UTC()
	err := pgx.BeginFunc(ctx, pool, func(tx pgx.Tx) error {
		if err := lockUser(ctx, tx, uid); err != nil {
			return err
		}
		_, err := tx.Exec(ctx,
			`INSERT INTO user_memories (user_id, id, scope, kind, content, tags, version, updated_at, deleted, change_seq)
			 VALUES ($1, $2, $3, $4, $5, $6, 1, $7, $8, nextval('userdata_change_seq'))
			 ON CONFLICT (user_id, id) DO UPDATE
			 SET scope = EXCLUDED.scope, kind = EXCLUDED.kind, content = EXCLUDED.content, tags = EXCLUDED.tags,
			     version = user_memories.version + 1, updated_at = EXCLUDED.updated_at, deleted = EXCLUDED.deleted,
			     change_seq = EXCLUDED.change_seq`,
			uid, "test-id", "global", "fact", "hello", []string{"t"}, now, false)
		return err
	})
	if err != nil {
		t.Fatalf("insert: %v", err)
	}
}
