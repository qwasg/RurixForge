package accounts_test

import (
	"context"
	"encoding/json"
	"net/http"
	"testing"
	"time"

	"os"

	"forge-cloud/internal/accounts"
	"forge-cloud/internal/devupstream"
	"forge-cloud/internal/gateway/gwtest"
)

func TestImportCodexAuthJSON(t *testing.T) {
	a := gwtest.New(t)
	a.MountFakeUpstream(t)
	toks := devupstream.IssueTokens("acct-import-1", 1, 24*time.Hour)
	authJSON, _ := json.Marshal(map[string]any{
		"tokens": map[string]any{
			"access_token":  toks["access_token"],
			"refresh_token": toks["refresh_token"],
			"id_token":      toks["id_token"],
			"account_id":    "acct-import-1",
		},
	})
	acc, created, err := a.Accounts.ImportAuthJSON(context.Background(), authJSON, accounts.ImportOptions{GroupIDs: []int64{1}})
	if err != nil || !created || acc.ExternalID != "acct-import-1" {
		t.Fatalf("import: created=%v acc=%+v err=%v", created, acc, err)
	}
	acc2, created2, err := a.Accounts.ImportAuthJSON(context.Background(), authJSON, accounts.ImportOptions{GroupIDs: []int64{1}})
	if err != nil || created2 || acc2.ID != acc.ID {
		t.Fatalf("reimport should update: created=%v id=%d err=%v", created2, acc2.ID, err)
	}
}

func TestImportCodexHTTP(t *testing.T) {
	a := gwtest.New(t)
	a.MountFakeUpstream(t)
	admin := a.NewAdminToken()
	toks := devupstream.IssueTokens("acct-http-1", 1, 24*time.Hour)
	body, _ := json.Marshal(map[string]any{
		"items": []map[string]any{{"authJson": string(mustJSON(map[string]any{
			"tokens": map[string]any{
				"access_token": toks["access_token"], "refresh_token": toks["refresh_token"],
				"id_token": toks["id_token"], "account_id": "acct-http-1",
			},
		}))}},
	})
	res := a.DoAdmin(http.MethodPost, "/accounts/import-codex", admin, body)
	if res.Status != http.StatusOK {
		t.Fatalf("import-codex: %s", res.Body)
	}
	var out struct {
		Created []accounts.View `json:"created"`
	}
	res.Decode(t, &out)
	if len(out.Created) != 1 || out.Created[0].AuthType != accounts.AuthOAuth {
		t.Fatalf("created=%+v", out.Created)
	}
}

func TestOAuthPKCEExchange(t *testing.T) {
	a := gwtest.New(t)
	a.MountFakeUpstream(t)
	admin := a.NewAdminToken()

	start := a.DoAdmin(http.MethodPost, "/accounts/oauth/openai/start", admin, map[string]any{})
	var st struct {
		SessionID string `json:"sessionId"`
		AuthURL   string `json:"authUrl"`
	}
	start.Decode(t, &st)
	if st.SessionID == "" || st.AuthURL == "" {
		t.Fatalf("start: %s", start.Body)
	}
	// 走假上游 authorize → 302 带 code。
	req, _ := http.NewRequest(http.MethodGet, st.AuthURL, nil)
	rec := httptestRecorder(t, req)
	if rec.StatusCode != http.StatusFound {
		t.Fatalf("authorize status=%d", rec.StatusCode)
	}
	callback := rec.Header.Get("Location")
	ex := a.DoAdmin(http.MethodPost, "/accounts/oauth/openai/exchange", admin, map[string]any{
		"sessionId": st.SessionID, "callbackUrl": callback, "groupIds": []int64{1},
	})
	if ex.Status != http.StatusOK {
		t.Fatalf("exchange: %s", ex.Body)
	}
	var acc accounts.View
	ex.Decode(t, &acc)
	if acc.AuthType != accounts.AuthOAuth || acc.Email == "" {
		t.Fatalf("account=%+v", acc)
	}
}

func TestModelsCRUD(t *testing.T) {
	a := gwtest.New(t)
	admin := a.NewAdminToken()
	create := a.DoAdmin(http.MethodPost, "/models", admin, map[string]any{
		"id": "gpt-test-1", "displayName": "Test", "platform": "openai",
		"pricing": map[string]any{"inputPer1M": 1000, "outputPer1M": 2000},
	})
	if create.Status != http.StatusOK {
		t.Fatalf("create: %s", create.Body)
	}
	patch := a.DoAdmin(http.MethodPatch, "/models/gpt-test-1", admin, map[string]any{"displayName": "Test 2"})
	if patch.Status != http.StatusOK {
		t.Fatalf("patch: %s", patch.Body)
	}
	list := a.DoAdmin(http.MethodGet, "/models", admin, nil)
	if list.Status != http.StatusOK {
		t.Fatalf("list: %s", list.Body)
	}
}

func TestImportCodexFilesCLI(t *testing.T) {
	a := gwtest.New(t)
	a.MountFakeUpstream(t)
	toks := devupstream.IssueTokens("acct-cli-1", 1, 24*time.Hour)
	path := t.TempDir() + "/auth.json"
	raw, _ := json.Marshal(map[string]any{
		"tokens": map[string]any{
			"access_token": toks["access_token"], "refresh_token": toks["refresh_token"],
			"id_token": toks["id_token"], "account_id": "acct-cli-1",
		},
	})
	if err := os.WriteFile(path, raw, 0o600); err != nil {
		t.Fatal(err)
	}
	res, err := a.Accounts.ImportCodexFiles(context.Background(), []string{path}, []int64{1})
	if err != nil || res.Created != 1 || len(res.Errors) != 0 {
		t.Fatalf("ImportCodexFiles: %+v err=%v", res, err)
	}
}

func mustJSON(v any) []byte {
	b, _ := json.Marshal(v)
	return b
}

// 避免 accounts_test import httptest 只为 OAuth redirect。
func httptestRecorder(t *testing.T, req *http.Request) *http.Response {
	t.Helper()
	client := &http.Client{CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }}
	resp, err := client.Do(req)
	if err != nil {
		t.Fatal(err)
	}
	return resp
}
