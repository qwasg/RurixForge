package convert

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
)

// ---------- chat.completions → Anthropic Messages 请求 ----------

type anthRequest struct {
	Model         string            `json:"model"`
	MaxTokens     int64             `json:"max_tokens"`
	System        string            `json:"system,omitempty"`
	Messages      []anthMessage     `json:"messages"`
	Tools         []anthTool        `json:"tools,omitempty"`
	ToolChoice    *anthToolChoice   `json:"tool_choice,omitempty"`
	Thinking      *anthThinking     `json:"thinking,omitempty"`
	OutputConfig  *anthOutputConfig `json:"output_config,omitempty"`
	Stream        bool              `json:"stream,omitempty"`
	Temperature   json.RawMessage   `json:"temperature,omitempty"`
	TopP          json.RawMessage   `json:"top_p,omitempty"`
	StopSequences []string          `json:"stop_sequences,omitempty"`
	Metadata      *anthMetadata     `json:"metadata,omitempty"`
}

type anthMessage struct {
	Role    string `json:"role"`
	Content []any  `json:"content"`
}

type anthTextBlock struct {
	Type string `json:"type"`
	Text string `json:"text"`
}

type anthImageBlock struct {
	Type   string          `json:"type"`
	Source anthImageSource `json:"source"`
}

type anthImageSource struct {
	Type      string `json:"type"`
	MediaType string `json:"media_type,omitempty"`
	Data      string `json:"data,omitempty"`
	URL       string `json:"url,omitempty"`
}

type anthToolUseBlock struct {
	Type  string          `json:"type"`
	ID    string          `json:"id"`
	Name  string          `json:"name"`
	Input json.RawMessage `json:"input"`
}

type anthToolResultBlock struct {
	Type      string `json:"type"`
	ToolUseID string `json:"tool_use_id"`
	Content   string `json:"content,omitempty"`
}

type anthTool struct {
	Name        string          `json:"name"`
	Description string          `json:"description,omitempty"`
	InputSchema json.RawMessage `json:"input_schema"`
}

type anthToolChoice struct {
	Type                   string `json:"type"`
	Name                   string `json:"name,omitempty"`
	DisableParallelToolUse bool   `json:"disable_parallel_tool_use,omitempty"`
}

type anthThinking struct {
	Type         string `json:"type"`
	BudgetTokens int64  `json:"budget_tokens,omitempty"`
	Display      string `json:"display,omitempty"`
}

type anthOutputConfig struct {
	Effort string `json:"effort"`
}

type anthMetadata struct {
	UserID string `json:"user_id"`
}

func chatToMessages(body []byte, opts ChatToMessagesOptions) ([]byte, error) {
	req, err := parseChatRequest(body)
	if err != nil {
		return nil, err
	}
	var system []string
	var msgs []anthMessage
	// push 追加内容块；与上一条同角色时合并（Anthropic 要求 user/assistant 交替，tool_result 属于 user）。
	push := func(role string, blocks ...any) {
		if len(blocks) == 0 {
			return
		}
		if n := len(msgs); n > 0 && msgs[n-1].Role == role {
			msgs[n-1].Content = append(msgs[n-1].Content, blocks...)
			return
		}
		msgs = append(msgs, anthMessage{Role: role, Content: blocks})
	}
	for i, m := range req.Messages {
		parts, err := parseContent(m.Content)
		if err != nil {
			return nil, fmt.Errorf("convert: messages[%d].content 无效: %w", i, err)
		}
		switch m.Role {
		case "system", "developer":
			if t := joinText(parts); t != "" {
				system = append(system, t)
			}
		case "user":
			var blocks []any
			for _, p := range parts {
				switch {
				case p.isImage:
					img, err := anthImage(p.url)
					if err != nil {
						return nil, fmt.Errorf("convert: messages[%d]: %w", i, err)
					}
					blocks = append(blocks, img)
				case p.text != "":
					blocks = append(blocks, anthTextBlock{Type: "text", Text: p.text})
				}
			}
			push("user", blocks...)
		case "assistant":
			if len(m.AnthropicContent) > 0 {
				for _, raw := range m.AnthropicContent {
					var block struct {
						Type string `json:"type"`
					}
					if json.Unmarshal(raw, &block) != nil || block.Type == "" {
						return nil, fmt.Errorf("convert: messages[%d].anthropic_content 必须包含 Messages 内容块", i)
					}
					push("assistant", raw)
				}
				continue
			}
			var blocks []any
			for _, p := range parts {
				if !p.isImage && p.text != "" {
					blocks = append(blocks, anthTextBlock{Type: "text", Text: p.text})
				}
			}
			for _, tc := range m.ToolCalls {
				blocks = append(blocks, anthToolUseBlock{
					Type:  "tool_use",
					ID:    anthToolID(tc.ID),
					Name:  tc.Function.Name,
					Input: argsObject(rawString(tc.Function.Arguments)),
				})
			}
			push("assistant", blocks...)
		case "tool":
			push("user", anthToolResultBlock{
				Type: "tool_result", ToolUseID: anthToolID(m.ToolCallID), Content: joinText(parts),
			})
		default:
			return nil, fmt.Errorf("convert: messages[%d] 的角色 %q 不受支持", i, m.Role)
		}
	}
	if len(msgs) == 0 || msgs[0].Role != "user" {
		msgs = append([]anthMessage{{Role: "user", Content: []any{anthTextBlock{Type: "text", Text: "(continue)"}}}}, msgs...)
	}

	out := anthRequest{
		Model:         firstNonEmpty(opts.Model, req.Model),
		System:        strings.Join(system, "\n\n"),
		Messages:      msgs,
		Stream:        req.Stream,
		StopSequences: stopSequences(req.Stop),
	}
	if req.User != "" {
		out.Metadata = &anthMetadata{UserID: req.User}
	}

	for _, t := range req.Tools {
		if t.Type != "" && t.Type != "function" {
			continue
		}
		schema := t.Function.Parameters
		if isNull(schema) {
			schema = defaultToolSchema
		}
		out.Tools = append(out.Tools, anthTool{Name: t.Function.Name, Description: t.Function.Description, InputSchema: schema})
	}
	// Anthropic 不允许无 tools 时给 tool_choice。
	if len(out.Tools) > 0 {
		out.ToolChoice = anthToolChoiceFrom(req.ToolChoice)
		if req.ParallelToolCalls != nil && !*req.ParallelToolCalls {
			if out.ToolChoice == nil {
				out.ToolChoice = &anthToolChoice{Type: "auto"}
			}
			if out.ToolChoice.Type != "none" {
				out.ToolChoice.DisableParallelToolUse = true
			}
		}
	}

	maxTok := int64(opts.DefaultMaxTokens)
	if n, ok := maxTokens(req); ok {
		maxTok = n
	} else if maxTok <= 0 {
		maxTok = 8192
	}
	adaptive, alwaysOn, betweenTools := adaptiveThinking(out.Model, opts)
	if adaptive {
		if out.ToolChoice != nil && (out.ToolChoice.Type == "any" || out.ToolChoice.Type == "tool") && rejectsForcedTools(out.Model) {
			return nil, errors.New("convert: 此 Claude 模型不支持强制工具调用，请使用 tool_choice=auto")
		}
		effort := strings.ToLower(req.ReasoningEffort)
		off := effort == "none" || (req.ThinkingEnabled != nil && !*req.ThinkingEnabled)
		if effort == "minimal" || off {
			effort = "low"
		}
		if effort != "" && effort != "low" && effort != "medium" && effort != "high" && effort != "xhigh" && effort != "max" {
			return nil, errors.New("convert: Claude 自适应 thinking 的 reasoning_effort 只能是 low/medium/high/xhigh/max")
		}
		if off {
			effort = "low"
		}
		if effort != "" {
			out.OutputConfig = &anthOutputConfig{Effort: effort}
		}
		switch {
		case off && betweenTools:
			out.Thinking = &anthThinking{Type: "between_tools"}
		case off && !alwaysOn:
			out.Thinking = &anthThinking{Type: "disabled"}
		case off:
			// 始终思考的模型不能关闭；兼容接口的关闭只降低强度并隐藏摘要。
			out.Thinking = &anthThinking{Type: "adaptive", Display: "omitted"}
		default:
			out.Thinking = &anthThinking{Type: "adaptive", Display: "summarized"}
		}
		// 新模型不接受非默认 sampling；max_tokens 是总输出硬上限，不因 effort 增大。
		out.MaxTokens = maxTok
		return marshalJSON(out)
	}
	budget := thinkingBudget(req.ReasoningEffort)
	if req.ThinkingEnabled != nil {
		if !*req.ThinkingEnabled {
			budget = 0
		} else if budget == 0 {
			budget = 8192
		}
	}
	// thinking 与强制工具（any/tool）互斥；最后一条 assistant 含 tool_use 时 Anthropic 要求其以带签名的 thinking 块开头，
	// chat 历史里没有这些块，只能关闭 thinking。
	if budget > 0 && out.ToolChoice != nil && (out.ToolChoice.Type == "any" || out.ToolChoice.Type == "tool") {
		budget = 0
	}
	if budget > 0 && lastAssistantUsesTool(msgs) && !lastAssistantHasThinking(msgs) {
		budget = 0
	}
	if budget > 0 {
		out.Thinking = &anthThinking{Type: "enabled", BudgetTokens: budget}
		if opts.ThinkingMode == "manual" || strings.HasPrefix(out.Model, "claude-haiku-4-5") {
			out.Thinking.Display = "summarized"
		}
		if maxTok < budget+4096 {
			maxTok = budget + 4096
		}
	} else {
		out.Temperature = clampTemperature(req.Temperature)
		// 新款 Claude 不接受同时给 temperature 与 top_p：两者都给时只保留 temperature。
		if isNull(req.Temperature) && !isNull(req.TopP) {
			out.TopP = req.TopP
		}
	}
	out.MaxTokens = maxTok
	return marshalJSON(out)
}

// 当前模型名在目录配置缺失时也必须避免发送已废弃的 budget_tokens。
func adaptiveThinking(model string, opts ChatToMessagesOptions) (adaptive, alwaysOn, betweenTools bool) {
	sonnet := strings.HasPrefix(model, "claude-sonnet-5-5")
	opus := strings.HasPrefix(model, "claude-opus-5-5")
	fable := strings.HasPrefix(model, "claude-fable-5-1")
	return opts.ThinkingMode == "adaptive" || sonnet || opus || fable,
		opts.ThinkingAlwaysOn || opus || fable, sonnet
}

func rejectsForcedTools(model string) bool {
	return strings.HasPrefix(model, "claude-sonnet-5-5") || strings.HasPrefix(model, "claude-opus-5-5") || strings.HasPrefix(model, "claude-fable-5-1")
}

func lastAssistantHasThinking(msgs []anthMessage) bool {
	for i := len(msgs) - 1; i >= 0; i-- {
		if msgs[i].Role != "assistant" {
			continue
		}
		for _, b := range msgs[i].Content {
			raw, ok := b.(json.RawMessage)
			if !ok {
				continue
			}
			var block struct {
				Type      string `json:"type"`
				Signature string `json:"signature"`
				Data      string `json:"data"`
			}
			if json.Unmarshal(raw, &block) == nil && ((block.Type == "thinking" && block.Signature != "") || (block.Type == "redacted_thinking" && block.Data != "")) {
				return true
			}
		}
		return false
	}
	return false
}

// anthImage：base64 data URI → base64 源；http(s) URL → url 源；其它报错。
func anthImage(url string) (anthImageBlock, error) {
	if rest, ok := strings.CutPrefix(url, "data:"); ok {
		header, data, ok := strings.Cut(rest, ",")
		params := strings.Split(header, ";")
		if !ok || len(params) < 2 || params[0] == "" || !strings.EqualFold(params[len(params)-1], "base64") {
			return anthImageBlock{}, errors.New("image_url 仅支持 base64 编码的 data URI")
		}
		return anthImageBlock{Type: "image", Source: anthImageSource{
			Type: "base64", MediaType: strings.ToLower(params[0]), Data: data,
		}}, nil
	}
	lower := strings.ToLower(url)
	if strings.HasPrefix(lower, "https://") || strings.HasPrefix(lower, "http://") {
		return anthImageBlock{Type: "image", Source: anthImageSource{Type: "url", URL: url}}, nil
	}
	return anthImageBlock{}, errors.New("image_url 必须是 data URI 或 http(s) URL")
}

// anthToolID 把工具调用 id 中 Anthropic 不接受的字符（仅允许 [A-Za-z0-9_-]）替换为 _；
// tool_use.id 与 tool_result.tool_use_id 用同一映射，配对不受影响。
func anthToolID(id string) string {
	return strings.Map(func(r rune) rune {
		if r == '_' || r == '-' || (r >= '0' && r <= '9') || (r >= 'a' && r <= 'z') || (r >= 'A' && r <= 'Z') {
			return r
		}
		return '_'
	}, id)
}

// argsObject 把 arguments 转成 tool_use.input：必须是 JSON 对象，空或非法时用 {}。
func argsObject(args string) json.RawMessage {
	t := strings.TrimSpace(args)
	if t == "" || t[0] != '{' || !json.Valid([]byte(t)) {
		return json.RawMessage(`{}`)
	}
	return json.RawMessage(t)
}

// anthToolChoiceFrom：auto→auto、required→any、none→none、指定函数→tool；无法识别返回 nil（上游默认 auto）。
func anthToolChoiceFrom(raw json.RawMessage) *anthToolChoice {
	t := bytes.TrimSpace(raw)
	if isNull(t) {
		return nil
	}
	if t[0] == '"' {
		switch rawString(t) {
		case "auto":
			return &anthToolChoice{Type: "auto"}
		case "required":
			return &anthToolChoice{Type: "any"}
		case "none":
			return &anthToolChoice{Type: "none"}
		}
		return nil
	}
	var o struct {
		Type     string `json:"type"`
		Name     string `json:"name"`
		Function struct {
			Name string `json:"name"`
		} `json:"function"`
	}
	if json.Unmarshal(t, &o) == nil && o.Type == "function" {
		if name := firstNonEmpty(o.Function.Name, o.Name); name != "" {
			return &anthToolChoice{Type: "tool", Name: name}
		}
	}
	return nil
}

// thinkingBudget：reasoning_effort → thinking.budget_tokens（0 = 不开 thinking）。
func thinkingBudget(effort string) int64 {
	switch strings.ToLower(effort) {
	case "minimal":
		return 1024
	case "low":
		return 2048
	case "medium":
		return 8192
	case "high":
		return 16384
	case "xhigh", "max":
		return 32000
	}
	return 0
}

func lastAssistantUsesTool(msgs []anthMessage) bool {
	for i := len(msgs) - 1; i >= 0; i-- {
		if msgs[i].Role != "assistant" {
			continue
		}
		for _, b := range msgs[i].Content {
			if _, ok := b.(anthToolUseBlock); ok {
				return true
			}
			if raw, ok := b.(json.RawMessage); ok {
				var block struct {
					Type string `json:"type"`
				}
				if json.Unmarshal(raw, &block) == nil && block.Type == "tool_use" {
					return true
				}
			}
		}
		return false
	}
	return false
}

// clampTemperature：Anthropic temperature 取值 0~1（OpenAI 为 0~2），大于 1 截为 1。
func clampTemperature(raw json.RawMessage) json.RawMessage {
	if isNull(raw) {
		return nil
	}
	if f, ok := rawNumber(raw); ok && f > 1 {
		return json.RawMessage("1")
	}
	return raw
}

// stopSequences：stop 为字符串或字符串数组；丢弃空白串（Anthropic 拒绝纯空白的停止序列）。
func stopSequences(raw json.RawMessage) []string {
	t := bytes.TrimSpace(raw)
	if isNull(t) {
		return nil
	}
	var list []string
	if t[0] == '"' {
		list = []string{rawString(t)}
	} else {
		_ = json.Unmarshal(t, &list)
	}
	var out []string
	for _, s := range list {
		if strings.TrimSpace(s) != "" {
			out = append(out, s)
		}
	}
	return out
}
