// Package devupstream 是开发/端到端测试用的假上游，由 `forge-cloud dev fake-upstream --addr 127.0.0.1:8199` 启动。
// 所有响应都是确定性的：回复文本固定为 "fake: <最后一条用户文本>"。
//
// 路由：
//
//	POST /v1/chat/completions   OpenAI chat（流式/非流式）。用量固定 prompt_tokens=10、completion_tokens=5、
//	                            prompt_tokens_details.cached_tokens=2；流式仅在 stream_options.include_usage=true 时
//	                            追加 usage 块（其余块带 "usage":null）。带 tools 且用户文本含 "call tool" 时
//	                            返回对第一个工具的调用（arguments "{}"）。
//	POST /v1/responses          OpenAI Responses（SSE / JSON），用量 input_tokens=10（cached 2）、output_tokens=5。
//	POST /v1/messages           Anthropic Messages（SSE / JSON）；需要 x-api-key（或 Bearer）与 anthropic-version 头、
//	                            max_tokens>0。用量 input_tokens=8、cache_read_input_tokens=2、output_tokens=5。
//	POST /v1/embeddings         每条输入一个 8 维向量（由文本 SHA-256 导出），每条输入计 5 个 token。
//	GET  /v1/models             固定模型列表（gpt-fake、gpt-fake-mini、claude-fake、text-embedding-fake）。
//	POST /codex/responses       Codex 订阅上游：只接受 stream=true、store=false；需要 Authorization 与
//	                            chatgpt-account-id，否则 401。响应带 x-codex-primary-* / x-codex-secondary-* 额度头。
//	                            access token 是 JWT 且已过期或含 "fake_revoked":true 时返回 401。
//	GET  /wham/usage            Codex 额度（需要 Authorization 与 chatgpt-account-id）。
//	GET  /oauth/authorize       登记 PKCE challenge 后 302 到 redirect_uri?code=…&state=…
//	POST /oauth/token           authorization_code（表单）与 refresh_token（JSON 或表单）。id_token/access_token 为
//	                            未签名 JWT，声明含 email、https://api.openai.com/auth.{chatgpt_account_id,
//	                            chatgpt_plan_type} 与 exp。code 形如 "acct:<id>" 时账号 ID 取 <id>，否则由 code 哈希导出；
//	                            refresh token 形如 "rt_fake.<账号ID>.<序号>"，含 "invalid" 时返回 400 invalid_grant。
//
// 故障触发（便于测试换号与冷却）：
//   - 用户文本含 "trigger 429"：429（Codex 路由返回 usage_limit_reached、resets_in_seconds=120；其它路由带 Retry-After: 30）；
//   - 用户文本含 "trigger 500"：500；
//   - API Key 或 chatgpt-account-id 以 "-429" / "-500" / "-401" 结尾：该账号的每个请求都返回对应状态。
package devupstream

import (
	"context"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"log/slog"
	"net"
	"net/http"
	"net/url"
	"strconv"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	"forge-cloud/internal/oauth/openai"
)

// Request 是一条被记录的上游请求（测试用来断言网关实际发出的内容）。
type Request struct {
	Method string
	Path   string
	Header http.Header
	Body   []byte
}

const maxRecorded = 500

// Server 是假上游的 http.Handler。
type Server struct {
	log *slog.Logger
	mux *http.ServeMux

	mu    sync.Mutex
	reqs  []Request
	codes map[string]string // 授权码 → PKCE challenge

	seq atomic.Int64
}

// New 返回假上游 handler。
func New(log *slog.Logger) *Server {
	if log == nil {
		log = slog.New(slog.NewTextHandler(io.Discard, nil))
	}
	s := &Server{log: log, mux: http.NewServeMux(), codes: map[string]string{}}
	s.mux.HandleFunc("POST /v1/chat/completions", s.chat)
	s.mux.HandleFunc("POST /v1/responses", s.responses)
	s.mux.HandleFunc("POST /v1/messages", s.messages)
	s.mux.HandleFunc("POST /v1/embeddings", s.embeddings)
	s.mux.HandleFunc("GET /v1/models", s.models)
	s.mux.HandleFunc("POST /codex/responses", s.codexResponses)
	s.mux.HandleFunc("GET /wham/usage", s.whamUsage)
	s.mux.HandleFunc("GET /oauth/authorize", s.authorize)
	s.mux.HandleFunc("POST /oauth/token", s.token)
	return s
}

func (s *Server) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	body, _ := io.ReadAll(io.LimitReader(r.Body, 64<<20))
	_ = r.Body.Close()
	s.mu.Lock()
	s.reqs = append(s.reqs, Request{Method: r.Method, Path: r.URL.Path, Header: r.Header.Clone(), Body: body})
	if len(s.reqs) > maxRecorded {
		s.reqs = s.reqs[len(s.reqs)-maxRecorded:]
	}
	s.mu.Unlock()
	r.Body = io.NopCloser(strings.NewReader(string(body)))
	s.log.Debug("fake upstream", "method", r.Method, "path", r.URL.Path)
	s.mux.ServeHTTP(w, r)
}

// Requests 返回已记录的请求副本。
func (s *Server) Requests() []Request {
	s.mu.Lock()
	defer s.mu.Unlock()
	return append([]Request(nil), s.reqs...)
}

// RequestsTo 返回发往 path 的请求。
func (s *Server) RequestsTo(path string) []Request {
	var out []Request
	for _, r := range s.Requests() {
		if r.Path == path {
			out = append(out, r)
		}
	}
	return out
}

// Reset 清空请求记录。
func (s *Server) Reset() {
	s.mu.Lock()
	s.reqs = nil
	s.mu.Unlock()
}

// Run 在 addr 上启动假上游，ctx 取消时退出。
func Run(ctx context.Context, addr string, log *slog.Logger) error {
	ln, err := net.Listen("tcp", addr)
	if err != nil {
		return err
	}
	srv := &http.Server{Handler: New(log), ReadHeaderTimeout: 10 * time.Second}
	errCh := make(chan error, 1)
	go func() { errCh <- srv.Serve(ln) }()
	log.Info("fake upstream listening", "addr", ln.Addr().String(),
		"codexBaseUrl", "http://"+ln.Addr().String()+"/codex", "apiBaseUrl", "http://"+ln.Addr().String()+"/v1")
	select {
	case <-ctx.Done():
		sctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		return srv.Shutdown(sctx)
	case err := <-errCh:
		if errors.Is(err, http.ErrServerClosed) {
			return nil
		}
		return err
	}
}

// ---------- 公共工具 ----------

const (
	fakeCreated      = 1700000000
	promptTokens     = 10
	cachedTokens     = 2
	completionTokens = 5
)

func writeJSON(w http.ResponseWriter, status int, v any) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(v)
}

func openAIError(w http.ResponseWriter, status int, typ, code, msg string) {
	writeJSON(w, status, map[string]any{"error": map[string]any{"message": msg, "type": typ, "code": code, "param": nil}})
}

type sseWriter struct {
	w   http.ResponseWriter
	seq int
}

func startSSE(w http.ResponseWriter) *sseWriter {
	w.Header().Set("Content-Type", "text/event-stream")
	w.Header().Set("Cache-Control", "no-cache")
	w.WriteHeader(http.StatusOK)
	return &sseWriter{w: w}
}

// event 写一帧；event 为空时只有 data 行。
func (s *sseWriter) event(event string, v any) {
	b, _ := json.Marshal(v)
	if event != "" {
		fmt.Fprintf(s.w, "event: %s\n", event)
	}
	fmt.Fprintf(s.w, "data: %s\n\n", b)
	if f, ok := s.w.(http.Flusher); ok {
		f.Flush()
	}
}

func (s *sseWriter) raw(line string) {
	fmt.Fprint(s.w, line)
	if f, ok := s.w.(http.Flusher); ok {
		f.Flush()
	}
}

// responsesEvent 写 Responses 事件（event 行 + data.type + sequence_number）。
func (s *sseWriter) responsesEvent(typ string, fields map[string]any) {
	fields["type"] = typ
	fields["sequence_number"] = s.seq
	s.seq++
	s.event(typ, fields)
}

func bearer(r *http.Request) string {
	h := r.Header.Get("Authorization")
	if len(h) > 7 && strings.EqualFold(h[:7], "bearer ") {
		return strings.TrimSpace(h[7:])
	}
	return ""
}

// credentialFault 返回凭据后缀触发的故障状态码（0 = 无）。
func credentialFault(cred string) int {
	switch {
	case strings.HasSuffix(cred, "-429"):
		return http.StatusTooManyRequests
	case strings.HasSuffix(cred, "-500"):
		return http.StatusInternalServerError
	case strings.HasSuffix(cred, "-401"):
		return http.StatusUnauthorized
	}
	return 0
}

func textFault(text string) int {
	t := strings.ToLower(text)
	switch {
	case strings.Contains(t, "trigger 429"):
		return http.StatusTooManyRequests
	case strings.Contains(t, "trigger 500"):
		return http.StatusInternalServerError
	}
	return 0
}

// openAIFault 写 OpenAI 形状的故障响应。
func openAIFault(w http.ResponseWriter, status int) {
	switch status {
	case http.StatusTooManyRequests:
		w.Header().Set("Retry-After", "30")
		openAIError(w, status, "rate_limit_error", "rate_limit_exceeded", "Rate limit reached (fake)")
	case http.StatusUnauthorized:
		openAIError(w, status, "invalid_request_error", "invalid_api_key", "Incorrect API key provided (fake)")
	default:
		openAIError(w, status, "server_error", "server_error", "The server had an error (fake)")
	}
}

func reply(text string) string { return "fake: " + text }

// splitReply 把回复拆成两段增量。
func splitReply(s string) []string {
	if len(s) <= 6 {
		return []string{s}
	}
	return []string{s[:6], s[6:]}
}

func wantsTool(text string, tools []json.RawMessage) bool {
	return len(tools) > 0 && strings.Contains(strings.ToLower(text), "call tool")
}

// toolName 取第一个工具名（chat 形状 {function:{name}}、Responses/Anthropic 扁平形状 {name}）。
func toolName(tools []json.RawMessage) string {
	if len(tools) == 0 {
		return ""
	}
	var t struct {
		Name     string `json:"name"`
		Function struct {
			Name string `json:"name"`
		} `json:"function"`
	}
	_ = json.Unmarshal(tools[0], &t)
	if t.Function.Name != "" {
		return t.Function.Name
	}
	return t.Name
}

// contentText 取消息 content 的文本（字符串，或 text/input_text 部件拼接）。
func contentText(raw json.RawMessage) string {
	var s string
	if json.Unmarshal(raw, &s) == nil {
		return s
	}
	var parts []struct {
		Type string `json:"type"`
		Text string `json:"text"`
	}
	if json.Unmarshal(raw, &parts) != nil {
		return ""
	}
	var out []string
	for _, p := range parts {
		if (p.Type == "text" || p.Type == "input_text") && p.Text != "" {
			out = append(out, p.Text)
		}
	}
	return strings.Join(out, " ")
}

type message struct {
	Type    string          `json:"type"`
	Role    string          `json:"role"`
	Content json.RawMessage `json:"content"`
}

func lastUserText(msgs []message) string {
	for i := len(msgs) - 1; i >= 0; i-- {
		m := msgs[i]
		if m.Role == "user" && (m.Type == "" || m.Type == "message") {
			if t := contentText(m.Content); t != "" {
				return t
			}
		}
	}
	return ""
}

// ---------- OpenAI chat ----------

type chatRequest struct {
	Model         string            `json:"model"`
	Messages      []message         `json:"messages"`
	Stream        bool              `json:"stream"`
	Tools         []json.RawMessage `json:"tools"`
	StreamOptions *struct {
		IncludeUsage bool `json:"include_usage"`
	} `json:"stream_options"`
}

func chatUsage() map[string]any {
	return map[string]any{
		"prompt_tokens": promptTokens, "completion_tokens": completionTokens,
		"total_tokens":          promptTokens + completionTokens,
		"prompt_tokens_details": map[string]any{"cached_tokens": cachedTokens},
	}
}

func (s *Server) chat(w http.ResponseWriter, r *http.Request) {
	key := bearer(r)
	if key == "" {
		openAIFault(w, http.StatusUnauthorized)
		return
	}
	var req chatRequest
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil || req.Model == "" {
		openAIError(w, http.StatusBadRequest, "invalid_request_error", "invalid_request", "body must be JSON with model")
		return
	}
	text := lastUserText(req.Messages)
	if st := credentialFault(key); st != 0 {
		openAIFault(w, st)
		return
	}
	if st := textFault(text); st != 0 {
		openAIFault(w, st)
		return
	}
	id := fmt.Sprintf("chatcmpl-fake-%d", s.seq.Add(1))
	tool := ""
	if wantsTool(text, req.Tools) {
		tool = toolName(req.Tools)
	}
	if !req.Stream {
		msg := map[string]any{"role": "assistant", "content": reply(text)}
		finish := "stop"
		if tool != "" {
			msg = map[string]any{"role": "assistant", "content": nil, "tool_calls": []any{map[string]any{
				"id": "call_fake_1", "type": "function", "function": map[string]any{"name": tool, "arguments": "{}"},
			}}}
			finish = "tool_calls"
		}
		writeJSON(w, http.StatusOK, map[string]any{
			"id": id, "object": "chat.completion", "created": fakeCreated, "model": req.Model,
			"choices": []any{map[string]any{"index": 0, "message": msg, "finish_reason": finish}},
			"usage":   chatUsage(),
		})
		return
	}
	includeUsage := req.StreamOptions != nil && req.StreamOptions.IncludeUsage
	sw := startSSE(w)
	chunk := func(delta map[string]any, finish any) {
		c := map[string]any{
			"id": id, "object": "chat.completion.chunk", "created": fakeCreated, "model": req.Model,
			"choices": []any{map[string]any{"index": 0, "delta": delta, "finish_reason": finish}},
		}
		if includeUsage {
			c["usage"] = nil
		}
		sw.event("", c)
	}
	if tool != "" {
		chunk(map[string]any{"role": "assistant", "content": nil, "tool_calls": []any{map[string]any{
			"index": 0, "id": "call_fake_1", "type": "function",
			"function": map[string]any{"name": tool, "arguments": ""},
		}}}, nil)
		chunk(map[string]any{"tool_calls": []any{map[string]any{"index": 0, "function": map[string]any{"arguments": "{}"}}}}, nil)
		chunk(map[string]any{}, "tool_calls")
	} else {
		chunk(map[string]any{"role": "assistant", "content": ""}, nil)
		for _, part := range splitReply(reply(text)) {
			chunk(map[string]any{"content": part}, nil)
		}
		chunk(map[string]any{}, "stop")
	}
	if includeUsage {
		sw.event("", map[string]any{
			"id": id, "object": "chat.completion.chunk", "created": fakeCreated, "model": req.Model,
			"choices": []any{}, "usage": chatUsage(),
		})
	}
	sw.raw("data: [DONE]\n\n")
}

// ---------- OpenAI Responses / Codex ----------

type responsesRequest struct {
	Model        string            `json:"model"`
	Input        json.RawMessage   `json:"input"`
	Stream       bool              `json:"stream"`
	Store        *bool             `json:"store"`
	Tools        []json.RawMessage `json:"tools"`
	Instructions string            `json:"instructions"`
}

func (req *responsesRequest) userText() string {
	var s string
	if json.Unmarshal(req.Input, &s) == nil {
		return s
	}
	var items []message
	_ = json.Unmarshal(req.Input, &items)
	return lastUserText(items)
}

func responsesUsage() map[string]any {
	return map[string]any{
		"input_tokens": promptTokens, "input_tokens_details": map[string]any{"cached_tokens": cachedTokens},
		"output_tokens": completionTokens, "output_tokens_details": map[string]any{"reasoning_tokens": 0},
		"total_tokens": promptTokens + completionTokens,
	}
}

func (s *Server) responses(w http.ResponseWriter, r *http.Request) {
	key := bearer(r)
	if key == "" {
		openAIFault(w, http.StatusUnauthorized)
		return
	}
	var req responsesRequest
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil || req.Model == "" {
		openAIError(w, http.StatusBadRequest, "invalid_request_error", "invalid_request", "body must be JSON with model")
		return
	}
	text := req.userText()
	if st := credentialFault(key); st != 0 {
		openAIFault(w, st)
		return
	}
	if st := textFault(text); st != 0 {
		openAIFault(w, st)
		return
	}
	s.writeResponses(w, &req, text, req.Stream)
}

func codexRateHeaders(h http.Header) {
	h.Set("x-codex-primary-used-percent", "12.5")
	h.Set("x-codex-primary-window-minutes", "300")
	h.Set("x-codex-primary-reset-after-seconds", "3600")
	h.Set("x-codex-secondary-used-percent", "3")
	h.Set("x-codex-secondary-window-minutes", "10080")
	h.Set("x-codex-secondary-reset-after-seconds", "86400")
}

func codexDetail(w http.ResponseWriter, status int, msg string) {
	writeJSON(w, status, map[string]any{"detail": msg})
}

// codexTokenRejected：access token 是 JWT 且已过期或带 fake_revoked 声明。
func codexTokenRejected(token string) bool {
	claims, err := openai.DecodeJWT(token)
	if err != nil {
		return false
	}
	if v, ok := claims["fake_revoked"].(bool); ok && v {
		return true
	}
	if n, ok := claims["exp"].(json.Number); ok {
		if exp, err := n.Int64(); err == nil && exp < time.Now().Unix() {
			return true
		}
	}
	return false
}

func (s *Server) codexResponses(w http.ResponseWriter, r *http.Request) {
	token, account := bearer(r), r.Header.Get("chatgpt-account-id")
	if token == "" || account == "" {
		codexDetail(w, http.StatusUnauthorized, "Unauthorized")
		return
	}
	if codexTokenRejected(token) || credentialFault(account) == http.StatusUnauthorized {
		codexDetail(w, http.StatusUnauthorized, "Your authentication token has expired. Please try signing in again.")
		return
	}
	var req responsesRequest
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil || req.Model == "" {
		codexDetail(w, http.StatusBadRequest, "Invalid request body")
		return
	}
	if !req.Stream {
		codexDetail(w, http.StatusBadRequest, "Stream must be set to true")
		return
	}
	if req.Store == nil || *req.Store {
		codexDetail(w, http.StatusBadRequest, "Store must be set to false")
		return
	}
	codexRateHeaders(w.Header())
	text := req.userText()
	fault := credentialFault(account)
	if fault == 0 {
		fault = textFault(text)
	}
	switch fault {
	case http.StatusTooManyRequests:
		w.Header().Set("x-codex-primary-used-percent", "100")
		w.Header().Set("x-codex-primary-reset-after-seconds", "120")
		writeJSON(w, http.StatusTooManyRequests, map[string]any{"error": map[string]any{
			"type": "usage_limit_reached", "message": "limit", "resets_in_seconds": 120,
		}})
		return
	case http.StatusInternalServerError:
		codexDetail(w, http.StatusInternalServerError, "Internal server error (fake)")
		return
	}
	s.writeResponses(w, &req, text, true)
}

func (s *Server) writeResponses(w http.ResponseWriter, req *responsesRequest, text string, stream bool) {
	n := s.seq.Add(1)
	respID := fmt.Sprintf("resp_fake_%d", n)
	full := reply(text)
	tool := ""
	if wantsTool(text, req.Tools) {
		tool = toolName(req.Tools)
	}
	var item map[string]any
	if tool != "" {
		item = map[string]any{
			"id": fmt.Sprintf("fc_fake_%d", n), "type": "function_call", "status": "completed",
			"call_id": "call_fake_1", "name": tool, "arguments": "{}",
		}
	} else {
		item = map[string]any{
			"id": fmt.Sprintf("msg_fake_%d", n), "type": "message", "status": "completed", "role": "assistant",
			"content": []any{map[string]any{"type": "output_text", "text": full, "annotations": []any{}}},
		}
	}
	response := func(status string, output []any, usage any) map[string]any {
		return map[string]any{
			"id": respID, "object": "response", "created_at": fakeCreated, "status": status,
			"model": req.Model, "output": output, "usage": usage,
		}
	}
	if !stream {
		writeJSON(w, http.StatusOK, response("completed", []any{item}, responsesUsage()))
		return
	}
	sw := startSSE(w)
	sw.responsesEvent("response.created", map[string]any{"response": response("in_progress", []any{}, nil)})
	sw.responsesEvent("response.in_progress", map[string]any{"response": response("in_progress", []any{}, nil)})
	itemID := item["id"]
	if tool != "" {
		added := map[string]any{"id": itemID, "type": "function_call", "status": "in_progress",
			"call_id": "call_fake_1", "name": tool, "arguments": ""}
		sw.responsesEvent("response.output_item.added", map[string]any{"output_index": 0, "item": added})
		sw.responsesEvent("response.function_call_arguments.delta", map[string]any{"item_id": itemID, "output_index": 0, "delta": "{}"})
		sw.responsesEvent("response.function_call_arguments.done", map[string]any{"item_id": itemID, "output_index": 0, "arguments": "{}"})
	} else {
		added := map[string]any{"id": itemID, "type": "message", "status": "in_progress", "role": "assistant", "content": []any{}}
		sw.responsesEvent("response.output_item.added", map[string]any{"output_index": 0, "item": added})
		part := func(t string) map[string]any {
			return map[string]any{"type": "output_text", "text": t, "annotations": []any{}}
		}
		sw.responsesEvent("response.content_part.added", map[string]any{"item_id": itemID, "output_index": 0, "content_index": 0, "part": part("")})
		for _, d := range splitReply(full) {
			sw.responsesEvent("response.output_text.delta", map[string]any{"item_id": itemID, "output_index": 0, "content_index": 0, "delta": d})
		}
		sw.responsesEvent("response.output_text.done", map[string]any{"item_id": itemID, "output_index": 0, "content_index": 0, "text": full})
		sw.responsesEvent("response.content_part.done", map[string]any{"item_id": itemID, "output_index": 0, "content_index": 0, "part": part(full)})
	}
	sw.responsesEvent("response.output_item.done", map[string]any{"output_index": 0, "item": item})
	sw.responsesEvent("response.completed", map[string]any{"response": response("completed", []any{item}, responsesUsage())})
}

// ---------- Anthropic Messages ----------

type messagesRequest struct {
	Model     string            `json:"model"`
	Messages  []message         `json:"messages"`
	MaxTokens int               `json:"max_tokens"`
	Stream    bool              `json:"stream"`
	Tools     []json.RawMessage `json:"tools"`
}

func anthropicError(w http.ResponseWriter, status int, typ, msg string) {
	writeJSON(w, status, map[string]any{"type": "error", "error": map[string]any{"type": typ, "message": msg}})
}

func anthropicFault(w http.ResponseWriter, status int) {
	switch status {
	case http.StatusTooManyRequests:
		w.Header().Set("Retry-After", "30")
		anthropicError(w, status, "rate_limit_error", "rate limited (fake)")
	case http.StatusUnauthorized:
		anthropicError(w, status, "authentication_error", "invalid x-api-key (fake)")
	default:
		anthropicError(w, status, "api_error", "internal error (fake)")
	}
}

func messagesUsage(outputTokens int) map[string]any {
	return map[string]any{
		"input_tokens": promptTokens - cachedTokens, "cache_read_input_tokens": cachedTokens,
		"cache_creation_input_tokens": 0, "output_tokens": outputTokens,
	}
}

func (s *Server) messages(w http.ResponseWriter, r *http.Request) {
	key := r.Header.Get("x-api-key")
	if key == "" {
		key = bearer(r)
	}
	if key == "" {
		anthropicFault(w, http.StatusUnauthorized)
		return
	}
	if r.Header.Get("anthropic-version") == "" {
		anthropicError(w, http.StatusBadRequest, "invalid_request_error", "anthropic-version header is required")
		return
	}
	var req messagesRequest
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil || req.Model == "" {
		anthropicError(w, http.StatusBadRequest, "invalid_request_error", "body must be JSON with model")
		return
	}
	if req.MaxTokens <= 0 {
		anthropicError(w, http.StatusBadRequest, "invalid_request_error", "max_tokens: Field required")
		return
	}
	text := lastUserText(req.Messages)
	if st := credentialFault(key); st != 0 {
		anthropicFault(w, st)
		return
	}
	if st := textFault(text); st != 0 {
		anthropicFault(w, st)
		return
	}
	id := fmt.Sprintf("msg_fake_%d", s.seq.Add(1))
	full := reply(text)
	tool := ""
	if wantsTool(text, req.Tools) {
		tool = toolName(req.Tools)
	}
	stop := "end_turn"
	block := map[string]any{"type": "text", "text": full}
	if tool != "" {
		stop = "tool_use"
		block = map[string]any{"type": "tool_use", "id": "toolu_fake_1", "name": tool, "input": map[string]any{}}
	}
	if !req.Stream {
		writeJSON(w, http.StatusOK, map[string]any{
			"id": id, "type": "message", "role": "assistant", "model": req.Model,
			"content": []any{block}, "stop_reason": stop, "stop_sequence": nil, "usage": messagesUsage(completionTokens),
		})
		return
	}
	sw := startSSE(w)
	sw.event("message_start", map[string]any{"type": "message_start", "message": map[string]any{
		"id": id, "type": "message", "role": "assistant", "model": req.Model, "content": []any{},
		"stop_reason": nil, "stop_sequence": nil, "usage": messagesUsage(1),
	}})
	if tool != "" {
		sw.event("content_block_start", map[string]any{"type": "content_block_start", "index": 0,
			"content_block": map[string]any{"type": "tool_use", "id": "toolu_fake_1", "name": tool, "input": map[string]any{}}})
		sw.event("ping", map[string]any{"type": "ping"})
		sw.event("content_block_delta", map[string]any{"type": "content_block_delta", "index": 0,
			"delta": map[string]any{"type": "input_json_delta", "partial_json": "{}"}})
	} else {
		sw.event("content_block_start", map[string]any{"type": "content_block_start", "index": 0,
			"content_block": map[string]any{"type": "text", "text": ""}})
		sw.event("ping", map[string]any{"type": "ping"})
		for _, d := range splitReply(full) {
			sw.event("content_block_delta", map[string]any{"type": "content_block_delta", "index": 0,
				"delta": map[string]any{"type": "text_delta", "text": d}})
		}
	}
	sw.event("content_block_stop", map[string]any{"type": "content_block_stop", "index": 0})
	sw.event("message_delta", map[string]any{"type": "message_delta",
		"delta": map[string]any{"stop_reason": stop, "stop_sequence": nil},
		"usage": map[string]any{"output_tokens": completionTokens}})
	sw.event("message_stop", map[string]any{"type": "message_stop"})
}

// ---------- 嵌入与模型列表 ----------

func (s *Server) embeddings(w http.ResponseWriter, r *http.Request) {
	key := bearer(r)
	if key == "" {
		openAIFault(w, http.StatusUnauthorized)
		return
	}
	if st := credentialFault(key); st != 0 {
		openAIFault(w, st)
		return
	}
	var req struct {
		Model string          `json:"model"`
		Input json.RawMessage `json:"input"`
	}
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil || req.Model == "" {
		openAIError(w, http.StatusBadRequest, "invalid_request_error", "invalid_request", "body must be JSON with model")
		return
	}
	var inputs []string
	var one string
	if json.Unmarshal(req.Input, &one) == nil {
		inputs = []string{one}
	} else if json.Unmarshal(req.Input, &inputs) != nil || len(inputs) == 0 {
		inputs = []string{string(req.Input)}
	}
	data := make([]any, len(inputs))
	for i, in := range inputs {
		sum := sha256.Sum256([]byte(in))
		vec := make([]float64, 8)
		for j := range vec {
			vec[j] = float64(sum[j]) / 255
		}
		data[i] = map[string]any{"object": "embedding", "index": i, "embedding": vec}
	}
	tokens := 5 * len(inputs)
	writeJSON(w, http.StatusOK, map[string]any{
		"object": "list", "data": data, "model": req.Model,
		"usage": map[string]any{"prompt_tokens": tokens, "total_tokens": tokens},
	})
}

func (s *Server) models(w http.ResponseWriter, r *http.Request) {
	if bearer(r) == "" && r.Header.Get("x-api-key") == "" {
		openAIFault(w, http.StatusUnauthorized)
		return
	}
	var data []any
	for _, id := range []string{"gpt-fake", "gpt-fake-mini", "claude-fake", "text-embedding-fake"} {
		data = append(data, map[string]any{"id": id, "object": "model", "created": fakeCreated, "owned_by": "devupstream"})
	}
	writeJSON(w, http.StatusOK, map[string]any{"object": "list", "data": data})
}

func (s *Server) whamUsage(w http.ResponseWriter, r *http.Request) {
	if bearer(r) == "" || r.Header.Get("chatgpt-account-id") == "" {
		codexDetail(w, http.StatusUnauthorized, "Unauthorized")
		return
	}
	if codexTokenRejected(bearer(r)) {
		codexDetail(w, http.StatusUnauthorized, "Your authentication token has expired. Please try signing in again.")
		return
	}
	codexRateHeaders(w.Header())
	writeJSON(w, http.StatusOK, map[string]any{
		"plan_type": "plus",
		"rate_limit": map[string]any{
			"allowed": true, "limit_reached": false,
			"primary_window": map[string]any{
				"used_percent": 12, "limit_window_seconds": 18000, "reset_after_seconds": 3600, "reset_at": 1900000000,
			},
			"secondary_window": map[string]any{
				"used_percent": 3, "limit_window_seconds": 604800, "reset_after_seconds": 86400, "reset_at": 1900082400,
			},
		},
		"credits": map[string]any{"has_credits": false, "unlimited": false, "balance": "0"},
	})
}

// ---------- OAuth ----------

func (s *Server) authorize(w http.ResponseWriter, r *http.Request) {
	q := r.URL.Query()
	if q.Get("client_id") != openai.ClientID || q.Get("response_type") != "code" || q.Get("code_challenge_method") != "S256" {
		http.Error(w, "invalid authorize request", http.StatusBadRequest)
		return
	}
	redirect, err := url.Parse(q.Get("redirect_uri"))
	if err != nil || redirect.Scheme == "" {
		http.Error(w, "invalid redirect_uri", http.StatusBadRequest)
		return
	}
	code := fmt.Sprintf("fakecode-%d", s.seq.Add(1))
	s.mu.Lock()
	s.codes[code] = q.Get("code_challenge")
	s.mu.Unlock()
	rq := redirect.Query()
	rq.Set("code", code)
	rq.Set("state", q.Get("state"))
	redirect.RawQuery = rq.Encode()
	http.Redirect(w, r, redirect.String(), http.StatusFound)
}

func tokenError(w http.ResponseWriter, status int, code, desc string) {
	writeJSON(w, status, map[string]any{"error": code, "error_description": desc})
}

func (s *Server) token(w http.ResponseWriter, r *http.Request) {
	params := map[string]string{}
	if strings.HasPrefix(r.Header.Get("Content-Type"), "application/json") {
		var m map[string]any
		if err := json.NewDecoder(r.Body).Decode(&m); err != nil {
			tokenError(w, http.StatusBadRequest, "invalid_request", "body must be JSON")
			return
		}
		for k, v := range m {
			if sv, ok := v.(string); ok {
				params[k] = sv
			}
		}
	} else {
		if err := r.ParseForm(); err != nil {
			tokenError(w, http.StatusBadRequest, "invalid_request", "bad form body")
			return
		}
		for k := range r.PostForm {
			params[k] = r.PostForm.Get(k)
		}
	}
	if params["client_id"] != openai.ClientID {
		tokenError(w, http.StatusUnauthorized, "invalid_client", "unknown client_id")
		return
	}
	var accountID string
	var gen int
	switch params["grant_type"] {
	case "authorization_code":
		code := params["code"]
		if code == "" || params["code_verifier"] == "" || params["redirect_uri"] != openai.RedirectURI {
			tokenError(w, http.StatusBadRequest, "invalid_request", "code, code_verifier and redirect_uri are required")
			return
		}
		if strings.Contains(code, "bad") {
			tokenError(w, http.StatusBadRequest, "invalid_grant", "authorization code is invalid")
			return
		}
		s.mu.Lock()
		challenge, known := s.codes[code]
		delete(s.codes, code)
		s.mu.Unlock()
		if known && openai.S256Challenge(params["code_verifier"]) != challenge {
			tokenError(w, http.StatusBadRequest, "invalid_grant", "PKCE verification failed")
			return
		}
		if id, ok := strings.CutPrefix(code, "acct:"); ok && id != "" {
			accountID = id
		} else {
			sum := sha256.Sum256([]byte(code))
			accountID = "acct-" + hex.EncodeToString(sum[:])[:12]
		}
		gen = 1
	case "refresh_token":
		rt := params["refresh_token"]
		parts := strings.Split(rt, ".")
		if strings.Contains(rt, "invalid") || len(parts) != 3 || parts[0] != "rt_fake" || parts[1] == "" {
			tokenError(w, http.StatusBadRequest, "invalid_grant", "refresh token is invalid")
			return
		}
		n, err := strconv.Atoi(parts[2])
		if err != nil {
			tokenError(w, http.StatusBadRequest, "invalid_grant", "refresh token is invalid")
			return
		}
		accountID, gen = parts[1], n+1
	default:
		tokenError(w, http.StatusBadRequest, "unsupported_grant_type", "unsupported grant_type")
		return
	}
	writeJSON(w, http.StatusOK, s.issueTokens(accountID, gen))
}

// IssueTokens 生成一组与 /oauth/token 相同形状的假 token（测试可直接用来构造 auth.json）。
func IssueTokens(accountID string, gen int, accessTTL time.Duration) map[string]any {
	now := time.Now()
	email := accountID + "@fake.local"
	auth := map[string]any{"chatgpt_account_id": accountID, "chatgpt_plan_type": "plus", "chatgpt_user_id": "user-" + accountID}
	nonce := fmt.Sprintf("%d-%d", gen, now.UnixNano())
	idToken := UnsignedJWT(map[string]any{
		"iss": "https://auth.openai.com", "aud": []string{openai.ClientID}, "sub": "user-" + accountID,
		"email": email, "email_verified": true, "iat": now.Unix(), "exp": now.Add(time.Hour).Unix(),
		openai.AuthClaim: auth,
	})
	accessToken := UnsignedJWT(map[string]any{
		"iss": "https://auth.openai.com", "sub": "user-" + accountID, "iat": now.Unix(),
		"exp": now.Add(accessTTL).Unix(), "jti": nonce,
		openai.AuthClaim: auth, openai.ProfileClaim: map[string]any{"email": email},
	})
	return map[string]any{
		"id_token": idToken, "access_token": accessToken,
		"refresh_token": fmt.Sprintf("rt_fake.%s.%d", accountID, gen),
		"expires_in":    int(accessTTL.Seconds()), "token_type": "Bearer",
	}
}

func (s *Server) issueTokens(accountID string, gen int) map[string]any {
	return IssueTokens(accountID, gen, 10*24*time.Hour)
}

// UnsignedJWT 生成 alg=none 的 JWT（只供假上游与测试使用）。
func UnsignedJWT(claims map[string]any) string {
	enc := base64.RawURLEncoding
	header, _ := json.Marshal(map[string]any{"alg": "none", "typ": "JWT"})
	payload, _ := json.Marshal(claims)
	return enc.EncodeToString(header) + "." + enc.EncodeToString(payload) + "."
}
