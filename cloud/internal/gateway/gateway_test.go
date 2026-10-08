package gateway_test

import (
	"context"
	"encoding/json"
	"net/http"
	"strings"
	"testing"
	"time"

	"forge-cloud/internal/accounts"
	"forge-cloud/internal/core"
	"forge-cloud/internal/devupstream"
	"forge-cloud/internal/gateway/gwtest"
	"forge-cloud/internal/ratelimit"
)

func setupGateway(t *testing.T) *gwtest.App {
	t.Helper()
	a := gwtest.New(t)
	a.MountFakeUpstream(t)
	a.SeedModel(core.Model{ID: "gpt-fake", Platform: accounts.PlatformOpenAI, UpstreamModel: "gpt-fake"})
	a.SeedModel(core.Model{ID: "claude-fake", Platform: accounts.PlatformAnthropic, UpstreamModel: "claude-fake",
		Pricing: core.Pricing{InputPer1M: 1_000_000, OutputPer1M: 2_000_000}})
	a.SeedOpenAIKeyAccount("openai-1", "sk-fake-openai", 1)
	toks := devupstream.IssueTokens("codex-acct-1", 1, 24*time.Hour)
	a.SeedCodexAccount("codex-1", "codex-acct-1", toks["access_token"].(string), toks["refresh_token"].(string), 1)
	return a
}

func TestChatPassthroughNonStream(t *testing.T) {
	a := setupGateway(t)
	res := a.DoV1(http.MethodPost, "/chat/completions", map[string]any{
		"model": "gpt-fake", "stream": false,
		"messages": []any{map[string]any{"role": "user", "content": "hello"}},
	}, nil)
	if res.Status != http.StatusOK {
		t.Fatalf("status=%d body=%s", res.Status, res.Body)
	}
	var out struct {
		Choices []struct {
			Message struct {
				Content string `json:"content"`
			} `json:"message"`
		} `json:"choices"`
	}
	res.Decode(t, &out)
	if !strings.HasPrefix(out.Choices[0].Message.Content, "fake: hello") {
		t.Fatalf("content=%q", out.Choices[0].Message.Content)
	}
	if len(a.Biller.Records) != 1 || a.Biller.Records[0].Status != "ok" {
		t.Fatalf("settle records=%+v", a.Biller.Records)
	}
}

func TestChatPassthroughStream(t *testing.T) {
	a := setupGateway(t)
	res := a.DoV1(http.MethodPost, "/chat/completions", map[string]any{
		"model": "gpt-fake", "stream": true,
		"messages": []any{map[string]any{"role": "user", "content": "stream"}},
	}, nil)
	if res.Status != http.StatusOK || !strings.Contains(res.Header.Get("Content-Type"), "text/event-stream") {
		t.Fatalf("status=%d ct=%q", res.Status, res.Header.Get("Content-Type"))
	}
	if !strings.Contains(string(res.Body), "fake:") || !strings.Contains(string(res.Body), "stream") {
		t.Fatalf("body=%s", res.Body)
	}
}

func TestCodexResponsesPassthrough(t *testing.T) {
	a := setupGateway(t)
	a.Exec(`DELETE FROM upstream_accounts WHERE auth_type = 'apikey'`)
	a.SeedModel(core.Model{ID: "codex-model", Platform: accounts.PlatformOpenAI, UpstreamModel: "gpt-fake",
		Capabilities: core.ModelCapabilities{Responses: true}})
	res := a.DoV1(http.MethodPost, "/responses", map[string]any{
		"model": "codex-model", "input": "hi codex",
	}, map[string]string{"X-Forge-Session": "sess-1"})
	if res.Status != http.StatusOK {
		t.Fatalf("status=%d %s", res.Status, res.Body)
	}
	reqs := a.Upstream.RequestsTo("/codex/responses")
	if len(reqs) == 0 {
		t.Fatal("no upstream codex request")
	}
	var body map[string]any
	_ = json.Unmarshal(reqs[0].Body, &body)
	if body["store"] != false || body["stream"] != true {
		t.Fatalf("codex body=%v", body)
	}
	if reqs[0].Header.Get("chatgpt-account-id") != "codex-acct-1" {
		t.Fatalf("headers=%v", reqs[0].Header)
	}
}

func TestCodexChatConvertedStream(t *testing.T) {
	a := setupGateway(t)
	a.SeedModel(core.Model{ID: "codex-chat", Platform: accounts.PlatformOpenAI, UpstreamModel: "gpt-fake"})
	res := a.DoV1(http.MethodPost, "/chat/completions", map[string]any{
		"model": "codex-chat", "stream": true,
		"messages": []any{map[string]any{"role": "user", "content": "via codex"}},
	}, nil)
	if res.Status != http.StatusOK {
		t.Fatalf("%s", res.Body)
	}
	if !strings.Contains(string(res.Body), "chat.completion.chunk") {
		t.Fatalf("expected chat chunks: %s", res.Body)
	}
}

func TestFailover429(t *testing.T) {
	a := setupGateway(t)
	a.Exec(`DELETE FROM upstream_accounts WHERE auth_type = 'apikey' AND name = 'openai-1'`)
	a.SeedOpenAIKeyAccount("bad", "sk-fake-429", 1, func(p *accounts.APIKeyAccountParams) { p.Priority = 1 })
	a.SeedOpenAIKeyAccount("good", "sk-fake-openai", 1, func(p *accounts.APIKeyAccountParams) { p.Priority = 2 })
	res := a.DoV1(http.MethodPost, "/chat/completions", map[string]any{
		"model": "gpt-fake", "stream": false,
		"messages": []any{map[string]any{"role": "user", "content": "hello"}},
	}, nil)
	if res.Status != http.StatusOK {
		t.Fatalf("failover failed: %d %s", res.Status, res.Body)
	}
}

func TestPrecheck402(t *testing.T) {
	a := setupGateway(t)
	a.Biller.PrecheckErr = core.E(http.StatusPaymentRequired, core.CodeInsufficientBalance, "余额不足")
	res := a.DoV1(http.MethodPost, "/chat/completions", map[string]any{
		"model": "gpt-fake", "messages": []any{map[string]any{"role": "user", "content": "x"}},
	}, nil)
	res.Expect(t, http.StatusPaymentRequired, "insufficient_balance")
}

func TestModelNotAllowed(t *testing.T) {
	a := setupGateway(t)
	p := gwtest.DefaultPrincipal()
	p.Group.AllowedModels = []string{"other-model"}
	a.Principals.P = p
	res := a.DoV1(http.MethodPost, "/chat/completions", map[string]any{
		"model": "gpt-fake", "messages": []any{map[string]any{"role": "user", "content": "x"}},
	}, nil)
	res.Expect(t, http.StatusForbidden, "model_not_allowed")
}

func TestConcurrencyLimited(t *testing.T) {
	a := setupGateway(t)
	p := gwtest.DefaultPrincipal()
	p.UserConcurrency = 1
	a.Principals.P = p
	ctx := context.Background()
	slot := ratelimit.UserSlotKey(p.UserID)
	ok, err := a.Gateway.Limiter().Acquire(ctx, slot, 1, "hold")
	if err != nil || !ok {
		t.Fatalf("prefill slot: ok=%v err=%v", ok, err)
	}
	defer func() { _ = a.Gateway.Limiter().Release(ctx, slot, "hold") }()
	res := a.DoV1(http.MethodPost, "/chat/completions", map[string]any{
		"model": "gpt-fake", "messages": []any{map[string]any{"role": "user", "content": "blocked"}},
	}, nil)
	res.Expect(t, http.StatusTooManyRequests, "concurrency_limited")
}

func TestCatalogScaledPricing(t *testing.T) {
	a := setupGateway(t)
	a.Exec(`UPDATE groups SET rate_multiplier = 2 WHERE id = 1`)
	token := a.RegisterUser("cat-"+core.RandomString(6)+"@example.com", "")
	uid := a.Int64(`SELECT id FROM users ORDER BY id DESC LIMIT 1`)
	p := gwtest.DefaultPrincipal()
	p.UserID = uid
	p.Group.RateMultiplier = 2
	a.Principals.P = p
	res := a.DoCatalog(token)
	if res.Status != http.StatusOK {
		t.Fatalf("%s", res.Body)
	}
	var cat struct {
		RateMultiplier float64 `json:"rateMultiplier"`
		Models         []struct {
			ID      string `json:"id"`
			Pricing struct {
				InputPer1M int64 `json:"inputPer1M"`
			} `json:"pricing"`
		} `json:"models"`
	}
	res.Decode(t, &cat)
	if cat.RateMultiplier != 2 {
		t.Fatalf("multiplier=%v", cat.RateMultiplier)
	}
	for _, m := range cat.Models {
		if m.ID == "claude-fake" && m.Pricing.InputPer1M != 2_000_000 {
			t.Fatalf("scaled pricing=%d", m.Pricing.InputPer1M)
		}
	}
}

func TestStickySessionHeader(t *testing.T) {
	a := setupGateway(t)
	h := map[string]string{"X-Forge-Session": "sticky-key-99"}
	a.DoV1(http.MethodPost, "/chat/completions", map[string]any{
		"model": "gpt-fake", "stream": false,
		"messages": []any{map[string]any{"role": "user", "content": "one"}},
	}, h)
	a.DoV1(http.MethodPost, "/chat/completions", map[string]any{
		"model": "gpt-fake", "stream": false,
		"messages": []any{map[string]any{"role": "user", "content": "two"}},
	}, h)
	found := false
	for _, k := range a.MR.Keys() {
		if strings.HasPrefix(k, "sticky:1:") {
			found = true
			break
		}
	}
	if !found {
		t.Fatal("sticky key not stored")
	}
}
