package gateway_test

import (
	"net/http"
	"strings"
	"testing"

	"forge-cloud/internal/accounts"
	"forge-cloud/internal/core"
	"forge-cloud/internal/gateway/gwtest"
)

func TestRoutingExcludesAccountsWithoutRequestedModel(t *testing.T) {
	a := gwtest.New(t)
	a.MountFakeUpstream(t)
	a.SeedModel(core.Model{ID: "gpt-fake", Platform: "openai"})
	a.SeedOpenAIKeyAccount("wrong-model-first", "sk-fake-openai", 1, func(p *accounts.APIKeyAccountParams) {
		p.Priority = 1
		p.AllowedModels = []string{"claude-fake"}
	})
	right := a.SeedOpenAIKeyAccount("right-model", "sk-fake-openai", 1, func(p *accounts.APIKeyAccountParams) {
		p.Priority = 90
		p.AllowedModels = []string{"gpt-fake"}
	})
	res := a.DoV1(http.MethodPost, "/chat/completions", map[string]any{
		"model": "gpt-fake", "messages": []any{map[string]any{"role": "user", "content": "hello"}},
	}, nil)
	if res.Status != http.StatusOK || len(a.Biller.Records) != 1 || a.Biller.Records[0].AccountID != right {
		t.Fatalf("misrouted: status=%d records=%+v body=%s", res.Status, a.Biller.Records, res.Body)
	}
	a.Exec(`UPDATE upstream_accounts SET allowed_models='["other-model"]'::jsonb WHERE id=$1`, right)
	res = a.DoV1(http.MethodPost, "/chat/completions", map[string]any{
		"model": "gpt-fake", "messages": []any{map[string]any{"role": "user", "content": "hello"}},
	}, nil)
	if res.Status != http.StatusServiceUnavailable || !strings.Contains(strings.ToUpper(string(res.Body)), "NO_AVAILABLE_ACCOUNT") {
		t.Fatalf("missing model should be unavailable: %d %s", res.Status, res.Body)
	}
	if len(a.Upstream.RequestsTo("/v1/chat/completions")) != 1 {
		t.Fatal("unsupported model reached upstream")
	}
}

func TestEndpointSupportCheckedWithinModelScope(t *testing.T) {
	a := gwtest.New(t)
	a.MountFakeUpstream(t)
	a.SeedModel(core.Model{ID: "gpt-fake", Platform: "openai", Capabilities: core.ModelCapabilities{Responses: true}})
	a.SeedOpenAIKeyAccount("chat-only", "sk-fake-openai", 1, func(p *accounts.APIKeyAccountParams) {
		p.AllowedModels = []string{"gpt-fake"}
		p.SupportsResponses = false
	})
	a.SeedOpenAIKeyAccount("responses-other-model", "sk-fake-openai", 1, func(p *accounts.APIKeyAccountParams) { p.AllowedModels = []string{"other-model"} })
	res := a.DoV1(http.MethodPost, "/responses", map[string]any{"model": "gpt-fake", "input": "hello"}, nil)
	if res.Status != http.StatusBadRequest || !strings.Contains(strings.ToUpper(string(res.Body)), "ENDPOINT_NOT_SUPPORTED") {
		t.Fatalf("endpoint selection: %d %s", res.Status, res.Body)
	}
	if len(a.Upstream.RequestsTo("/v1/responses")) != 0 {
		t.Fatal("unsupported endpoint reached upstream")
	}
}
