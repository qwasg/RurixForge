package convert

import (
	"encoding/json"
	"errors"
	"fmt"
	"maps"
	"slices"
	"strconv"
	"strings"

	"forge-cloud/internal/core"
)

// ---------- Responses 上游结构（只取转换需要的字段，按类型宽松解码）----------

type respItem struct {
	ID        string          `json:"id"`
	Type      string          `json:"type"`
	CallID    string          `json:"call_id"`
	Name      string          `json:"name"`
	Arguments json.RawMessage `json:"arguments"`
	Content   json.RawMessage `json:"content"`
	Summary   json.RawMessage `json:"summary"`
}

type respPart struct {
	Type    string `json:"type"`
	Text    string `json:"text"`
	Refusal string `json:"refusal"`
}

func respParts(raw json.RawMessage) []respPart {
	var ps []respPart
	_ = json.Unmarshal(raw, &ps)
	return ps
}

// respObject 是 response 对象（非流式响应体，或 completed/incomplete/failed 事件里的 response）。
type respObject struct {
	ID                string            `json:"id"`
	Status            string            `json:"status"`
	Output            []json.RawMessage `json:"output"`
	Usage             *respUsageIn      `json:"usage"`
	IncompleteDetails *struct {
		Reason string `json:"reason"`
	} `json:"incomplete_details"`
	Error json.RawMessage `json:"error"`
}

type respItemEvent struct {
	OutputIndex int      `json:"output_index"`
	Item        respItem `json:"item"`
}

type respDeltaEvent struct {
	ItemID      string `json:"item_id"`
	OutputIndex int    `json:"output_index"`
	Delta       string `json:"delta"`
}

func incompleteFinish(reason string) string {
	switch reason {
	case "max_output_tokens", "max_tokens":
		return "length"
	case "content_filter":
		return "content_filter"
	}
	return ""
}

// streamError 解析流内 error 事件：{type:"error", code, message} 或 {type:"error", error:{...}}（Anthropic 同形）。
func streamError(data []byte) *UpstreamError {
	var e struct {
		Error           json.RawMessage `json:"error"`
		Code            json.RawMessage `json:"code"`
		Message         json.RawMessage `json:"message"`
		ResetsInSeconds json.RawMessage `json:"resets_in_seconds"`
		ResetsAt        json.RawMessage `json:"resets_at"`
	}
	_ = json.Unmarshal(data, &e)
	if ue := parseUpstreamError(e.Error); ue != nil {
		return ue
	}
	ue := &UpstreamError{
		Code:            rawString(e.Code),
		Message:         rawString(e.Message),
		ResetsInSeconds: resetsIn(e.ResetsInSeconds, e.ResetsAt),
	}
	if ue.Code == "" && ue.Message == "" {
		ue.Message = "上游返回错误"
	}
	return ue
}

// ---------- Responses 事件流 → chat.completion.chunk ----------

type responsesToChat struct {
	chatBuilder
	// 输出项同时按 item id 与 output_index 记账，兼容缺 id 或缺 index 的兼容上游。
	callByID   map[string]int
	callByIdx  map[int]int
	textSeen   map[string]bool
	reasonSeen map[string]bool
}

func newResponsesToChat(model string, includeUsage bool) *responsesToChat {
	return &responsesToChat{
		chatBuilder: chatBuilder{model: model, includeUsage: includeUsage},
		callByID:    map[string]int{},
		callByIdx:   map[int]int{},
		textSeen:    map[string]bool{},
		reasonSeen:  map[string]bool{},
	}
}

func markSeen(m map[string]bool, id string, idx int) {
	if id != "" {
		m["id:"+id] = true
	}
	m["#"+strconv.Itoa(idx)] = true
}

func wasSeen(m map[string]bool, id string, idx int) bool {
	return (id != "" && m["id:"+id]) || m["#"+strconv.Itoa(idx)]
}

func (c *responsesToChat) callFor(id string, idx int) (int, bool) {
	if id != "" {
		if i, ok := c.callByID[id]; ok {
			return i, true
		}
	}
	i, ok := c.callByIdx[idx]
	return i, ok
}

func (c *responsesToChat) registerCall(id string, idx, call int) {
	if id != "" {
		c.callByID[id] = call
	}
	c.callByIdx[idx] = call
}

func (c *responsesToChat) Feed(ev Event) ([]byte, error) {
	typ, data, ok, err := eventHead(ev)
	if err != nil {
		return nil, fmt.Errorf("convert: Responses 事件 data 无法解析: %w", err)
	}
	if !ok || c.done {
		return nil, nil
	}
	switch typ {
	case "response.created", "response.in_progress", "response.queued":
		var e struct {
			Response struct {
				ID string `json:"id"`
			} `json:"response"`
		}
		_ = json.Unmarshal(data, &e)
		if c.upstreamID == "" {
			c.upstreamID = e.Response.ID
		}
	case "response.output_item.added":
		var e respItemEvent
		_ = json.Unmarshal(data, &e)
		return c.itemAdded(e.OutputIndex, e.Item), nil
	case "response.output_item.done":
		var e respItemEvent
		_ = json.Unmarshal(data, &e)
		return c.itemDone(e.OutputIndex, e.Item), nil
	case "response.output_text.delta", "response.refusal.delta":
		var e respDeltaEvent
		_ = json.Unmarshal(data, &e)
		markSeen(c.textSeen, e.ItemID, e.OutputIndex)
		return c.text(e.Delta), nil
	case "response.reasoning_summary_part.added":
		c.reasoningBoundary()
	case "response.reasoning_summary_text.delta", "response.reasoning_text.delta":
		var e respDeltaEvent
		_ = json.Unmarshal(data, &e)
		markSeen(c.reasonSeen, e.ItemID, e.OutputIndex)
		return c.think(e.Delta), nil
	case "response.function_call_arguments.delta":
		var e respDeltaEvent
		_ = json.Unmarshal(data, &e)
		if i, ok := c.callFor(e.ItemID, e.OutputIndex); ok {
			return c.callArgs(i, e.Delta), nil
		}
	case "response.function_call_arguments.done":
		var e struct {
			ItemID      string          `json:"item_id"`
			OutputIndex int             `json:"output_index"`
			Arguments   json.RawMessage `json:"arguments"`
		}
		_ = json.Unmarshal(data, &e)
		if i, ok := c.callFor(e.ItemID, e.OutputIndex); ok && !c.calls[i].streamed {
			return c.callArgs(i, rawString(e.Arguments)), nil
		}
	case "response.completed", "response.incomplete":
		var e struct {
			Response respObject `json:"response"`
		}
		_ = json.Unmarshal(data, &e)
		out := c.applyFinal(&e.Response)
		c.done = true
		return out, nil
	case "response.failed":
		var e struct {
			Response respObject `json:"response"`
		}
		_ = json.Unmarshal(data, &e)
		if c.upstreamID == "" {
			c.upstreamID = e.Response.ID
		}
		ue := parseUpstreamError(e.Response.Error)
		if ue == nil {
			ue = &UpstreamError{Message: "上游响应失败（response.failed）"}
		}
		return c.fail(ue), nil
	case "error":
		return c.fail(streamError(data)), nil
	}
	return nil, nil
}

func (c *responsesToChat) itemAdded(idx int, it respItem) []byte {
	switch it.Type {
	case "function_call":
		if _, ok := c.callFor(it.ID, idx); ok {
			return nil
		}
		call, out := c.addCall(it.CallID, it.Name, rawString(it.Arguments))
		c.registerCall(it.ID, idx, call)
		return out
	case "reasoning":
		c.reasoningBoundary()
	}
	return nil
}

// itemDone 是逐项兜底：该项内容未经 delta 流出时（部分兼容上游只给 done），按完整项补发。
func (c *responsesToChat) itemDone(idx int, it respItem) []byte {
	switch it.Type {
	case "message":
		if wasSeen(c.textSeen, it.ID, idx) {
			return nil
		}
		markSeen(c.textSeen, it.ID, idx)
		var sb strings.Builder
		for _, p := range respParts(it.Content) {
			switch p.Type {
			case "output_text":
				sb.WriteString(p.Text)
			case "refusal":
				sb.WriteString(p.Refusal)
			}
		}
		return c.text(sb.String())
	case "reasoning":
		if wasSeen(c.reasonSeen, it.ID, idx) {
			return nil
		}
		markSeen(c.reasonSeen, it.ID, idx)
		var out []byte
		for _, p := range append(respParts(it.Summary), respParts(it.Content)...) {
			if p.Text != "" {
				c.reasoningBoundary()
				out = append(out, c.think(p.Text)...)
			}
		}
		return out
	case "function_call":
		args := rawString(it.Arguments)
		if i, ok := c.callFor(it.ID, idx); ok {
			if c.calls[i].streamed {
				return nil
			}
			return c.callArgs(i, args)
		}
		call, out := c.addCall(it.CallID, it.Name, args)
		c.registerCall(it.ID, idx, call)
		return out
	}
	return nil
}

// applyFinal 处理终态 response：用量、incomplete 原因；整条流未产出任何内容时（只给终态的「假流」/非流式体）
// 按 output 逐项补发。已产出过内容则不再回看 output，杜绝重复。
func (c *responsesToChat) applyFinal(r *respObject) []byte {
	if c.upstreamID == "" {
		c.upstreamID = r.ID
	}
	if r.Usage != nil {
		c.usage = r.Usage.toUsage()
		c.hasUsage = true
		c.reasoningTokens = r.Usage.reasoningTokens()
	}
	if r.IncompleteDetails != nil {
		if f := incompleteFinish(r.IncompleteDetails.Reason); f != "" {
			c.finish = f
		}
	}
	if c.started {
		return nil
	}
	var out []byte
	for i, raw := range r.Output {
		var it respItem
		_ = json.Unmarshal(raw, &it)
		out = append(out, c.itemDone(i, it)...)
	}
	return out
}

// ---------- 非流式 Responses → chat.completion ----------

func chatCompletionFromResponses(body []byte, model string) ([]byte, core.Usage, error) {
	var r respObject
	if err := decodeObject(body, &r); err != nil {
		return nil, core.Usage{}, fmt.Errorf("convert: Responses 响应体无法解析: %w", err)
	}
	if r.Status == "failed" || (len(r.Output) == 0 && !isNull(r.Error)) {
		return nil, core.Usage{}, fmt.Errorf("convert: 上游响应失败: %s", describeError(parseUpstreamError(r.Error)))
	}
	c := newResponsesToChat(model, false)
	c.upstreamID = r.ID
	c.applyFinal(&r)
	return c.Completion(), c.usage, nil
}

// ---------- Responses 事件流 → 最终 response 对象 ----------

type responsesCollector struct {
	final json.RawMessage
	items map[int]json.RawMessage // output_item.done，按 output_index
	usage core.Usage
	upErr *UpstreamError
}

func newResponsesCollector() *responsesCollector {
	return &responsesCollector{items: map[int]json.RawMessage{}}
}

func (c *responsesCollector) Feed(ev Event) {
	typ, data, ok, err := eventHead(ev)
	if err != nil || !ok || c.final != nil {
		return
	}
	switch typ {
	case "response.output_item.done":
		var e struct {
			OutputIndex int             `json:"output_index"`
			Item        json.RawMessage `json:"item"`
		}
		_ = json.Unmarshal(data, &e)
		if !isNull(e.Item) {
			c.items[e.OutputIndex] = e.Item
		}
	case "response.completed", "response.incomplete":
		var e struct {
			Response json.RawMessage `json:"response"`
		}
		_ = json.Unmarshal(data, &e)
		if isNull(e.Response) {
			return
		}
		c.final = e.Response
		var r struct {
			Usage *respUsageIn `json:"usage"`
		}
		_ = json.Unmarshal(e.Response, &r)
		if r.Usage != nil {
			c.usage = r.Usage.toUsage()
		}
	case "response.failed":
		if c.upErr == nil {
			var e struct {
				Response struct {
					Error json.RawMessage `json:"error"`
				} `json:"response"`
			}
			_ = json.Unmarshal(data, &e)
			if c.upErr = parseUpstreamError(e.Response.Error); c.upErr == nil {
				c.upErr = &UpstreamError{Message: "上游响应失败（response.failed）"}
			}
		}
	case "error":
		if c.upErr == nil {
			c.upErr = streamError(data)
		}
	}
}

// Result 返回终态 response 原文；其 output 为空而流内收到过 output_item.done 时，用这些项按 output_index 回填
// （部分上游的 completed 事件不带 output）。
func (c *responsesCollector) Result() ([]byte, error) {
	if c.final == nil {
		if c.upErr != nil {
			return nil, fmt.Errorf("convert: 上游响应失败: %s", describeError(c.upErr))
		}
		return nil, errors.New("convert: 上游流已结束但未收到 response.completed")
	}
	if len(c.items) == 0 {
		return c.final, nil
	}
	var obj map[string]json.RawMessage
	if json.Unmarshal(c.final, &obj) != nil {
		return c.final, nil
	}
	var existing []json.RawMessage
	if json.Unmarshal(obj["output"], &existing) == nil && len(existing) > 0 {
		return c.final, nil
	}
	items := make([]json.RawMessage, 0, len(c.items))
	for _, i := range slices.Sorted(maps.Keys(c.items)) {
		items = append(items, c.items[i])
	}
	obj["output"] = jsonBytes(items)
	return marshalJSON(obj)
}

func (c *responsesCollector) Usage() core.Usage { return c.usage }

func (c *responsesCollector) UpstreamError() *UpstreamError { return c.upErr }
