package gateway

import (
	"bytes"
	"context"
	"crypto/rand"
	"encoding/json"
	"fmt"
	"net/http"
	"strconv"
	"strings"
	"time"

	"forge-cloud/internal/accounts"
	"forge-cloud/internal/core"
	"forge-cloud/internal/gateway/convert"
	"forge-cloud/internal/oauth/openai"
)

// relayMode 决定如何把上游响应交给客户端。
type relayMode int

const (
	relayStream           relayMode = iota // SSE 透传（旁路截取用量）
	relayJSON                              // 非流式透传
	relayCollect                           // Responses SSE → 聚合成 response JSON（Codex + 非流式客户端）
	relayChatStream                        // 上游 SSE → chat.completion.chunk
	relayChatCollect                       // 上游 SSE → chat.completion JSON
	relayChatFromMessages                  // Anthropic 非流式 JSON → chat.completion JSON
)

// plan 是一次上游请求的目标、请求体与回传方式。
type plan struct {
	url   string
	body  []byte
	mode  relayMode
	proto string // 上游协议：chat|responses|messages|embeddings（决定用量截取方式）
	sse   bool   // 期望上游返回 SSE
}

// platformSupports：模型平台与端点的静态兼容性（账号级细节见 Account.SupportsEndpoint）。
func platformSupports(platform, endpoint string) bool {
	switch platform {
	case accounts.PlatformOpenAI:
		return endpoint == epChat || endpoint == epResponses || endpoint == epEmbeddings
	case accounts.PlatformAnthropic:
		return endpoint == epChat || endpoint == epMessages
	}
	return false
}

func (c *call) buildPlan(a *accounts.Account, upstreamModel string) (*plan, error) {
	cfg := c.s.d.Config
	switch {
	case a.IsCodex():
		url := cfg.CodexBaseURL + "/responses"
		switch c.endpoint {
		case epResponses:
			body, err := shapeCodexResponses(c.body, upstreamModel, c.settings.CodexInstructions)
			if err != nil {
				return nil, err
			}
			mode := relayCollect
			if c.stream {
				mode = relayStream
			}
			return &plan{url: url, body: body, mode: mode, proto: epResponses, sse: true}, nil
		case epChat:
			store := false
			body, err := convert.ChatToResponsesRequest(c.body, convert.ChatToResponsesOptions{
				Model: upstreamModel, Instructions: c.settings.CodexInstructions, PromptCacheKey: c.sessionKey,
				Stream: true, Store: &store, DropSampling: true,
			})
			if err != nil {
				return nil, conversionError(err)
			}
			mode := relayChatCollect
			if c.stream {
				mode = relayChatStream
			}
			return &plan{url: url, body: body, mode: mode, proto: epResponses, sse: true}, nil
		}
	case a.Platform == accounts.PlatformOpenAI && a.AuthType == accounts.AuthAPIKey:
		mode := relayJSON
		if c.stream {
			mode = relayStream
		}
		switch c.endpoint {
		case epChat:
			set := map[string]any{"model": upstreamModel}
			body, err := patchJSON(c.body, set)
			if err == nil && c.stream {
				body, err = injectIncludeUsage(body)
			}
			if err != nil {
				return nil, err
			}
			return &plan{url: a.BaseURL + "/chat/completions", body: body, mode: mode, proto: epChat, sse: c.stream}, nil
		case epResponses:
			body, err := patchJSON(c.body, map[string]any{"model": upstreamModel})
			if err != nil {
				return nil, err
			}
			return &plan{url: a.BaseURL + "/responses", body: body, mode: mode, proto: epResponses, sse: c.stream}, nil
		case epEmbeddings:
			body, err := patchJSON(c.body, map[string]any{"model": upstreamModel})
			if err != nil {
				return nil, err
			}
			return &plan{url: a.BaseURL + "/embeddings", body: body, mode: relayJSON, proto: epEmbeddings}, nil
		}
	case a.Platform == accounts.PlatformAnthropic && a.AuthType == accounts.AuthAPIKey:
		url := a.BaseURL + "/messages"
		switch c.endpoint {
		case epMessages:
			body, err := patchJSON(c.body, map[string]any{"model": upstreamModel})
			if err != nil {
				return nil, err
			}
			mode := relayJSON
			if c.stream {
				mode = relayStream
			}
			return &plan{url: url, body: body, mode: mode, proto: epMessages, sse: c.stream}, nil
		case epChat:
			body, err := convert.ChatToMessagesRequest(c.body, convert.ChatToMessagesOptions{
				Model: upstreamModel, ThinkingMode: c.model.Capabilities.ThinkingMode,
				ThinkingAlwaysOn: c.model.Capabilities.ThinkingAlwaysOn,
			})
			if err != nil {
				return nil, conversionError(err)
			}
			if body, err = patchJSON(body, map[string]any{"stream": c.stream}); err != nil {
				return nil, conversionError(err)
			}
			mode := relayChatFromMessages
			if c.stream {
				mode = relayChatStream
			}
			return &plan{url: url, body: body, mode: mode, proto: epMessages, sse: c.stream}, nil
		}
	}
	return nil, core.BadRequest(core.CodeEndpointNotSupported, "该上游账号不支持此端点")
}

func conversionError(err error) *core.Error {
	return core.BadRequest(core.CodeInvalidRequest, "请求转换失败："+err.Error())
}

// newRequest 构造上游 HTTP 请求（按账号类型写鉴权与协议头）。
func (c *call) newRequest(ctx context.Context, a *accounts.Account, creds *accounts.Credentials, p *plan) (*http.Request, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, p.url, bytes.NewReader(p.body))
	if err != nil {
		return nil, err
	}
	h := req.Header
	h.Set("Content-Type", "application/json")
	accept := "application/json"
	if p.sse {
		accept = "text/event-stream"
	}
	switch {
	case a.IsCodex():
		accountID := firstNonEmpty(creds.AccountID, a.ExternalID)
		openai.SetCodexHeaders(h, creds.AccessToken, accountID, c.codexSession(), c.r.UserAgent())
		accept = "text/event-stream"
	case a.Platform == accounts.PlatformAnthropic:
		if c.endpoint == epMessages {
			if v := c.r.Header.Get("anthropic-version"); v != "" {
				h.Set("anthropic-version", v)
			}
			if v := c.r.Header.Get("anthropic-beta"); v != "" {
				h.Set("anthropic-beta", v)
			}
		}
		accounts.SetAPIKeyHeaders(h, a, creds.APIKey)
		h.Set("User-Agent", accounts.UserAgent())
	default:
		accounts.SetAPIKeyHeaders(h, a, creds.APIKey)
		h.Set("User-Agent", accounts.UserAgent())
	}
	h.Set("Accept", accept)
	return req, nil
}

// codexSession：Codex 上游的 session_id 头取粘性键，没有则每个请求一个随机 UUID（跨重试不变）。
func (c *call) codexSession() string {
	if c.sessionKey != "" {
		return c.sessionKey
	}
	if c.randomSession == "" {
		c.randomSession = newUUID()
	}
	return c.randomSession
}

func newUUID() string {
	var b [16]byte
	_, _ = rand.Read(b[:])
	b[6] = (b[6] & 0x0f) | 0x40
	b[8] = (b[8] & 0x3f) | 0x80
	return fmt.Sprintf("%x-%x-%x-%x-%x", b[0:4], b[4:6], b[6:8], b[8:10], b[10:16])
}

// ---------- 请求体整形 ----------

func marshalNoEscape(v any) ([]byte, error) {
	var buf bytes.Buffer
	enc := json.NewEncoder(&buf)
	enc.SetEscapeHTML(false)
	if err := enc.Encode(v); err != nil {
		return nil, err
	}
	return bytes.TrimRight(buf.Bytes(), "\n"), nil
}

// patchJSON 在 JSON 对象上设置/删除顶层字段（其余字段原样保留）。
func patchJSON(body []byte, set map[string]any, del ...string) ([]byte, error) {
	var m map[string]json.RawMessage
	if err := json.Unmarshal(body, &m); err != nil || m == nil {
		return nil, core.BadRequest(core.CodeInvalidRequest, "请求体必须是 JSON 对象")
	}
	for k, v := range set {
		b, err := marshalNoEscape(v)
		if err != nil {
			return nil, err
		}
		m[k] = b
	}
	for _, k := range del {
		delete(m, k)
	}
	return marshalNoEscape(m)
}

// injectIncludeUsage 强制 stream_options.include_usage=true（流式 chat 透传时截取用量）。
func injectIncludeUsage(body []byte) ([]byte, error) {
	var m map[string]json.RawMessage
	if err := json.Unmarshal(body, &m); err != nil {
		return nil, core.BadRequest(core.CodeInvalidRequest, "请求体必须是 JSON 对象")
	}
	opts := map[string]json.RawMessage{}
	if raw, ok := m["stream_options"]; ok && string(raw) != "null" {
		if err := json.Unmarshal(raw, &opts); err != nil {
			return nil, core.BadRequest(core.CodeInvalidRequest, "stream_options 必须是对象")
		}
	}
	opts["include_usage"] = json.RawMessage("true")
	b, err := marshalNoEscape(opts)
	if err != nil {
		return nil, err
	}
	m["stream_options"] = b
	return marshalNoEscape(m)
}

// codexDroppedFields 是 Codex 订阅上游不接受的采样参数。
var codexDroppedFields = []string{"max_output_tokens", "temperature", "top_p"}

// shapeCodexResponses：Codex 订阅上游要求 store=false、stream=true；model 改成实发模型；
// 客户端没给 instructions 时用系统设置 codexInstructions；字符串 input 包成一条 user 消息。
func shapeCodexResponses(body []byte, upstreamModel, instructions string) ([]byte, error) {
	var m map[string]json.RawMessage
	if err := json.Unmarshal(body, &m); err != nil || m == nil {
		return nil, core.BadRequest(core.CodeInvalidRequest, "请求体必须是 JSON 对象")
	}
	set := map[string]any{"model": upstreamModel, "store": false, "stream": true}
	if instructions != "" {
		var cur string
		if raw, ok := m["instructions"]; !ok || string(raw) == "null" || (json.Unmarshal(raw, &cur) == nil && cur == "") {
			set["instructions"] = instructions
		}
	}
	var text string
	if raw, ok := m["input"]; ok && json.Unmarshal(raw, &text) == nil {
		set["input"] = []any{map[string]any{
			"type": "message", "role": "user",
			"content": []any{map[string]any{"type": "input_text", "text": text}},
		}}
	}
	return patchJSON(body, set, codexDroppedFields...)
}

// ---------- 冷却 ----------

const maxCooldown = 7 * 24 * time.Hour

// cooldownFor429 依次取 Retry-After、已用满窗口的 x-codex-*-reset-after-seconds、错误体 resets_in_seconds，缺省 60 秒。
func cooldownFor429(h http.Header, body []byte) time.Duration {
	if d := parseRetryAfter(h.Get("Retry-After")); d > 0 {
		return clampCooldown(d)
	}
	primary, secondary := openai.ParseCodexRateLimits(h)
	best := 0
	for _, w := range []*openai.RateWindow{primary, secondary} {
		if w == nil || w.ResetAfterSeconds == nil {
			continue
		}
		if w.Exhausted() || w.UsedPercent == nil {
			best = max(best, *w.ResetAfterSeconds)
		}
	}
	if best > 0 {
		return clampCooldown(time.Duration(best) * time.Second)
	}
	if ue := upstreamError(body); ue != nil && ue.ResetsInSeconds > 0 {
		return clampCooldown(time.Duration(ue.ResetsInSeconds) * time.Second)
	}
	return 60 * time.Second
}

func parseRetryAfter(v string) time.Duration {
	v = strings.TrimSpace(v)
	if v == "" {
		return 0
	}
	if n, err := strconv.ParseFloat(v, 64); err == nil && n > 0 {
		return time.Duration(n * float64(time.Second))
	}
	if t, err := http.ParseTime(v); err == nil {
		if d := time.Until(t); d > 0 {
			return d
		}
	}
	return 0
}

func clampCooldown(d time.Duration) time.Duration {
	if d < time.Second {
		return time.Second
	}
	if d > maxCooldown {
		return maxCooldown
	}
	return d
}

func retryAfterSeconds(d time.Duration) int {
	s := int((d + time.Second - 1) / time.Second)
	return min(max(s, 1), 3600)
}
