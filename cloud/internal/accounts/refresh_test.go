package accounts_test

import (
	"context"
	"testing"
	"time"

	"forge-cloud/internal/devupstream"
	"forge-cloud/internal/gateway/gwtest"
)

func TestRefreshDueScansStaleOAuth(t *testing.T) {
	a := gwtest.New(t)
	a.MountFakeUpstream(t)
	toks := devupstream.IssueTokens("acct-refresh", 1, time.Hour)
	accID := a.SeedCodexAccount("refresh-me", "acct-refresh", toks["access_token"].(string), toks["refresh_token"].(string), 1)
	a.Exec(`UPDATE upstream_accounts SET token_expires_at = now() + interval '1 hour',
		last_refresh_at = now() - interval '8 days' WHERE id = $1`, accID)
	ok, failed := a.Accounts.RefreshDue(context.Background())
	if ok != 1 || failed != 0 {
		t.Fatalf("RefreshDue ok=%d failed=%d", ok, failed)
	}
	if a.Int64(`SELECT count(*) FROM upstream_accounts WHERE id = $1 AND last_refresh_at > now() - interval '1 minute'`, accID) != 1 {
		t.Fatalf("token not refreshed")
	}
}
