package convert

import (
	"bytes"
	"encoding/json"
	"fmt"
	"sort"

	"forge-cloud/internal/core"
)

// ---------- Anthropic Messages 上游结构 ----------

type msgContentBlock struct {
	Type     string          `json:"type"`
	Text     string          `json:"text"`
	Thinking string          `json:"thinking"`
	ID       string          `json:"id"`
	Name     string          `json:"name"`
	Input    json.RawMessage `json:"input"`
}

type msgResponse struct {
	ID         string            `json:"id"`
	Type       string            `json:"type"`
	Content    []json.RawMessage `json:"content"`
	StopReason string            `json:"stop_reason"`
	Usage      *msgUsageIn       `json:"usage"`
	Error      json.RawMessage   `json:"error"`
}

// stopReasonFinish 映射 stop_reason；未知值返回空（按有无函数调用推断）。
func stopReasonFinish(r string) string {
	switch r {
	case "end_turn", "stop_sequence", "pause_turn":
		return "stop"
	case "tool_use":
		return "tool_calls"
	case "max_tokens", "model_context_window_exceeded":
		return "length"
	case "refusal":
		return "content_filter"
	}
	return ""
}

// ---------- Messages 事件流 → chat.completion.chunk ----------

type msgBlockState struct {
	tool bool
	call int
}

type messagesToChat struct {
	chatBuilder
	blocks map[int]msgBlockState
	acc    msgUsageAcc
	native map[int]map[string]json.RawMessage
}

func newMessagesToChat(model string, includeUsage bool) *messagesToChat {
	return &messagesToChat{
		chatBuilder: chatBuilder{model: model, includeUsage: includeUsage},
		blocks:      map[int]msgBlockState{},
		native:      map[int]map[string]json.RawMessage{},
	}
}

func (c *messagesToChat) syncUsage() {
	if c.acc.seen {
		c.usage = c.acc.u
		c.hasUsage = true
	}
}

func (c *messagesToChat) Feed(ev Event) ([]byte, error) {
	typ, data, ok, err := eventHead(ev)
	if err != nil {
		return nil, fmt.Errorf("convert: Messages 事件 data 无法解析: %w", err)
	}
	if !ok || c.done {
		return nil, nil
	}
	switch typ {
	case "message_start":
		var e struct {
			Message struct {
				ID    string      `json:"id"`
				Usage *msgUsageIn `json:"usage"`
			} `json:"message"`
		}
		_ = json.Unmarshal(data, &e)
		if c.upstreamID == "" {
			c.upstreamID = e.Message.ID
		}
		c.acc.start(e.Message.Usage)
		c.syncUsage()
	case "content_block_start":
		var e struct {
			Index int             `json:"index"`
			Block json.RawMessage `json:"content_block"`
		}
		_ = json.Unmarshal(data, &e)
		var b msgContentBlock
		_ = json.Unmarshal(e.Block, &b)
		var fields map[string]json.RawMessage
		_ = json.Unmarshal(e.Block, &fields)
		c.native[e.Index] = fields
		return c.blockStart(e.Index, b), nil
	case "content_block_delta":
		var e struct {
			Index int `json:"index"`
			Delta struct {
				Type        string `json:"type"`
				Text        string `json:"text"`
				Thinking    string `json:"thinking"`
				Signature   string `json:"signature"`
				PartialJSON string `json:"partial_json"`
			} `json:"delta"`
		}
		_ = json.Unmarshal(data, &e)
		switch e.Delta.Type {
		case "text_delta":
			c.nativeString(e.Index, "text", e.Delta.Text)
			return c.text(e.Delta.Text), nil
		case "thinking_delta":
			c.nativeString(e.Index, "thinking", e.Delta.Thinking)
			return c.think(e.Delta.Thinking), nil
		case "signature_delta":
			c.nativeString(e.Index, "signature", e.Delta.Signature)
		case "input_json_delta":
			if b, ok := c.blocks[e.Index]; ok && b.tool {
				return c.callArgs(b.call, e.Delta.PartialJSON), nil
			}
		}
	case "content_block_stop":
		var e struct {
			Index int `json:"index"`
		}
		_ = json.Unmarshal(data, &e)
		return c.blockStop(e.Index), nil
	case "message_delta":
		var e struct {
			Delta struct {
				StopReason string `json:"stop_reason"`
			} `json:"delta"`
			Usage *msgUsageIn `json:"usage"`
		}
		_ = json.Unmarshal(data, &e)
		if e.Delta.StopReason != "" {
			c.finish = stopReasonFinish(e.Delta.StopReason)
		}
		c.acc.delta(e.Usage)
		c.syncUsage()
	case "message_stop":
		c.done = true
		c.anthropicContent = c.nativeContent()
		if len(c.anthropicContent) > 0 {
			out := c.open()
			return append(out, c.frame(chunkDelta{AnthropicContent: c.anthropicContent}, nil)...), nil
		}
	case "error":
		return c.fail(streamError(data)), nil
	}
	return nil, nil
}

func (c *messagesToChat) blockStart(idx int, b msgContentBlock) []byte {
	switch b.Type {
	case "text":
		return c.text(b.Text)
	case "thinking":
		c.reasoningBoundary()
		return c.think(b.Thinking)
	case "tool_use":
		// 流式时 input 恒为 {}，参数随 input_json_delta 到达；非流式体的 input 是完整对象。
		args := ""
		if in := bytes.TrimSpace(b.Input); !isNull(in) && string(compactJSON(in)) != "{}" {
			args = string(compactJSON(in))
		}
		call, out := c.addCall(b.ID, b.Name, args)
		c.blocks[idx] = msgBlockState{tool: true, call: call}
		return out
	}
	return nil
}

// blockStop：没有任何参数的 tool_use 补 "{}"，保证客户端拿到可解析的 arguments。
func (c *messagesToChat) blockStop(idx int) []byte {
	b, ok := c.blocks[idx]
	if ok && b.tool && c.native[idx] != nil {
		args := c.calls[b.call].args.String()
		if args == "" {
			args = "{}"
		}
		c.native[idx]["input"] = json.RawMessage(args)
	}
	if !ok || !b.tool || c.calls[b.call].streamed {
		return nil
	}
	return c.callArgs(b.call, "{}")
}

func (c *messagesToChat) nativeString(idx int, field, delta string) {
	block := c.native[idx]
	if block == nil {
		return
	}
	var previous string
	_ = json.Unmarshal(block[field], &previous)
	block[field] = jsonBytes(previous + delta)
}

// 只在上游确实返回签名/密文时附带可回注内容。保留完整块顺序、文本、工具与 redacted_thinking。
func (c *messagesToChat) nativeContent() []json.RawMessage {
	var indices []int
	signed := false
	for idx, block := range c.native {
		indices = append(indices, idx)
		var typ, signature, data string
		_ = json.Unmarshal(block["type"], &typ)
		_ = json.Unmarshal(block["signature"], &signature)
		_ = json.Unmarshal(block["data"], &data)
		if (typ == "thinking" && signature != "") || (typ == "redacted_thinking" && data != "") {
			signed = true
		}
	}
	if !signed {
		return nil
	}
	sort.Ints(indices)
	out := make([]json.RawMessage, 0, len(indices))
	for _, idx := range indices {
		out = append(out, jsonBytes(c.native[idx]))
	}
	return out
}

// ---------- 非流式 Messages → chat.completion ----------

func chatCompletionFromMessages(body []byte, model string) ([]byte, core.Usage, error) {
	var r msgResponse
	if err := decodeObject(body, &r); err != nil {
		return nil, core.Usage{}, fmt.Errorf("convert: Messages 响应体无法解析: %w", err)
	}
	if r.Type == "error" || (len(r.Content) == 0 && !isNull(r.Error)) {
		return nil, core.Usage{}, fmt.Errorf("convert: 上游返回错误: %s", describeError(parseUpstreamError(r.Error)))
	}
	c := newMessagesToChat(model, false)
	c.upstreamID = r.ID
	c.acc.start(r.Usage)
	c.syncUsage()
	for i, raw := range r.Content {
		var fields map[string]json.RawMessage
		_ = json.Unmarshal(raw, &fields)
		c.native[i] = fields
		var b msgContentBlock
		_ = json.Unmarshal(raw, &b)
		c.blockStart(i, b)
		c.blockStop(i)
	}
	c.anthropicContent = c.nativeContent()
	c.finish = stopReasonFinish(r.StopReason)
	return c.Completion(), c.usage, nil
}
