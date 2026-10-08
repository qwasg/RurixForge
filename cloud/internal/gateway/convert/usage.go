package convert

import (
	"bytes"
	"encoding/json"
	"math"

	"forge-cloud/internal/core"
)

// ---------- 上游用量结构 ----------

// chatUsageIn：OpenAI chat usage（prompt_tokens 含缓存命中）；DeepSeek 另给 prompt_cache_hit/miss_tokens。
type chatUsageIn struct {
	PromptTokens        tokens `json:"prompt_tokens"`
	CompletionTokens    tokens `json:"completion_tokens"`
	PromptTokensDetails *struct {
		CachedTokens tokens `json:"cached_tokens"`
	} `json:"prompt_tokens_details"`
	PromptCacheHitTokens  *tokens `json:"prompt_cache_hit_tokens"`
	PromptCacheMissTokens *tokens `json:"prompt_cache_miss_tokens"`
}

func (u *chatUsageIn) toUsage() core.Usage {
	prompt := int64(u.PromptTokens)
	out := core.Usage{OutputTokens: int64(u.CompletionTokens)}
	hit, miss := u.PromptCacheHitTokens, u.PromptCacheMissTokens
	if hit != nil || miss != nil {
		var h, m int64
		switch {
		case hit != nil && miss != nil:
			h, m = int64(*hit), int64(*miss)
		case hit != nil:
			h = int64(*hit)
			m = prompt - h
		default:
			m = int64(*miss)
			h = prompt - m
		}
		out.InputTokens, out.CacheReadTokens = nonNeg(m), nonNeg(h)
		return out
	}
	var cached int64
	if u.PromptTokensDetails != nil {
		cached = nonNeg(int64(u.PromptTokensDetails.CachedTokens))
	}
	out.InputTokens = nonNeg(prompt - cached)
	out.CacheReadTokens = cached
	return out
}

// respUsageIn：Responses usage（input_tokens 含缓存命中）。
type respUsageIn struct {
	InputTokens        tokens `json:"input_tokens"`
	InputTokensDetails *struct {
		CachedTokens tokens `json:"cached_tokens"`
	} `json:"input_tokens_details"`
	OutputTokens        tokens `json:"output_tokens"`
	OutputTokensDetails *struct {
		ReasoningTokens tokens `json:"reasoning_tokens"`
	} `json:"output_tokens_details"`
}

func (u *respUsageIn) toUsage() core.Usage {
	var cached int64
	if u.InputTokensDetails != nil {
		cached = nonNeg(int64(u.InputTokensDetails.CachedTokens))
	}
	return core.Usage{
		InputTokens:     nonNeg(int64(u.InputTokens) - cached),
		OutputTokens:    int64(u.OutputTokens),
		CacheReadTokens: cached,
	}
}

// reasoningTokens：上游给了 output_tokens_details 时返回推理 token 数，否则 nil。
func (u *respUsageIn) reasoningTokens() *int64 {
	if u.OutputTokensDetails == nil {
		return nil
	}
	v := int64(u.OutputTokensDetails.ReasoningTokens)
	return &v
}

// msgUsageIn：Anthropic usage（input_tokens 已不含缓存）。指针区分「缺省」与 0。
type msgUsageIn struct {
	InputTokens              *tokens `json:"input_tokens"`
	OutputTokens             *tokens `json:"output_tokens"`
	CacheCreationInputTokens *tokens `json:"cache_creation_input_tokens"`
	CacheReadInputTokens     *tokens `json:"cache_read_input_tokens"`
}

// msgUsageAcc 合并 Anthropic 流内用量：message_start 给输入与缓存；message_delta 的 output_tokens 为累计值，
// 其输入/缓存字段非零时覆盖 message_start 的值（缺省或 0 不覆盖，避免兼容上游的占位 0 抹掉真实计数）。
type msgUsageAcc struct {
	u    core.Usage
	seen bool
}

func (a *msgUsageAcc) start(in *msgUsageIn) {
	if in == nil {
		return
	}
	a.seen = true
	set := func(dst *int64, v *tokens) {
		if v != nil {
			*dst = int64(*v)
		}
	}
	set(&a.u.InputTokens, in.InputTokens)
	set(&a.u.CacheReadTokens, in.CacheReadInputTokens)
	set(&a.u.CacheWriteTokens, in.CacheCreationInputTokens)
	set(&a.u.OutputTokens, in.OutputTokens)
}

func (a *msgUsageAcc) delta(in *msgUsageIn) {
	if in == nil {
		return
	}
	a.seen = true
	if in.OutputTokens != nil {
		a.u.OutputTokens = int64(*in.OutputTokens)
	}
	override := func(dst *int64, v *tokens) {
		if v != nil && *v > 0 {
			*dst = int64(*v)
		}
	}
	override(&a.u.InputTokens, in.InputTokens)
	override(&a.u.CacheReadTokens, in.CacheReadInputTokens)
	override(&a.u.CacheWriteTokens, in.CacheCreationInputTokens)
}

// ---------- 透传路径的用量旁路 ----------
// 解析一律宽松：json.Unmarshal 先整体校验语法，语法错误时不写目标；类型不符的字段只丢该字段。

type chatUsageTap struct{ u core.Usage }

func (t *chatUsageTap) Feed(ev Event) {
	if !bytes.Contains(ev.Data, []byte(`"usage"`)) {
		return
	}
	var c struct {
		Usage *chatUsageIn `json:"usage"`
	}
	_ = json.Unmarshal(ev.Data, &c)
	if c.Usage != nil {
		t.u = c.Usage.toUsage()
	}
}

func (t *chatUsageTap) Usage() core.Usage { return t.u }

type responsesUsageTap struct{ u core.Usage }

func (t *responsesUsageTap) Feed(ev Event) {
	if !isResponsesFinal(ev) {
		return
	}
	var e struct {
		Type     string `json:"type"`
		Response *struct {
			Usage *respUsageIn `json:"usage"`
		} `json:"response"`
	}
	_ = json.Unmarshal(ev.Data, &e)
	switch firstNonEmpty(e.Type, ev.Event) {
	case "response.completed", "response.incomplete":
		if e.Response != nil && e.Response.Usage != nil {
			t.u = e.Response.Usage.toUsage()
		}
	}
}

func (t *responsesUsageTap) Usage() core.Usage { return t.u }

// isResponsesFinal 是廉价预筛（delta 事件不做完整解析），最终以 data.type 为准。
func isResponsesFinal(ev Event) bool {
	switch ev.Event {
	case "response.completed", "response.incomplete":
		return true
	}
	return bytes.Contains(ev.Data, []byte(`"response.completed"`)) ||
		bytes.Contains(ev.Data, []byte(`"response.incomplete"`))
}

type messagesUsageTap struct{ acc msgUsageAcc }

func (t *messagesUsageTap) Feed(ev Event) {
	if ev.Event != "message_start" && ev.Event != "message_delta" &&
		!bytes.Contains(ev.Data, []byte(`"message_start"`)) && !bytes.Contains(ev.Data, []byte(`"message_delta"`)) {
		return
	}
	var e struct {
		Type    string `json:"type"`
		Message *struct {
			Usage *msgUsageIn `json:"usage"`
		} `json:"message"`
		Usage *msgUsageIn `json:"usage"`
	}
	_ = json.Unmarshal(ev.Data, &e)
	switch firstNonEmpty(e.Type, ev.Event) {
	case "message_start":
		if e.Message != nil {
			t.acc.start(e.Message.Usage)
		}
	case "message_delta":
		t.acc.delta(e.Usage)
	}
}

func (t *messagesUsageTap) Usage() core.Usage { return t.acc.u }

// ---------- 非流式响应体用量 ----------

func chatUsageFromBody(body []byte) core.Usage {
	var b struct {
		Usage *chatUsageIn `json:"usage"`
	}
	_ = json.Unmarshal(body, &b)
	if b.Usage == nil {
		return core.Usage{}
	}
	return b.Usage.toUsage()
}

// responsesUsageFromBody 取 response 对象的 usage；也接受 {"response":{...}} 包装（completed 事件体）。
func responsesUsageFromBody(body []byte) core.Usage {
	var b struct {
		Usage    *respUsageIn `json:"usage"`
		Response *struct {
			Usage *respUsageIn `json:"usage"`
		} `json:"response"`
	}
	_ = json.Unmarshal(body, &b)
	switch {
	case b.Usage != nil:
		return b.Usage.toUsage()
	case b.Response != nil && b.Response.Usage != nil:
		return b.Response.Usage.toUsage()
	}
	return core.Usage{}
}

func messagesUsageFromBody(body []byte) core.Usage {
	var b struct {
		Usage *msgUsageIn `json:"usage"`
	}
	_ = json.Unmarshal(body, &b)
	var acc msgUsageAcc
	acc.start(b.Usage)
	return acc.u
}

// embeddingsUsageFromBody：usage.prompt_tokens（缺省时退回 total_tokens）。
func embeddingsUsageFromBody(body []byte) core.Usage {
	var b struct {
		Usage *struct {
			PromptTokens tokens `json:"prompt_tokens"`
			TotalTokens  tokens `json:"total_tokens"`
		} `json:"usage"`
	}
	_ = json.Unmarshal(body, &b)
	if b.Usage == nil {
		return core.Usage{}
	}
	in := int64(b.Usage.PromptTokens)
	if in == 0 {
		in = int64(b.Usage.TotalTokens)
	}
	return core.Usage{InputTokens: in}
}

// ---------- 错误解析 ----------

// errorObject 兼容 OpenAI {message,type,code}、Anthropic {type,message}、Codex {type,message,resets_in_seconds|resets_at}。
type errorObject struct {
	Message         json.RawMessage `json:"message"`
	Type            json.RawMessage `json:"type"`
	Code            json.RawMessage `json:"code"`
	ResetsInSeconds json.RawMessage `json:"resets_in_seconds"`
	ResetsAt        json.RawMessage `json:"resets_at"`
}

// parseUpstreamError 解析一个错误值：对象取各字段（code 可为数字）；非空字符串作 message；非空数组取紧凑原文。
func parseUpstreamError(raw []byte) *UpstreamError {
	t := bytes.TrimSpace(raw)
	if isNull(t) {
		return nil
	}
	switch t[0] {
	case '{':
		var o errorObject
		_ = json.Unmarshal(t, &o)
		e := &UpstreamError{
			Type:            rawString(o.Type),
			Code:            rawString(o.Code),
			Message:         rawString(o.Message),
			ResetsInSeconds: resetsIn(o.ResetsInSeconds, o.ResetsAt),
		}
		if e.Type == "" && e.Code == "" && e.Message == "" && e.ResetsInSeconds == 0 {
			return nil
		}
		return e
	case '"':
		if s := rawString(t); s != "" {
			return &UpstreamError{Message: s}
		}
	case '[':
		if c := compactJSON(t); string(c) != "[]" {
			return &UpstreamError{Message: string(c)}
		}
	}
	return nil
}

// resetsIn：优先 resets_in_seconds（向上取整）；否则由 resets_at（Unix 秒，毫秒值自动换算）减当前时间。
func resetsIn(inSeconds, at []byte) int {
	if f, ok := rawNumber(inSeconds); ok && f > 0 {
		return int(math.Ceil(f))
	}
	if f, ok := rawNumber(at); ok && f > 0 {
		if f > 1e12 {
			f /= 1000
		}
		if d := int64(math.Ceil(f)) - nowUnix(); d > 0 {
			return int(d)
		}
	}
	return 0
}

// errorFromBody：{error:{...}|"..."}（OpenAI/Anthropic/Codex）> {detail:"..."|{...}|[...]}（Codex/FastAPI）
// > 顶层 {message:"..."}（常见 API 网关）；顶层数组取首个对象。
func errorFromBody(body []byte) *UpstreamError {
	t := bytes.TrimSpace(body)
	if len(t) == 0 {
		return nil
	}
	if t[0] == '[' {
		var arr []json.RawMessage
		if json.Unmarshal(t, &arr) != nil || len(arr) == 0 {
			return nil
		}
		first := bytes.TrimSpace(arr[0])
		if len(first) == 0 || first[0] != '{' {
			return nil
		}
		t = first
	}
	if t[0] != '{' {
		return nil
	}
	var b struct {
		Error   json.RawMessage `json:"error"`
		Detail  json.RawMessage `json:"detail"`
		Message json.RawMessage `json:"message"`
		Type    json.RawMessage `json:"type"`
		Code    json.RawMessage `json:"code"`
	}
	if json.Unmarshal(t, &b) != nil {
		return nil
	}
	if e := parseUpstreamError(b.Error); e != nil {
		return e
	}
	if e := parseUpstreamError(b.Detail); e != nil {
		return e
	}
	if m := bytes.TrimSpace(b.Message); len(m) > 0 && m[0] == '"' {
		if msg := rawString(m); msg != "" {
			return &UpstreamError{Type: rawString(b.Type), Code: rawString(b.Code), Message: msg}
		}
	}
	return nil
}
