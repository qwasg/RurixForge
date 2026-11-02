// codex-local-api shares this machine's existing Forge Codex login through a
// loopback-only OpenAI-compatible API. It never exports upstream credentials.
package main

import (
	"bytes"
	"context"
	"crypto/sha256"
	"crypto/subtle"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"log"
	"net/http"
	"os"
	"os/signal"
	"path/filepath"
	"strings"
	"time"

	"forge-cloud/internal/gateway/convert"
	"forge-cloud/internal/oauth/openai"
)

const (
	listenAddress       = "127.0.0.1:8111"
	codexEndpoint       = "https://chatgpt.com/backend-api/codex/responses"
	requestLimit        = 8 << 20
	responseLimit       = 64 << 20
	requestTimeout      = 600 * time.Second
	defaultInstructions = "You are a helpful assistant. Follow the user's instructions."
)

type gatewayConfig struct {
	APIKey                string   `json:"apiKey"`
	Models                []string `json:"models"`
	Model                 string   `json:"model"`
	CodexHome             string   `json:"codexHome"`
	CodexBin              string   `json:"codexBin"`
	BaseURL               string   `json:"baseUrl"`
	RequestTimeoutSeconds int      `json:"requestTimeoutSeconds,omitempty"`
}

type credentialLoader func(context.Context, gatewayConfig, bool) (codexCredentials, error)

type localGateway struct {
	config      gatewayConfig
	client      *http.Client
	upstream    string
	credentials credentialLoader
	slots       chan struct{}
}

func newGateway(config gatewayConfig, client *http.Client, endpoint string, loader credentialLoader) *localGateway {
	return &localGateway{config: config, client: client, upstream: endpoint, credentials: loader, slots: make(chan struct{}, 4)}
}

func readConfig(path string) (gatewayConfig, error) {
	var config gatewayConfig
	data, err := os.ReadFile(path)
	if err != nil {
		return config, errors.New("cannot read private client configuration")
	}
	data = bytes.TrimPrefix(data, []byte{0xef, 0xbb, 0xbf})
	if json.Unmarshal(data, &config) != nil || len(config.APIKey) < 24 || len(config.Models) == 0 {
		return config, errors.New("invalid private client configuration")
	}
	seen := map[string]bool{}
	for _, model := range config.Models {
		if !strings.HasPrefix(model, "codex/") || len(model) <= len("codex/") || strings.ContainsAny(model, " \t\r\n") || seen[model] {
			return config, errors.New("invalid configured model list")
		}
		seen[model] = true
	}
	if !seen[config.Model] || !filepath.IsAbs(config.CodexHome) {
		return config, errors.New("invalid default model or Codex home")
	}
	if config.RequestTimeoutSeconds < 0 || config.RequestTimeoutSeconds > 3600 {
		return config, errors.New("request timeout must be between 1 and 3600 seconds, or zero for the default")
	}
	return config, nil
}

func (g *localGateway) timeout() time.Duration {
	if g.config.RequestTimeoutSeconds > 0 {
		return time.Duration(g.config.RequestTimeoutSeconds) * time.Second
	}
	return requestTimeout
}

func main() {
	home, _ := os.UserHomeDir()
	configPath := flag.String("config", filepath.Join(home, ".config", "rurixforge", "codex-api.json"), "private client configuration path")
	checkAuth := flag.Bool("check-auth", false, "verify and renew the existing Forge Codex authentication, then exit")
	flag.Parse()
	config, err := readConfig(*configPath)
	if err != nil {
		fmt.Fprintln(os.Stderr, err.Error())
		os.Exit(1)
	}
	if *checkAuth {
		ctx, cancel := context.WithTimeout(context.Background(), 45*time.Second)
		_, err := loadCodexCredentials(ctx, config, true)
		cancel()
		if err != nil {
			fmt.Fprintln(os.Stderr, "Existing Forge Codex authentication could not be verified.")
			os.Exit(1)
		}
		fmt.Println("Existing Forge Codex authentication verified.")
		return
	}
	client := &http.Client{CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }}
	gateway := newGateway(config, client, codexEndpoint, loadCodexCredentials)
	server := &http.Server{Addr: listenAddress, Handler: gateway, ReadHeaderTimeout: 5 * time.Second,
		ReadTimeout: 15 * time.Second, WriteTimeout: gateway.timeout() + 10*time.Second, IdleTimeout: time.Minute,
		MaxHeaderBytes: 64 << 10, ErrorLog: log.New(io.Discard, "", 0)}
	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt)
	defer stop()
	go func() {
		<-ctx.Done()
		shutdown, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		_ = server.Shutdown(shutdown)
	}()
	fmt.Println("Codex model API listening on http://" + listenAddress + "/v1")
	if err := server.ListenAndServe(); err != nil && !errors.Is(err, http.ErrServerClosed) {
		fmt.Fprintln(os.Stderr, "Codex model API could not listen on its loopback port")
		os.Exit(1)
	}
}

func (g *localGateway) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	w.Header().Set("X-Content-Type-Options", "nosniff")
	w.Header().Set("Cache-Control", "no-store")
	if r.URL.Path == "/healthz" && r.Method == http.MethodGet {
		writeJSON(w, http.StatusOK, []byte(`{"status":"ok"}`))
		return
	}
	if !g.authorized(r) {
		writeError(w, http.StatusUnauthorized, "invalid_api_key", "A valid local API key is required.")
		return
	}
	switch {
	case r.URL.Path == "/v1/models" && r.Method == http.MethodGet:
		models := make([]map[string]any, 0, len(g.config.Models))
		for _, id := range g.config.Models {
			models = append(models, map[string]any{"id": id, "object": "model", "created": 0, "owned_by": "rurixforge-codex"})
		}
		body, _ := json.Marshal(map[string]any{"object": "list", "data": models})
		writeJSON(w, http.StatusOK, body)
	case (r.URL.Path == "/v1/responses" || r.URL.Path == "/v1/chat/completions") && r.Method == http.MethodPost:
		g.inference(w, r)
	default:
		writeError(w, http.StatusNotFound, "not_found", "Unknown API route or method.")
	}
}

func (g *localGateway) authorized(r *http.Request) bool {
	provided := ""
	if value := r.Header.Get("Authorization"); len(value) >= 7 && strings.EqualFold(value[:7], "Bearer ") {
		provided = strings.TrimSpace(value[7:])
	}
	if provided == "" {
		provided = strings.TrimSpace(r.Header.Get("x-api-key"))
	}
	want, got := sha256.Sum256([]byte(g.config.APIKey)), sha256.Sum256([]byte(provided))
	return g.config.APIKey != "" && provided != "" && subtle.ConstantTimeCompare(want[:], got[:]) == 1
}

func (g *localGateway) inference(w http.ResponseWriter, r *http.Request) {
	select {
	case g.slots <- struct{}{}:
		defer func() { <-g.slots }()
	default:
		w.Header().Set("Retry-After", "1")
		writeError(w, 429, "concurrency_limit", "Four requests are already running.")
		return
	}
	r.Body = http.MaxBytesReader(w, r.Body, requestLimit)
	body, err := io.ReadAll(r.Body)
	if err != nil {
		var tooLarge *http.MaxBytesError
		if errors.As(err, &tooLarge) {
			writeError(w, 413, "request_too_large", "Request exceeds 8 MiB.")
		} else {
			writeError(w, 400, "invalid_request", "Cannot read request body.")
		}
		return
	}
	var head struct {
		Model         string `json:"model"`
		Stream        bool   `json:"stream"`
		StreamOptions struct {
			IncludeUsage bool `json:"include_usage"`
		} `json:"stream_options"`
	}
	if json.Unmarshal(body, &head) != nil || bytes.Equal(bytes.TrimSpace(body), []byte("null")) {
		writeError(w, 400, "invalid_request", "Request must be a JSON object with valid field types.")
		return
	}
	if head.Model == "" {
		head.Model = g.config.Model
	}
	allowed := false
	for _, model := range g.config.Models {
		if model == head.Model {
			allowed = true
			break
		}
	}
	if !allowed {
		writeError(w, 404, "model_not_found", "Choose a model returned by /v1/models.")
		return
	}
	upstreamModel := strings.TrimPrefix(head.Model, "codex/")
	chat := r.URL.Path == "/v1/chat/completions"
	if chat {
		store := false
		body, err = convert.ChatToResponsesRequest(body, convert.ChatToResponsesOptions{
			Model: upstreamModel, Instructions: defaultInstructions, Stream: true, Store: &store, DropSampling: true,
		})
	} else {
		body, err = shapeResponses(body, upstreamModel)
	}
	if err != nil {
		writeError(w, 400, "invalid_request", "Request cannot be converted to the Codex Responses protocol.")
		return
	}
	ctx, cancel := context.WithTimeout(r.Context(), g.timeout())
	defer cancel()
	credentials, err := g.credentials(ctx, g.config, false)
	if err != nil {
		upstreamDiagnostic("credentials", 0)
		writeError(w, 503, "codex_auth_unavailable", "The existing Forge Codex login is unavailable; check its sign-in state.")
		return
	}
	for attempt := 0; attempt < 2; attempt++ {
		req, err := http.NewRequestWithContext(ctx, http.MethodPost, g.upstream, bytes.NewReader(body))
		if err != nil {
			writeError(w, 500, "gateway_configuration", "Cannot create upstream request.")
			return
		}
		openai.SetCodexHeaders(req.Header, credentials.AccessToken, credentials.AccountID, "", "")
		req.Header.Set("Content-Type", "application/json")
		req.Header.Set("Accept", "text/event-stream")
		response, err := g.client.Do(req)
		if err != nil {
			upstreamDiagnostic("transport", 0)
			status, code := 502, "upstream_unavailable"
			if errors.Is(ctx.Err(), context.DeadlineExceeded) {
				status, code = 504, "upstream_timeout"
			}
			writeError(w, status, code, "Codex upstream request did not complete.")
			return
		}
		if response.StatusCode == 401 && attempt == 0 {
			upstreamDiagnostic("refresh_required", 401)
			response.Body.Close()
			credentials, err = g.credentials(ctx, g.config, true)
			if err != nil {
				upstreamDiagnostic("credential_refresh", 0)
				writeError(w, 503, "codex_auth_unavailable", "The existing Forge Codex login could not be renewed.")
				return
			}
			continue
		}
		defer response.Body.Close()
		if response.StatusCode != http.StatusOK {
			upstreamDiagnostic("http", response.StatusCode)
			status := response.StatusCode
			if status < 400 || status > 599 || status == 401 || status == 403 {
				status = 502
			}
			if retry := response.Header.Get("Retry-After"); len(retry) < 64 {
				w.Header().Set("Retry-After", retry)
			}
			writeError(w, status, "codex_upstream_error", "Codex upstream rejected the request.")
			return
		}
		contentType := strings.TrimSpace(strings.ToLower(response.Header.Get("Content-Type")))
		// The Codex backend sometimes omits Content-Type on an otherwise valid
		// SSE response. The event parser below still validates every frame.
		if contentType != "" && !strings.HasPrefix(contentType, "text/event-stream") {
			upstreamDiagnostic("response_format", response.StatusCode)
			writeError(w, 502, "invalid_upstream_response", "Codex upstream did not return an event stream.")
			return
		}
		g.relay(w, response.Body, chat, head.Stream, head.StreamOptions.IncludeUsage, head.Model)
		return
	}
}

func shapeResponses(body []byte, model string) ([]byte, error) {
	var fields map[string]json.RawMessage
	if json.Unmarshal(body, &fields) != nil || fields == nil {
		return nil, errors.New("invalid object")
	}
	set := func(key string, value any) { fields[key], _ = json.Marshal(value) }
	set("model", model)
	set("stream", true)
	set("store", false)
	for _, key := range []string{"max_output_tokens", "temperature", "top_p"} {
		delete(fields, key)
	}
	var instructions string
	if raw, ok := fields["instructions"]; !ok || string(raw) == "null" || (json.Unmarshal(raw, &instructions) == nil && instructions == "") {
		set("instructions", defaultInstructions)
	}
	var input string
	if json.Unmarshal(fields["input"], &input) == nil {
		set("input", []any{map[string]any{"type": "message", "role": "user", "content": []any{map[string]any{"type": "input_text", "text": input}}}})
	}
	var include []string
	if raw, ok := fields["include"]; ok && json.Unmarshal(raw, &include) != nil {
		return nil, errors.New("invalid include")
	}
	found := false
	for _, item := range include {
		if item == "reasoning.encrypted_content" {
			found = true
		}
	}
	if !found {
		include = append(include, "reasoning.encrypted_content")
	}
	set("include", include)
	return json.Marshal(fields)
}

func (g *localGateway) relay(w http.ResponseWriter, body io.Reader, chat, stream, includeUsage bool, model string) {
	reader := convert.NewSSEReader(io.LimitReader(body, responseLimit+1))
	collector := convert.NewResponsesCollector()
	var converter convert.ToChat
	if chat {
		converter = convert.NewResponsesToChat(model, includeUsage)
	}
	started, terminal := false, false
	writeFrame := func(frame []byte) bool {
		if len(frame) == 0 {
			return true
		}
		if !started {
			w.Header().Set("Content-Type", "text/event-stream; charset=utf-8")
			w.Header().Set("X-Accel-Buffering", "no")
			w.WriteHeader(200)
			started = true
		}
		if _, err := w.Write(frame); err != nil {
			return false
		}
		return http.NewResponseController(w).Flush() == nil
	}
	fail := func(code string) {
		upstreamDiagnostic(code, 200)
		if !started {
			writeError(w, 502, code, "Codex upstream did not complete a valid response.")
			return
		}
		payload, _ := json.Marshal(map[string]any{"type": "error", "error": map[string]any{"code": code, "message": "Codex upstream did not complete a valid response."}})
		_ = writeFrame(convert.FormatSSE("error", payload))
	}
	for {
		event, err := reader.Next()
		if errors.Is(err, io.EOF) {
			break
		}
		if err != nil {
			fail("upstream_stream_interrupted")
			return
		}
		if bytes.Equal(bytes.TrimSpace(event.Data), []byte("[DONE]")) {
			break
		}
		var header struct {
			Type string `json:"type"`
		}
		if json.Unmarshal(event.Data, &header) != nil {
			fail("invalid_upstream_event")
			return
		}
		kind := header.Type
		if kind == "" {
			kind = event.Event
		}
		if kind == "error" || kind == "response.failed" {
			fail("codex_upstream_error")
			return
		}
		if chat {
			frame, err := converter.Feed(event)
			if err != nil || converter.UpstreamError() != nil {
				fail("upstream_conversion_error")
				return
			}
			if stream && !writeFrame(frame) {
				return
			}
		} else {
			collector.Feed(event)
			if stream && !writeFrame(convert.FormatSSE(event.Event, event.Data)) {
				return
			}
		}
		if kind == "response.completed" || kind == "response.incomplete" {
			terminal = true
			break
		}
	}
	if !terminal {
		fail("upstream_stream_incomplete")
		return
	}
	if chat {
		if !converter.Done() {
			fail("upstream_stream_incomplete")
			return
		}
		tail := converter.Finish()
		if stream {
			_ = writeFrame(tail)
		} else {
			writeJSON(w, 200, converter.Completion())
		}
		return
	}
	if !stream {
		result, err := collector.Result()
		if err != nil {
			fail("upstream_stream_incomplete")
			return
		}
		writeJSON(w, 200, result)
	}
}

func writeError(w http.ResponseWriter, status int, code, message string) {
	body, _ := json.Marshal(map[string]any{"error": map[string]any{"type": "api_error", "code": code, "message": message}})
	writeJSON(w, status, body)
}

// Only fixed internal phase labels and status numbers enter diagnostics.
// Never include URLs, request bodies, upstream response text, or credentials.
func upstreamDiagnostic(phase string, status int) {
	fmt.Fprintf(os.Stderr, "codex-local-api upstream_error phase=%s status=%d\n", phase, status)
}

func writeJSON(w http.ResponseWriter, status int, body []byte) {
	w.Header().Set("Content-Type", "application/json; charset=utf-8")
	w.WriteHeader(status)
	_, _ = w.Write(body)
}
