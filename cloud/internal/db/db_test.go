package db_test

import (
	"context"
	"testing"

	"forge-cloud/internal/testutil"
)

func TestMigrationsApplyAndSeedDefaultGroup(t *testing.T) {
	pool := testutil.NewDB(t)
	var name string
	if err := pool.QueryRow(context.Background(), `SELECT name FROM groups WHERE is_default`).Scan(&name); err != nil {
		t.Fatalf("默认分组缺失: %v", err)
	}
	if name != "default" {
		t.Fatalf("默认分组名 = %q", name)
	}
	var n int
	if err := pool.QueryRow(context.Background(),
		`SELECT count(*) FROM information_schema.tables WHERE table_schema = 'public' AND table_name IN
		 ('users','refresh_sessions','api_keys','upstream_accounts','models','usage_logs','balance_ledger',
		  'user_settings','user_memories','user_skills','redeem_codes','subscriptions','plans','audit_logs')`).Scan(&n); err != nil {
		t.Fatal(err)
	}
	if n != 14 {
		t.Fatalf("核心表数量 = %d，期望 14", n)
	}
}
