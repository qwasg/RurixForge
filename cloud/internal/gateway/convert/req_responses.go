package convert

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
)

// ---------- chat.completions → Responses 请求 ----------

type respRequest struct {
	Model             string          `json:"model"`
	Instructions      string          `json:"instructions,omitempty"`
	Input             []any           `json:"input"`
	Tools             []respTool      `json:"tools,omitempty"`
	ToolChoice        any             `json:"tool_choice,omitempty"`
	ParallelToolCalls *bool           `json:"parallel_tool_calls,omitempty"`
	Reasoning         *respReasoning  `json:"reasoning,omitempty"`
	Stream            bool            `json:"stream"`
	Store             *bool           `json:"store,omitempty"`
	Include           []string        `json:"include,omitempty"`
	PromptCacheKey    string          `json:"prompt_cache_key,omitempty"`
	Text              *respTextConfig `json:"text,omitempty"`
	MaxOutputTokens   *int64          `json:"max_output_tokens,omitempty"`
	Temperature       json.RawMessage `json:"temperature,omitempty"`
	TopP              json.RawMessage `json:"top_p,omitempty"`
	User              string          `json:"user,omitempty"`
}

type respInputMessage struct {
	Type    string `json:"type"`
	Role    string `json:"role"`
	Content []any  `json:"content"`
}

type respTextPart struct {
	Type string `json:"type"`
	Text string `json:"text"`
}

type respImagePart struct {
	Type     string `json:"type"`
	ImageURL string `json:"image_url"`
	Detail   string `json:"detail,omitempty"`
}

type respFunctionCall struct {
	Type      string `json:"type"`
	CallID    string `json:"call_id"`
	Name      string `json:"name"`
	Arguments string `json:"arguments"`
}

type respFunctionCallOutput struct {
	Type   string `json:"type"`
	CallID string `json:"call_id"`
	Output string `json:"output"`
}

type respTool struct {
	Type        string          `json:"type"`
	Name        string          `json:"name"`
	Description string          `json:"description,omitempty"`
	Parameters  json.RawMessage `json:"parameters"`
	Strict      bool            `json:"strict"`
}

type respNamedToolChoice struct {
	Type string `json:"type"`
	Name string `json:"name"`
}

type respReasoning struct {
	Effort  string `json:"effort"`
	Summary string `json:"summary"`
}

type respTextConfig struct {
	Format    any    `json:"format,omitempty"`
	Verbosity string `json:"verbosity,omitempty"`
}

type respJSONSchemaFormat struct {
	Type        string          `json:"type"`
	Name        string          `json:"name"`
	Schema      json.RawMessage `json:"schema,omitempty"`
	Strict      *bool           `json:"strict,omitempty"`
	Description string          `json:"description,omitempty"`
}

type respTypeOnly struct {
	Type string `json:"type"`
}

func chatToResponses(body []byte, opts ChatToResponsesOptions) ([]byte, error) {
	req, err := parseChatRequest(body)
	if err != nil {
		return nil, err
	}
	out := respRequest{
		Model:             firstNonEmpty(opts.Model, req.Model),
		Input:             []any{},
		ParallelToolCalls: req.ParallelToolCalls,
		Stream:            opts.Stream,
		Store:             opts.Store,
		PromptCacheKey:    firstNonEmpty(opts.PromptCacheKey, req.PromptCacheKey),
		User:              req.User,
	}
	var system []string
	for i, m := range req.Messages {
		parts, err := parseContent(m.Content)
		if err != nil {
			return nil, fmt.Errorf("convert: messages[%d].content 无效: %w", i, err)
		}
		switch m.Role {
		case "system", "developer":
			if opts.Instructions == "" {
				if t := joinText(parts); t != "" {
					system = append(system, t)
				}
				continue
			}
			if content := respTextContent(parts, "input_text"); len(content) > 0 {
				out.Input = append(out.Input, respInputMessage{Type: "message", Role: "developer", Content: content})
			}
		case "user":
			var content []any
			for _, p := range parts {
				switch {
				case p.isImage:
					content = append(content, respImagePart{Type: "input_image", ImageURL: p.url, Detail: p.detail})
				case p.text != "":
					content = append(content, respTextPart{Type: "input_text", Text: p.text})
				}
			}
			if len(content) > 0 {
				out.Input = append(out.Input, respInputMessage{Type: "message", Role: "user", Content: content})
			}
		case "assistant":
			if content := respTextContent(parts, "output_text"); len(content) > 0 {
				out.Input = append(out.Input, respInputMessage{Type: "message", Role: "assistant", Content: content})
			}
			for _, tc := range m.ToolCalls {
				out.Input = append(out.Input, respFunctionCall{
					Type:      "function_call",
					CallID:    tc.ID,
					Name:      tc.Function.Name,
					Arguments: firstNonEmpty(rawString(tc.Function.Arguments), "{}"),
				})
			}
		case "tool":
			out.Input = append(out.Input, respFunctionCallOutput{
				Type: "function_call_output", CallID: m.ToolCallID, Output: joinText(parts),
			})
		default:
			return nil, fmt.Errorf("convert: messages[%d] 的角色 %q 不受支持", i, m.Role)
		}
	}
	out.Instructions = opts.Instructions
	if out.Instructions == "" {
		out.Instructions = strings.Join(system, "\n\n")
	}

	for _, t := range req.Tools {
		if t.Type != "" && t.Type != "function" {
			continue
		}
		params := t.Function.Parameters
		if isNull(params) {
			params = defaultToolSchema
		}
		out.Tools = append(out.Tools, respTool{
			Type:        "function",
			Name:        t.Function.Name,
			Description: t.Function.Description,
			Parameters:  params,
			Strict:      t.Function.Strict != nil && *t.Function.Strict,
		})
	}
	out.ToolChoice = respToolChoice(req.ToolChoice)

	if req.ReasoningEffort != "" {
		out.Reasoning = &respReasoning{Effort: req.ReasoningEffort, Summary: "auto"}
	}
	if opts.Store != nil && !*opts.Store {
		out.Include = []string{"reasoning.encrypted_content"}
	}
	format, err := respTextFormat(req.ResponseFormat)
	if err != nil {
		return nil, err
	}
	if format != nil || req.Verbosity != "" {
		out.Text = &respTextConfig{Format: format, Verbosity: req.Verbosity}
	}
	if !opts.DropSampling {
		if n, ok := maxTokens(req); ok {
			out.MaxOutputTokens = &n
		}
		if !isNull(req.Temperature) {
			out.Temperature = req.Temperature
		}
		if !isNull(req.TopP) {
			out.TopP = req.TopP
		}
	}
	return marshalJSON(out)
}

func respTextContent(parts []contentPart, typ string) []any {
	var content []any
	for _, p := range parts {
		if !p.isImage && p.text != "" {
			content = append(content, respTextPart{Type: typ, Text: p.text})
		}
	}
	return content
}

// respToolChoice："auto"/"none"/"required" 原样；{type:function,function:{name}} → {type:function,name}；其它对象原样透传。
func respToolChoice(raw json.RawMessage) any {
	t := bytes.TrimSpace(raw)
	if isNull(t) {
		return nil
	}
	if t[0] == '"' {
		return rawString(t)
	}
	var o struct {
		Type     string `json:"type"`
		Name     string `json:"name"`
		Function struct {
			Name string `json:"name"`
		} `json:"function"`
	}
	if json.Unmarshal(t, &o) == nil && o.Type == "function" {
		return respNamedToolChoice{Type: "function", Name: firstNonEmpty(o.Function.Name, o.Name)}
	}
	return json.RawMessage(t)
}

// respTextFormat：json_object → {type:json_object}；json_schema → {type:json_schema,name,schema,strict,description}；
// text 或缺省 → nil（上游默认即文本）。
func respTextFormat(raw json.RawMessage) (any, error) {
	if isNull(raw) {
		return nil, nil
	}
	var rf struct {
		Type       string `json:"type"`
		JSONSchema *struct {
			Name        string          `json:"name"`
			Description string          `json:"description"`
			Schema      json.RawMessage `json:"schema"`
			Strict      *bool           `json:"strict"`
		} `json:"json_schema"`
	}
	if err := json.Unmarshal(raw, &rf); err != nil {
		return nil, fmt.Errorf("convert: response_format 无效: %w", err)
	}
	switch rf.Type {
	case "json_object":
		return respTypeOnly{Type: "json_object"}, nil
	case "json_schema":
		js := rf.JSONSchema
		if js == nil {
			return nil, errors.New("convert: response_format 缺少 json_schema")
		}
		f := respJSONSchemaFormat{
			Type:        "json_schema",
			Name:        firstNonEmpty(js.Name, "response"),
			Strict:      js.Strict,
			Description: js.Description,
		}
		if !isNull(js.Schema) {
			f.Schema = js.Schema
		}
		return f, nil
	}
	return nil, nil
}
