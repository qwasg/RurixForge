package gateway_test

import (
	"context"
	"net/http"
	"net/http/httptest"
	"strconv"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"forge-cloud/internal/accounts"
	"forge-cloud/internal/gateway/gwtest"
)

func TestNetworkFailureAdvertisesCooldown(t *testing.T) {
	a := setupGateway(t)
	a.Exec(`DELETE FROM upstream_accounts`)
	var calls atomic.Int32
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls.Add(1)
		conn, _, err := w.(http.Hijacker).Hijack()
		if err != nil {
			t.Errorf("hijack: %v", err)
			return
		}
		_ = conn.Close()
	}))
	t.Cleanup(upstream.Close)
	a.SeedOpenAIKeyAccount("offline", "sk-fake-offline", 1, func(p *accounts.APIKeyAccountParams) {
		p.BaseURL = upstream.URL
	})
	body := map[string]any{
		"model":    "gpt-fake",
		"messages": []any{map[string]any{"role": "user", "content": "ping"}},
	}
	first := a.DoV1(http.MethodPost, "/chat/completions", body, nil)
	first.Expect(t, http.StatusBadGateway, "upstream_error")
	if got := first.Header.Get("Retry-After"); got != "30" {
		t.Fatalf("network Retry-After=%q, want 30", got)
	}
	second := a.DoV1(http.MethodPost, "/chat/completions", body, nil)
	second.Expect(t, http.StatusServiceUnavailable, "no_available_account")
	retry, err := strconv.Atoi(second.Header.Get("Retry-After"))
	if err != nil || retry < 1 || retry > 30 {
		t.Fatalf("cooldown Retry-After=%q", second.Header.Get("Retry-After"))
	}
	if got := calls.Load(); got != 1 {
		t.Fatalf("upstream calls=%d, want 1 while account is cooling down", got)
	}
}

func TestCooldownRetryIgnoresUnsupportedModels(t *testing.T) {
	a := setupGateway(t)
	a.Exec(`DELETE FROM upstream_accounts`)
	id := a.SeedOpenAIKeyAccount("other-model", "sk-fake-other", 1, func(p *accounts.APIKeyAccountParams) {
		p.AllowedModels = []string{"different-model"}
	})
	if err := a.Accounts.Cooldown(context.Background(), id, time.Minute, "test cooldown"); err != nil {
		t.Fatal(err)
	}
	res := a.DoV1(http.MethodPost, "/chat/completions", map[string]any{
		"model":    "gpt-fake",
		"messages": []any{map[string]any{"role": "user", "content": "ping"}},
	}, nil)
	res.Expect(t, http.StatusServiceUnavailable, "no_available_account")
	if got := res.Header.Get("Retry-After"); got != "" {
		t.Fatalf("unrelated model advertised Retry-After=%q", got)
	}
}

func TestChatDoneDoesNotWaitForUpstreamClose(t *testing.T) {
	a := setupGateway(t)
	a.Exec(`DELETE FROM upstream_accounts`)
	release := make(chan struct{})
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "text/event-stream")
		_, _ = w.Write([]byte("data: {\"choices\":[{\"delta\":{\"content\":\"complete\"}}]}\n\n" +
			"data: {\"choices\":[],\"usage\":{\"prompt_tokens\":2,\"completion_tokens\":1,\"total_tokens\":3}}\n\n" +
			"data: [DONE]\n\n"))
		w.(http.Flusher).Flush()
		select {
		case <-r.Context().Done():
		case <-release:
		}
	}))
	t.Cleanup(upstream.Close)
	a.SeedOpenAIKeyAccount("keeps-stream-open", "sk-fake-stream", 1, func(p *accounts.APIKeyAccountParams) {
		p.BaseURL = upstream.URL
	})
	result := make(chan *gwtest.Response, 1)
	go func() {
		result <- a.DoV1(http.MethodPost, "/chat/completions", map[string]any{
			"model": "gpt-fake", "stream": true,
			"stream_options": map[string]any{"include_usage": true},
			"messages":       []any{map[string]any{"role": "user", "content": "ping"}},
		}, nil)
	}()
	var res *gwtest.Response
	select {
	case res = <-result:
		close(release)
	case <-time.After(2 * time.Second):
		close(release)
		<-result
		t.Fatal("gateway waited for upstream EOF after receiving [DONE]")
	}
	if res.Status != http.StatusOK || !strings.Contains(string(res.Body), "data: [DONE]") {
		t.Fatalf("status=%d body=%s", res.Status, res.Body)
	}
	if len(a.Biller.Records) != 1 || a.Biller.Records[0].Status != "ok" || a.Biller.Records[0].Usage.Total() != 3 {
		t.Fatalf("completed stream settlement=%+v", a.Biller.Records)
	}
}
