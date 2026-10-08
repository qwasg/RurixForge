package convert

import (
	"encoding/json"
	"strings"

	"forge-cloud/internal/core"
)

// ---------- chat.completion.chunk / chat.completion 输出结构（字段顺序即输出顺序）----------

type chunkFn struct {
	Name      string `json:"name,omitempty"`
	Arguments string `json:"arguments"`
}

type chunkToolCall struct {
	Index    int     `json:"index"`
	ID       string  `json:"id,omitempty"`
	Type     string  `json:"type,omitempty"`
	Function chunkFn `json:"function"`
}

type chunkDelta struct {
	Role             string            `json:"role,omitempty"`
	Content          *string           `json:"content,omitempty"`
	ReasoningContent *string           `json:"reasoning_content,omitempty"`
	ToolCalls        []chunkToolCall   `json:"tool_calls,omitempty"`
	AnthropicContent []json.RawMessage `json:"anthropic_content,omitempty"`
}

type chunkChoice struct {
	Index        int        `json:"index"`
	Delta        chunkDelta `json:"delta"`
	FinishReason *string    `json:"finish_reason"`
}

type chatChunk struct {
	ID      string        `json:"id"`
	Object  string        `json:"object"`
	Created int64         `json:"created"`
	Model   string        `json:"model"`
	Choices []chunkChoice `json:"choices"`
	Usage   *chatUsageOut `json:"usage,omitempty"`
}

// chatUsageOut 是发给客户端的 OpenAI 口径用量：prompt_tokens 含缓存读/写。
type chatUsageOut struct {
	PromptTokens            int64                 `json:"prompt_tokens"`
	CompletionTokens        int64                 `json:"completion_tokens"`
	TotalTokens             int64                 `json:"total_tokens"`
	PromptTokensDetails     promptDetailsOut      `json:"prompt_tokens_details"`
	CompletionTokensDetails *completionDetailsOut `json:"completion_tokens_details,omitempty"`
}

type promptDetailsOut struct {
	CachedTokens int64 `json:"cached_tokens"`
}

type completionDetailsOut struct {
	ReasoningTokens int64 `json:"reasoning_tokens"`
}

func chatUsageFor(u core.Usage, reasoning *int64) chatUsageOut {
	prompt := u.InputTokens + u.CacheReadTokens + u.CacheWriteTokens
	out := chatUsageOut{
		PromptTokens:        prompt,
		CompletionTokens:    u.OutputTokens,
		TotalTokens:         prompt + u.OutputTokens,
		PromptTokensDetails: promptDetailsOut{CachedTokens: u.CacheReadTokens},
	}
	if reasoning != nil {
		out.CompletionTokensDetails = &completionDetailsOut{ReasoningTokens: *reasoning}
	}
	return out
}

type chatCompletionOut struct {
	ID      string             `json:"id"`
	Object  string             `json:"object"`
	Created int64              `json:"created"`
	Model   string             `json:"model"`
	Choices []completionChoice `json:"choices"`
	Usage   chatUsageOut       `json:"usage"`
}

type completionChoice struct {
	Index        int           `json:"index"`
	Message      completionMsg `json:"message"`
	FinishReason string        `json:"finish_reason"`
}

type completionMsg struct {
	Role             string            `json:"role"`
	Content          *string           `json:"content"`
	ReasoningContent string            `json:"reasoning_content,omitempty"`
	ToolCalls        []outToolCall     `json:"tool_calls,omitempty"`
	AnthropicContent []json.RawMessage `json:"anthropic_content,omitempty"`
}

type outToolCall struct {
	ID       string    `json:"id"`
	Type     string    `json:"type"`
	Function outToolFn `json:"function"`
}

type outToolFn struct {
	Name      string `json:"name"`
	Arguments string `json:"arguments"`
}

type errorFrameBody struct {
	Error errorFrameErr `json:"error"`
}

type errorFrameErr struct {
	Message string  `json:"message"`
	Type    string  `json:"type"`
	Code    *string `json:"code"`
}

// ---------- 两种上游共用的 chat 聚合器 ----------

// chatBuilder 聚合上游输出（文本、推理、函数调用、用量、错误），按需产出 chat.completion.chunk 帧，
// 并能在任意时刻给出非流式 chat.completion。实现 ToChat 除 Feed 外的全部方法。
type chatBuilder struct {
	model        string
	includeUsage bool

	upstreamID string // 上游 response/message id；chunk id = "chatcmpl-" + upstreamID（未知时随机）
	id         string
	created    int64

	started  bool // 已产出 role 开头块（= 已向客户端写出内容）
	finished bool // Finish 已调用
	done     bool // 已收到终止事件

	content          strings.Builder
	reasoning        strings.Builder
	reasonSep        bool // 下一段推理文本前补 "\n\n"
	calls            []*toolCallAcc
	anthropicContent []json.RawMessage

	finish          string // 上游明确给出的 finish_reason；空则按有无函数调用推断
	usage           core.Usage
	hasUsage        bool
	reasoningTokens *int64

	upErr   *UpstreamError
	errSent bool
}

type toolCallAcc struct {
	id, name string
	args     strings.Builder
	streamed bool // 已产出过参数
}

func (b *chatBuilder) ensureID() {
	if b.id != "" {
		return
	}
	if b.upstreamID != "" {
		b.id = "chatcmpl-" + b.upstreamID
	} else {
		b.id = "chatcmpl-" + newID()
	}
	b.created = nowUnix()
}

func (b *chatBuilder) frame(delta chunkDelta, finish *string) []byte {
	b.ensureID()
	return FormatSSE("", jsonBytes(chatChunk{
		ID: b.id, Object: "chat.completion.chunk", Created: b.created, Model: b.model,
		Choices: []chunkChoice{{Delta: delta, FinishReason: finish}},
	}))
}

// open 在首个内容帧前补 role=assistant 开头块。开头块推迟到有内容时才产出：
// 上游在任何内容前就失败时网关尚未写出字节，仍可换号重试。
func (b *chatBuilder) open() []byte {
	if b.started {
		return nil
	}
	b.started = true
	empty := ""
	return b.frame(chunkDelta{Role: "assistant", Content: &empty}, nil)
}

func (b *chatBuilder) text(s string) []byte {
	if s == "" {
		return nil
	}
	out := b.open()
	b.content.WriteString(s)
	return append(out, b.frame(chunkDelta{Content: &s}, nil)...)
}

func (b *chatBuilder) think(s string) []byte {
	if s == "" {
		return nil
	}
	if b.reasonSep {
		s = "\n\n" + s
		b.reasonSep = false
	}
	out := b.open()
	b.reasoning.WriteString(s)
	return append(out, b.frame(chunkDelta{ReasoningContent: &s}, nil)...)
}

// reasoningBoundary 标记新推理段开始：已有推理文本时，下一段前补空行（多段 summary / 多个 thinking 块）。
func (b *chatBuilder) reasoningBoundary() {
	if b.reasoning.Len() > 0 {
		b.reasonSep = true
	}
}

// addCall 登记一个函数调用并产出其开头增量（id、name、arguments 初值）；返回 0 起的调用序号。
func (b *chatBuilder) addCall(id, name, args string) (int, []byte) {
	idx := len(b.calls)
	c := &toolCallAcc{id: id, name: name, streamed: args != ""}
	c.args.WriteString(args)
	b.calls = append(b.calls, c)
	out := b.open()
	return idx, append(out, b.frame(chunkDelta{ToolCalls: []chunkToolCall{{
		Index: idx, ID: id, Type: "function", Function: chunkFn{Name: name, Arguments: args},
	}}}, nil)...)
}

func (b *chatBuilder) callArgs(idx int, delta string) []byte {
	if delta == "" || idx < 0 || idx >= len(b.calls) {
		return nil
	}
	c := b.calls[idx]
	c.streamed = true
	c.args.WriteString(delta)
	out := b.open()
	return append(out, b.frame(chunkDelta{ToolCalls: []chunkToolCall{{
		Index: idx, Function: chunkFn{Arguments: delta},
	}}}, nil)...)
}

// fail 记录上游错误（保留首个）并视为终止。已开流时立即产出错误帧 data: {"error":{...}}（OpenAI SDK 据此抛错）；
// 未开流时不产出任何字节，交给网关换号或回 HTTP 错误（网关不处理时 Finish 仍会补错误帧）。
func (b *chatBuilder) fail(e *UpstreamError) []byte {
	b.done = true
	if b.upErr == nil {
		b.upErr = e
	}
	if !b.started || b.errSent {
		return nil
	}
	b.errSent = true
	return b.errorFrame()
}

func (b *chatBuilder) errorFrame() []byte {
	e := b.upErr
	body := errorFrameBody{Error: errorFrameErr{
		Message: firstNonEmpty(e.Message, "上游返回错误"),
		Type:    firstNonEmpty(e.Type, "upstream_error"),
	}}
	if e.Code != "" {
		code := e.Code
		body.Error.Code = &code
	}
	return FormatSSE("", jsonBytes(body))
}

func (b *chatBuilder) finishReason() string {
	if b.finish != "" {
		return b.finish
	}
	if len(b.calls) > 0 {
		return "tool_calls"
	}
	return "stop"
}

// Finish：正常结束 → [开头块] + finish_reason 块 + [usage 块] + [DONE]；
// 上游报错 → [错误帧（Feed 未发过时）] + [DONE]。重复调用返回 nil。
func (b *chatBuilder) Finish() []byte {
	if b.finished {
		return nil
	}
	b.finished = true
	if b.upErr != nil {
		var out []byte
		if !b.errSent {
			b.errSent = true
			out = b.errorFrame()
		}
		return append(out, DoneFrame...)
	}
	out := b.open()
	reason := b.finishReason()
	out = append(out, b.frame(chunkDelta{}, &reason)...)
	if b.includeUsage && b.hasUsage {
		u := b.usageOut()
		out = append(out, FormatSSE("", jsonBytes(chatChunk{
			ID: b.id, Object: "chat.completion.chunk", Created: b.created, Model: b.model,
			Choices: []chunkChoice{}, Usage: &u,
		}))...)
	}
	return append(out, DoneFrame...)
}

// Completion 返回聚合出的 chat.completion：仅有函数调用时 content 为 null；无参数的调用补 "{}"。
func (b *chatBuilder) Completion() []byte {
	b.ensureID()
	msg := completionMsg{Role: "assistant", ReasoningContent: b.reasoning.String(), AnthropicContent: b.anthropicContent}
	text := b.content.String()
	if text != "" || len(b.calls) == 0 {
		msg.Content = &text
	}
	for _, c := range b.calls {
		args := c.args.String()
		if args == "" {
			args = "{}"
		}
		msg.ToolCalls = append(msg.ToolCalls, outToolCall{
			ID: c.id, Type: "function", Function: outToolFn{Name: c.name, Arguments: args},
		})
	}
	return jsonBytes(chatCompletionOut{
		ID: b.id, Object: "chat.completion", Created: b.created, Model: b.model,
		Choices: []completionChoice{{Message: msg, FinishReason: b.finishReason()}},
		Usage:   b.usageOut(),
	})
}

func (b *chatBuilder) usageOut() chatUsageOut { return chatUsageFor(b.usage, b.reasoningTokens) }

func (b *chatBuilder) Usage() core.Usage { return b.usage }

func (b *chatBuilder) Done() bool { return b.done }

func (b *chatBuilder) UpstreamError() *UpstreamError { return b.upErr }
