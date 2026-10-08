package accounts

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"sort"
	"strings"
	"time"

	"forge-cloud/internal/config"
	"forge-cloud/internal/core"
	"forge-cloud/internal/httpx"
	"forge-cloud/internal/oauth/openai"
)

// ---------- 额度 ----------

// FetchQuota 调 `GET {chatgpt}/wham/usage`，原样存入 quota.usage 与 quota.fetchedAt（仅 Codex 订阅账号）。
func (s *Service) FetchQuota(ctx context.Context, id int64) (*Account, error) {
	a, err := s.Get(ctx, id)
	if err != nil {
		return nil, err
	}
	if !a.IsCodex() {
		return nil, core.BadRequest("NOT_SUPPORTED", "只有 Codex 订阅账号支持额度查询")
	}
	creds, err := s.FreshCredentials(ctx, a)
	if err != nil {
		return nil, err
	}
	client, err := s.HTTPClient(a)
	if err != nil {
		return nil, err
	}
	fetch := func(c *Credentials) (*http.Response, []byte, error) {
		rctx, cancel := context.WithTimeout(ctx, 30*time.Second)
		defer cancel()
		req, err := http.NewRequestWithContext(rctx, http.MethodGet, s.cfg.ChatGPTBaseURL+"/wham/usage", nil)
		if err != nil {
			return nil, nil, err
		}
		openai.SetCodexHeaders(req.Header, c.AccessToken, c.AccountID, "", "")
		req.Header.Set("Accept", "application/json")
		resp, err := client.Do(req)
		if err != nil {
			return nil, nil, err
		}
		defer resp.Body.Close()
		body, err := io.ReadAll(io.LimitReader(resp.Body, 1<<20))
		return resp, body, err
	}
	resp, body, err := fetch(creds)
	if err == nil && resp.StatusCode == http.StatusUnauthorized {
		if creds, err = s.RefreshAfterUnauthorized(ctx, a, creds.AccessToken); err != nil {
			return nil, err
		}
		resp, body, err = fetch(creds)
	}
	if err != nil {
		return nil, core.E(http.StatusBadGateway, "QUOTA_FETCH_FAILED", "额度查询失败："+err.Error())
	}
	_ = s.RecordCodexQuota(ctx, a.ID, resp.Header)
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return nil, core.E(http.StatusBadGateway, "QUOTA_FETCH_FAILED",
			fmt.Sprintf("额度查询失败：HTTP %d %s", resp.StatusCode, upstreamErrorMessage(body)))
	}
	if !json.Valid(body) {
		return nil, core.E(http.StatusBadGateway, "QUOTA_FETCH_FAILED", "额度查询返回的不是 JSON")
	}
	patch, _ := json.Marshal(map[string]any{"usage": json.RawMessage(body), "fetchedAt": time.Now().UTC()})
	if _, err := s.db.Exec(ctx, `UPDATE upstream_accounts SET quota = quota || $2::jsonb, updated_at = now() WHERE id = $1`,
		a.ID, patch); err != nil {
		return nil, err
	}
	return s.Get(ctx, a.ID)
}

func (s *Service) handleQuota(w http.ResponseWriter, r *http.Request) {
	id, err := httpx.PathInt64(r, "id")
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	if _, err := s.FetchQuota(r.Context(), id); err != nil {
		httpx.WriteError(w, err)
		return
	}
	s.audit(r, "account.quota", accountTarget(id), nil)
	s.writeAccount(w, r, id)
}

// ---------- 连通性测试 ----------

// TestResult 是 /accounts/{id}/test 的响应。
type TestResult struct {
	OK         bool   `json:"ok"`
	LatencyMs  int    `json:"latencyMs"`
	HTTPStatus int    `json:"httpStatus"`
	Message    string `json:"message"`
}

// testModel：请求里的模型（目录 ID 按账号映射解析，否则按上游模型名原样用）；
// 缺省取该平台的默认/第一个启用模型。
func (s *Service) testModel(ctx context.Context, a *Account, model string) string {
	model = strings.TrimSpace(model)
	if model != "" {
		if m, err := s.Model(ctx, model); err == nil {
			return a.UpstreamModel(m)
		}
		if v := strings.TrimSpace(a.ModelMapping[model]); v != "" {
			return v
		}
		return model
	}
	if list, err := s.EnabledModels(ctx); err == nil {
		var first *CatalogModel
		for i := range list {
			m := &list[i]
			if m.Platform != a.Platform || (a.IsCodex() && !m.Capabilities.Responses) {
				continue
			}
			if m.IsDefault {
				return a.UpstreamModel(&m.Model)
			}
			if first == nil {
				first = m
			}
		}
		if first != nil {
			return a.UpstreamModel(&first.Model)
		}
	}
	switch {
	case a.IsCodex():
		return "gpt-5"
	case a.Platform == PlatformAnthropic:
		return "claude-sonnet-4-5"
	}
	return "gpt-4o-mini"
}

// TestAccount 发一次最小真实请求（apikey：max_tokens=1；Codex：读到第一条流事件）。
func (s *Service) TestAccount(ctx context.Context, id int64, model string) (*TestResult, error) {
	a, err := s.Get(ctx, id)
	if err != nil {
		return nil, err
	}
	creds, err := s.FreshCredentials(ctx, a)
	if err != nil {
		msg := err.Error()
		if e := core.AsError(err); e != nil {
			msg = e.Message
		}
		return &TestResult{Message: msg}, nil
	}
	client, err := s.HTTPClient(a)
	if err != nil {
		return &TestResult{Message: err.Error()}, nil
	}
	up := s.testModel(ctx, a, model)
	ctx, cancel := context.WithTimeout(ctx, 60*time.Second)
	defer cancel()

	switch {
	case a.IsCodex():
		st, _ := s.settings.Get(ctx)
		body, _ := json.Marshal(map[string]any{
			"model":        up,
			"instructions": st.CodexInstructions,
			"input": []any{map[string]any{
				"type": "message", "role": "user",
				"content": []any{map[string]any{"type": "input_text", "text": "ping"}},
			}},
			"stream": true,
			"store":  false,
		})
		return s.probeCodex(ctx, a, client, creds, body), nil
	case a.Platform == PlatformAnthropic:
		body, _ := json.Marshal(map[string]any{
			"model": up, "max_tokens": 1,
			"messages": []any{map[string]any{"role": "user", "content": "ping"}},
		})
		return s.probeJSON(ctx, client, a, creds, "/messages", body), nil
	default:
		body, _ := json.Marshal(map[string]any{
			"model": up, "max_tokens": 1,
			"messages": []any{map[string]any{"role": "user", "content": "ping"}},
		})
		res := s.probeJSON(ctx, client, a, creds, "/chat/completions", body)
		// 推理模型不接受 max_tokens，改用 max_completion_tokens 再试一次。
		if res.HTTPStatus == http.StatusBadRequest && strings.Contains(res.Message, "max_completion_tokens") {
			body, _ = json.Marshal(map[string]any{
				"model": up, "max_completion_tokens": 1,
				"messages": []any{map[string]any{"role": "user", "content": "ping"}},
			})
			res = s.probeJSON(ctx, client, a, creds, "/chat/completions", body)
		}
		return res, nil
	}
}

// SetAPIKeyHeaders 写 apikey 账号的鉴权头（OpenAI：Bearer；Anthropic：x-api-key + anthropic-version）。
func SetAPIKeyHeaders(h http.Header, a *Account, apiKey string) {
	if a.Platform == PlatformAnthropic {
		h.Set("x-api-key", apiKey)
		if h.Get("anthropic-version") == "" {
			h.Set("anthropic-version", AnthropicVersion)
		}
		return
	}
	h.Set("Authorization", "Bearer "+apiKey)
}

// UserAgent 是访问 apikey 上游时的 User-Agent。
func UserAgent() string { return "forge-cloud/" + config.Version }

func (s *Service) probeJSON(ctx context.Context, client *http.Client, a *Account, creds *Credentials, path string, body []byte) *TestResult {
	start := time.Now()
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, a.BaseURL+path, bytes.NewReader(body))
	if err != nil {
		return &TestResult{Message: err.Error()}
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Accept", "application/json")
	req.Header.Set("User-Agent", UserAgent())
	SetAPIKeyHeaders(req.Header, a, creds.APIKey)
	resp, err := client.Do(req)
	if err != nil {
		return &TestResult{LatencyMs: int(time.Since(start).Milliseconds()), Message: "请求失败：" + err.Error()}
	}
	defer resp.Body.Close()
	raw, _ := io.ReadAll(io.LimitReader(resp.Body, 256<<10))
	res := &TestResult{LatencyMs: int(time.Since(start).Milliseconds()), HTTPStatus: resp.StatusCode}
	if resp.StatusCode >= 200 && resp.StatusCode < 300 {
		res.OK, res.Message = true, "OK"
	} else {
		res.Message = upstreamErrorMessage(raw)
	}
	return res
}

func (s *Service) probeCodex(ctx context.Context, a *Account, client *http.Client, creds *Credentials, body []byte) *TestResult {
	start := time.Now()
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, s.cfg.CodexBaseURL+"/responses", bytes.NewReader(body))
	if err != nil {
		return &TestResult{Message: err.Error()}
	}
	openai.SetCodexHeaders(req.Header, creds.AccessToken, creds.AccountID, "probe-"+core.RandomString(12), "")
	req.Header.Set("Accept", "text/event-stream")
	req.Header.Set("Content-Type", "application/json")
	resp, err := client.Do(req)
	if err != nil {
		return &TestResult{LatencyMs: int(time.Since(start).Milliseconds()), Message: "请求失败：" + err.Error()}
	}
	defer resp.Body.Close()
	_ = s.RecordCodexQuota(context.WithoutCancel(ctx), a.ID, resp.Header)
	res := &TestResult{HTTPStatus: resp.StatusCode}
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		raw, _ := io.ReadAll(io.LimitReader(resp.Body, 256<<10))
		res.LatencyMs = int(time.Since(start).Milliseconds())
		res.Message = upstreamErrorMessage(raw)
		return res
	}
	event, data, err := firstSSEEvent(resp.Body)
	res.LatencyMs = int(time.Since(start).Milliseconds())
	switch {
	case err != nil:
		res.Message = "读取流式响应失败：" + err.Error()
	case event == "error" || event == "response.failed" || strings.Contains(event, "error"):
		res.Message = upstreamErrorMessage(data)
	default:
		res.OK, res.Message = true, "OK（首个事件 "+event+"）"
	}
	return res
}

// firstSSEEvent 读取第一条带 data 的 SSE 事件，返回事件名（缺省取 data.type）与 data。
func firstSSEEvent(r io.Reader) (string, []byte, error) {
	buf := make([]byte, 0, 4096)
	chunk := make([]byte, 4096)
	for len(buf) < 1<<20 {
		n, err := r.Read(chunk)
		buf = append(buf, chunk[:n]...)
		norm := bytes.ReplaceAll(buf, []byte("\r\n"), []byte("\n"))
		if i := bytes.Index(norm, []byte("\n\n")); i >= 0 {
			var event string
			var data [][]byte
			for _, line := range bytes.Split(norm[:i], []byte("\n")) {
				switch {
				case bytes.HasPrefix(line, []byte("event:")):
					event = strings.TrimSpace(string(line[6:]))
				case bytes.HasPrefix(line, []byte("data:")):
					data = append(data, bytes.TrimPrefix(line[5:], []byte(" ")))
				}
			}
			d := bytes.Join(data, []byte("\n"))
			if event == "" {
				var t struct {
					Type string `json:"type"`
				}
				_ = json.Unmarshal(d, &t)
				event = t.Type
			}
			return event, d, nil
		}
		if err != nil {
			if err == io.EOF {
				return "", nil, fmt.Errorf("流在首个事件前结束")
			}
			return "", nil, err
		}
	}
	return "", nil, fmt.Errorf("首个事件过大")
}

func (s *Service) handleTest(w http.ResponseWriter, r *http.Request) {
	id, err := httpx.PathInt64(r, "id")
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	var in struct {
		Model string `json:"model"`
	}
	if r.ContentLength != 0 {
		if err := httpx.DecodeJSON(r, &in, 0); err != nil {
			httpx.WriteError(w, err)
			return
		}
	}
	res, err := s.TestAccount(r.Context(), id, in.Model)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	httpx.WriteJSON(w, http.StatusOK, res)
}

// ---------- 上游模型列表 ----------

// UpstreamModels 拉取 apikey 账号的 `{baseUrl}/models`（OAuth 账号不支持）。
func (s *Service) UpstreamModels(ctx context.Context, id int64) ([]string, error) {
	a, err := s.Get(ctx, id)
	if err != nil {
		return nil, err
	}
	if a.AuthType != AuthAPIKey {
		return nil, core.BadRequest("NOT_SUPPORTED", "OAuth 账号不支持拉取模型列表")
	}
	creds, err := s.Credentials(a)
	if err != nil {
		return nil, err
	}
	client, err := s.HTTPClient(a)
	if err != nil {
		return nil, err
	}
	ctx, cancel := context.WithTimeout(ctx, 30*time.Second)
	defer cancel()
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, a.BaseURL+"/models", nil)
	if err != nil {
		return nil, err
	}
	req.Header.Set("Accept", "application/json")
	req.Header.Set("User-Agent", UserAgent())
	SetAPIKeyHeaders(req.Header, a, creds.APIKey)
	resp, err := client.Do(req)
	if err != nil {
		return nil, core.E(http.StatusBadGateway, core.CodeUpstreamError, "拉取模型列表失败："+err.Error())
	}
	defer resp.Body.Close()
	raw, _ := io.ReadAll(io.LimitReader(resp.Body, 8<<20))
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return nil, core.E(http.StatusBadGateway, core.CodeUpstreamError,
			fmt.Sprintf("拉取模型列表失败：HTTP %d %s", resp.StatusCode, upstreamErrorMessage(raw)))
	}
	var list struct {
		Data []struct {
			ID string `json:"id"`
		} `json:"data"`
	}
	if err := json.Unmarshal(raw, &list); err != nil {
		return nil, core.E(http.StatusBadGateway, core.CodeUpstreamError, "模型列表不是合法 JSON")
	}
	ids := make([]string, 0, len(list.Data))
	seen := map[string]bool{}
	for _, m := range list.Data {
		if m.ID != "" && !seen[m.ID] {
			seen[m.ID] = true
			ids = append(ids, m.ID)
		}
	}
	sort.Strings(ids)
	return ids, nil
}

func (s *Service) handleUpstreamModels(w http.ResponseWriter, r *http.Request) {
	id, err := httpx.PathInt64(r, "id")
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	ids, err := s.UpstreamModels(r.Context(), id)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	httpx.WriteJSON(w, http.StatusOK, map[string]any{"items": ids})
}

// upstreamErrorMessage 从上游错误体里取可读信息（OpenAI / Anthropic / Codex detail），否则截断原文。
func upstreamErrorMessage(body []byte) string {
	var v struct {
		Error   json.RawMessage `json:"error"`
		Detail  json.RawMessage `json:"detail"`
		Message string          `json:"message"`
	}
	if json.Unmarshal(body, &v) == nil {
		var s string
		if json.Unmarshal(v.Error, &s) == nil && s != "" {
			return s
		}
		var o struct {
			Message string `json:"message"`
			Type    string `json:"type"`
			Code    string `json:"code"`
		}
		if json.Unmarshal(v.Error, &o) == nil && firstNonEmpty(o.Message, o.Type, o.Code) != "" {
			return firstNonEmpty(o.Message, o.Type, o.Code)
		}
		if json.Unmarshal(v.Detail, &s) == nil && s != "" {
			return s
		}
		if v.Message != "" {
			return v.Message
		}
	}
	msg := strings.TrimSpace(string(body))
	if msg == "" {
		return "（空响应体）"
	}
	return truncate(msg, 300)
}
