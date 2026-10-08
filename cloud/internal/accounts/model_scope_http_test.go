package accounts_test

import (
	"bytes"
	"context"
	"net/http"
	"reflect"
	"strconv"
	"testing"

	"forge-cloud/internal/accounts"
	"forge-cloud/internal/core"
	"forge-cloud/internal/gateway/gwtest"
)

func TestAccountModelScopeCRUD(t *testing.T) {
	a := gwtest.New(t)
	admin := a.NewAdminToken()
	res := a.DoAdmin(http.MethodPost, "/accounts", admin, map[string]any{
		"name": "scoped", "platform": "openai", "apiKey": "sk-test-model-scope",
		"allowedModels": []string{" gpt-latest ", "gpt-latest"},
	})
	if res.Status != http.StatusOK {
		t.Fatalf("create: %s", res.Body)
	}
	var view accounts.View
	res.Decode(t, &view)
	if !reflect.DeepEqual(view.AllowedModels, []string{"gpt-latest"}) {
		t.Fatalf("scope=%v", view.AllowedModels)
	}
	var before []byte
	if err := a.DB.QueryRow(context.Background(), `SELECT credentials FROM upstream_accounts WHERE id=$1`, view.ID).Scan(&before); err != nil {
		t.Fatal(err)
	}
	path := "/accounts/" + strconv.FormatInt(view.ID, 10)
	res = a.DoAdmin(http.MethodPatch, path, admin, map[string]any{"name": "renamed"})
	res.Decode(t, &view)
	if !reflect.DeepEqual(view.AllowedModels, []string{"gpt-latest"}) {
		t.Fatalf("unrelated patch changed scope: %s", res.Body)
	}
	res = a.DoAdmin(http.MethodPatch, path, admin, map[string]any{"allowedModels": []string{}})
	if res.Status != http.StatusOK {
		t.Fatalf("clear: %s", res.Body)
	}
	res.Decode(t, &view)
	if view.AllowedModels == nil || len(view.AllowedModels) != 0 {
		t.Fatalf("clear scope=%v", view.AllowedModels)
	}
	var after []byte
	if err := a.DB.QueryRow(context.Background(), `SELECT credentials FROM upstream_accounts WHERE id=$1`, view.ID).Scan(&after); err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(before, after) {
		t.Fatal("scope update changed encrypted credentials")
	}
	res = a.DoAdmin(http.MethodPatch, path, admin, map[string]any{"allowedModels": []string{"bad id"}})
	if res.Status != http.StatusBadRequest {
		t.Fatalf("invalid scope accepted: %s", res.Body)
	}
}

func TestModelAvailabilityRespectsAccountScope(t *testing.T) {
	a := gwtest.New(t)
	a.MountFakeUpstream(t)
	a.SeedModel(core.Model{ID: "model-a", Platform: "openai"})
	a.SeedModel(core.Model{ID: "model-b", Platform: "openai"})
	accountID := a.SeedOpenAIKeyAccount("scope-a", "sk-fake-openai", 1, func(p *accounts.APIKeyAccountParams) { p.AllowedModels = []string{"model-a"} })
	list, err := a.Accounts.EnabledModels(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	counts, err := a.Accounts.AvailableModelCounts(context.Background(), 1, list)
	if err != nil || counts["model-a"] != 1 || counts["model-b"] != 0 {
		t.Fatalf("counts=%v err=%v", counts, err)
	}
	user := a.RegisterUser("scope-user@example.com", "")
	res := a.DoCatalog(user)
	var catalog struct {
		Models []struct {
			ID        string `json:"id"`
			Available bool   `json:"available"`
		} `json:"models"`
	}
	res.Decode(t, &catalog)
	if len(catalog.Models) != 2 {
		t.Fatalf("catalog: %s", res.Body)
	}
	for _, m := range catalog.Models {
		if m.Available != (m.ID == "model-a") {
			t.Fatalf("catalog incorrectly available: %+v", m)
		}
	}
	a.Exec(`UPDATE upstream_accounts SET status='disabled' WHERE id=$1`, accountID)
	counts, err = a.Accounts.AvailableModelCounts(context.Background(), 1, list)
	if err != nil || counts["model-a"] != 0 {
		t.Fatalf("disabled counts=%v err=%v", counts, err)
	}
}
