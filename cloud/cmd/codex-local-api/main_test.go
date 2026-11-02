package main

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"forge-cloud/internal/gateway/convert"
)

func testConfig() gatewayConfig {
	return gatewayConfig{APIKey: strings.Repeat("k", 40), Models: []string{"codex/gpt-test"}, Model: "codex/gpt-test"}
}

func request(g *localGateway, method, path, body, auth string) *httptest.ResponseRecorder {
	r := httptest.NewRequest(method, path, strings.NewReader(body))
	if auth != "" {
		r.Header.Set("Authorization", "Bearer "+auth)
	}
	w := httptest.NewRecorder()
	g.ServeHTTP(w, r)
	return w
}

func completedSSE() []byte {
	completed := `{"type":"response.completed","response":{"id":"resp_test","object":"response","created_at":1700000000,"status":"completed","model":"gpt-test","output":[{"id":"msg_test","type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":"RF_OK","annotations":[]}]}],"usage":{"input_tokens":3,"output_tokens":2,"total_tokens":5}}}`
	return convert.FormatSSE("response.completed", []byte(completed))
}

func TestAuthenticationAndCatalog(t *testing.T) {
	cfg := testConfig()
	g := newGateway(cfg, nil, "", func(context.Context, gatewayConfig, bool) (codexCredentials, error) {
		t.Fatal("catalog/auth checks must not load upstream credentials")
		return codexCredentials{}, nil
	})
	for _, key := range []string{"", "wrong"} {
		w := request(g, "GET", "/v1/models", "", key)
		if w.Code != 401 {
			t.Fatalf("unauthorized status=%d", w.Code)
		}
	}
	w := request(g, "GET", "/healthz", "", "")
	if w.Code != 200 || strings.TrimSpace(w.Body.String()) != `{"status":"ok"}` {
		t.Fatal("invalid health response")
	}
	for _, header := range []string{"Authorization", "x-api-key"} {
		r := httptest.NewRequest("GET", "/v1/models", nil)
		value := cfg.APIKey
		if header == "Authorization" {
			value = "Bearer " + value
		}
		r.Header.Set(header, value)
		w := httptest.NewRecorder()
		g.ServeHTTP(w, r)
		if w.Code != 200 || !strings.Contains(w.Body.String(), "codex/gpt-test") || strings.Contains(w.Body.String(), cfg.APIKey) {
			t.Fatalf("invalid catalog result for %s", header)
		}
	}
}

func TestShapePreservesToolHistoryAndEncryptedReasoning(t *testing.T) {
	body := []byte(`{"model":"codex/gpt-test","input":[{"type":"reasoning","id":"rs1","encrypted_content":"opaque"},{"type":"function_call","call_id":"call1","name":"echo","arguments":"{}"},{"type":"function_call_output","call_id":"call1","output":"RF_TOOL_OK"}],"tools":[{"type":"function","name":"echo","parameters":{"type":"object"}}],"tool_choice":"none","include":["message.output_text.logprobs"],"stream":false,"store":true,"temperature":0.2,"max_output_tokens":8,"top_p":1}`)
	out, err := shapeResponses(body, "gpt-test")
	if err != nil {
		t.Fatal(err)
	}
	var original, shaped map[string]json.RawMessage
	_ = json.Unmarshal(body, &original)
	_ = json.Unmarshal(out, &shaped)
	for _, key := range []string{"input", "tools", "tool_choice"} {
		if string(original[key]) != string(shaped[key]) {
			t.Fatalf("%s was altered", key)
		}
	}
	for _, key := range []string{"temperature", "max_output_tokens", "top_p"} {
		if _, ok := shaped[key]; ok {
			t.Fatalf("unsupported field %s survived", key)
		}
	}
	if string(shaped["model"]) != `"gpt-test"` || string(shaped["stream"]) != "true" || string(shaped["store"]) != "false" {
		t.Fatal("invalid upstream shaping")
	}
	if !strings.Contains(string(shaped["include"]), "reasoning.encrypted_content") || !strings.Contains(string(shaped["include"]), "message.output_text.logprobs") {
		t.Fatal("include fields were not preserved")
	}
}

func TestResponsesAndChatModes(t *testing.T) {
	for _, mode := range []struct {
		path   string
		stream bool
	}{
		{"/v1/responses", false}, {"/v1/responses", true},
		{"/v1/chat/completions", false}, {"/v1/chat/completions", true},
	} {
		name := mode.path
		if mode.stream {
			name += "-stream"
		}
		t.Run(name, func(t *testing.T) {
			upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				if r.Header.Get("Authorization") != "Bearer upstream-test-token" || r.Header.Get("chatgpt-account-id") != "account-test" {
					t.Error("wrong upstream credentials")
				}
				var body map[string]any
				if json.NewDecoder(r.Body).Decode(&body) != nil || body["model"] != "gpt-test" || body["stream"] != true || body["store"] != false {
					t.Error("invalid upstream request")
				}
				w.Header().Set("Content-Type", "text/event-stream")
				_, _ = w.Write(completedSSE())
			}))
			defer upstream.Close()
			g := newGateway(testConfig(), upstream.Client(), upstream.URL, func(context.Context, gatewayConfig, bool) (codexCredentials, error) {
				return codexCredentials{AccessToken: "upstream-test-token", AccountID: "account-test"}, nil
			})
			body := map[string]any{"model": "codex/gpt-test", "stream": mode.stream, "input": "hello",
				"messages": []any{map[string]any{"role": "user", "content": "hello"}}}
			encoded, _ := json.Marshal(body)
			result := request(g, "POST", mode.path, string(encoded), g.config.APIKey)
			if result.Code != 200 || !strings.Contains(result.Body.String(), "RF_OK") {
				t.Fatalf("result status=%d body=%s", result.Code, result.Body.String())
			}
			if mode.stream && mode.path == "/v1/chat/completions" && !strings.Contains(result.Body.String(), "data: [DONE]") {
				t.Fatal("chat completion frame missing")
			}
			if !mode.stream && !json.Valid(result.Body.Bytes()) {
				t.Fatal("nonstream response is not JSON")
			}
		})
	}
}

func TestOnlyOneCredentialRenewalOn401(t *testing.T) {
	var requests, renewals atomic.Int32
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		requests.Add(1)
		w.WriteHeader(401)
		_, _ = io.WriteString(w, `{"error":"do-not-leak-upstream-secret"}`)
	}))
	defer upstream.Close()
	g := newGateway(testConfig(), upstream.Client(), upstream.URL, func(_ context.Context, _ gatewayConfig, force bool) (codexCredentials, error) {
		if force {
			renewals.Add(1)
		}
		return codexCredentials{AccessToken: "upstream-secret", AccountID: "account-test"}, nil
	})
	w := request(g, "POST", "/v1/responses", `{"input":"hello"}`, g.config.APIKey)
	if w.Code != 502 || requests.Load() != 2 || renewals.Load() != 1 || strings.Contains(w.Body.String(), "upstream-secret") {
		t.Fatalf("invalid retry result: status=%d requests=%d renewals=%d", w.Code, requests.Load(), renewals.Load())
	}
}

func TestRejectsIncompleteStreamAndCapacity(t *testing.T) {
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "text/event-stream")
		_, _ = io.WriteString(w, "data: {\"type\":\"response.created\",\"response\":{\"id\":\"r1\"}}\n\n")
	}))
	defer upstream.Close()
	g := newGateway(testConfig(), upstream.Client(), upstream.URL, func(context.Context, gatewayConfig, bool) (codexCredentials, error) {
		return codexCredentials{AccessToken: "token", AccountID: "account"}, nil
	})
	w := request(g, "POST", "/v1/responses", `{"input":"hello"}`, g.config.APIKey)
	if w.Code != 502 || !strings.Contains(w.Body.String(), "upstream_stream_incomplete") {
		t.Fatal("incomplete stream incorrectly succeeded")
	}
	for i := 0; i < cap(g.slots); i++ {
		g.slots <- struct{}{}
	}
	w = request(g, "POST", "/v1/responses", `{"input":"hello"}`, g.config.APIKey)
	if w.Code != 429 {
		t.Fatal("concurrency limit not enforced")
	}
}

func TestRejectsUnknownModelsAndOversizedInputBeforeCredentials(t *testing.T) {
	g := newGateway(testConfig(), nil, "", func(context.Context, gatewayConfig, bool) (codexCredentials, error) {
		t.Fatal("invalid input must not load credentials")
		return codexCredentials{}, nil
	})
	w := request(g, "POST", "/v1/responses", `{"model":"unconfigured","input":"hello"}`, g.config.APIKey)
	if w.Code != 404 {
		t.Fatal("unconfigured model accepted")
	}
	w = request(g, "POST", "/v1/responses", strings.Repeat("x", requestLimit+1), g.config.APIKey)
	if w.Code != 413 {
		t.Fatal("oversized request accepted")
	}
}

func TestUpstreamWithoutContentType(t *testing.T) {
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		// A nil header suppresses net/http's automatic content sniffing.
		w.Header()["Content-Type"] = nil
		w.WriteHeader(200)
		_, _ = w.Write(completedSSE())
	}))
	defer upstream.Close()
	g := newGateway(testConfig(), upstream.Client(), upstream.URL, func(context.Context, gatewayConfig, bool) (codexCredentials, error) {
		return codexCredentials{AccessToken: "test-token", AccountID: "test-account"}, nil
	})
	w := request(g, "POST", "/v1/responses", `{"input":"hello"}`, g.config.APIKey)
	if w.Code != 200 || !strings.Contains(w.Body.String(), "RF_OK") {
		t.Fatalf("valid untyped SSE rejected: status=%d body=%s", w.Code, w.Body.String())
	}
}

func TestLongReasoningTimeoutConfiguration(t *testing.T) {
	g := newGateway(testConfig(), nil, "", nil)
	if g.timeout() != 600*time.Second {
		t.Fatal("default reasoning timeout must be ten minutes")
	}
	g.config.RequestTimeoutSeconds = 1800
	if g.timeout() != 1800*time.Second {
		t.Fatal("configured timeout not applied")
	}
}
