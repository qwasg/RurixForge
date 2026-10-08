package convert

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
)

// ---------- chat.completions 请求解析（两个请求转换共用）----------

type chatRequest struct {
	Model               string          `json:"model"`
	Messages            []chatMessage   `json:"messages"`
	Tools               []chatTool      `json:"tools"`
	ToolChoice          json.RawMessage `json:"tool_choice"`
	ParallelToolCalls   *bool           `json:"parallel_tool_calls"`
	Stream              bool            `json:"stream"`
	N                   json.RawMessage `json:"n"`
	MaxTokens           json.RawMessage `json:"max_tokens"`
	MaxCompletionTokens json.RawMessage `json:"max_completion_tokens"`
	Temperature         json.RawMessage `json:"temperature"`
	TopP                json.RawMessage `json:"top_p"`
	Stop                json.RawMessage `json:"stop"`
	ReasoningEffort     string          `json:"reasoning_effort"`
	ThinkingEnabled     *bool           `json:"thinking_enabled"`
	ResponseFormat      json.RawMessage `json:"response_format"`
	Verbosity           string          `json:"verbosity"`
	User                string          `json:"user"`
	PromptCacheKey      string          `json:"prompt_cache_key"`
}

type chatMessage struct {
	Role       string          `json:"role"`
	Content    json.RawMessage `json:"content"`
	ToolCalls  []chatToolCall  `json:"tool_calls"`
	ToolCallID string          `json:"tool_call_id"`
	// Messages 转换返回的完整 assistant 内容；包括不能修改的 thinking/signature 和原始块顺序。
	AnthropicContent []json.RawMessage `json:"anthropic_content"`
}

type chatToolCall struct {
	ID       string `json:"id"`
	Type     string `json:"type"`
	Function struct {
		Name string `json:"name"`
		// 通常是 JSON 字符串；个别客户端直接给对象（rawString 取其紧凑原文）。
		Arguments json.RawMessage `json:"arguments"`
	} `json:"function"`
}

type chatTool struct {
	Type     string `json:"type"`
	Function struct {
		Name        string          `json:"name"`
		Description string          `json:"description"`
		Parameters  json.RawMessage `json:"parameters"`
		Strict      *bool           `json:"strict"`
	} `json:"function"`
}

var defaultToolSchema = json.RawMessage(`{"type":"object","properties":{}}`)

func parseChatRequest(body []byte) (*chatRequest, error) {
	t := bytes.TrimSpace(body)
	if len(t) == 0 || t[0] != '{' {
		return nil, errors.New("convert: 请求体必须是 JSON 对象")
	}
	var req chatRequest
	if err := json.Unmarshal(t, &req); err != nil {
		return nil, fmt.Errorf("convert: 请求体无效: %w", err)
	}
	if n, ok := rawNumber(req.N); ok && n > 1 {
		return nil, errors.New("convert: 不支持 n > 1")
	}
	return &req, nil
}

// maxTokens：max_completion_tokens 优先（OpenAI 已以其取代 max_tokens），其次 max_tokens；非正数视为未给。
func maxTokens(req *chatRequest) (int64, bool) {
	for _, raw := range []json.RawMessage{req.MaxCompletionTokens, req.MaxTokens} {
		if f, ok := rawNumber(raw); ok && f >= 1 {
			return int64(f), true
		}
	}
	return 0, false
}

// contentPart 是解析后的消息内容块：文本或图片（未知类型的块已丢弃）。
type contentPart struct {
	text    string
	isImage bool
	url     string // data URI 或 http(s) URL
	detail  string
}

// parseContent：content 为字符串（单个文本块）、内容块数组或 null。
func parseContent(raw json.RawMessage) ([]contentPart, error) {
	t := bytes.TrimSpace(raw)
	if isNull(t) {
		return nil, nil
	}
	switch t[0] {
	case '"':
		return []contentPart{{text: rawString(t)}}, nil
	case '[':
		var items []json.RawMessage
		if err := json.Unmarshal(t, &items); err != nil {
			return nil, err
		}
		parts := make([]contentPart, 0, len(items))
		for _, item := range items {
			item = bytes.TrimSpace(item)
			if len(item) > 0 && item[0] == '"' {
				parts = append(parts, contentPart{text: rawString(item)})
				continue
			}
			var p struct {
				Type     string          `json:"type"`
				Text     json.RawMessage `json:"text"`
				ImageURL json.RawMessage `json:"image_url"`
			}
			if err := json.Unmarshal(item, &p); err != nil {
				return nil, fmt.Errorf("内容块无效: %w", err)
			}
			switch p.Type {
			case "text":
				parts = append(parts, contentPart{text: rawString(p.Text)})
			case "image_url":
				url, detail := parseImageURL(p.ImageURL)
				if url == "" {
					return nil, errors.New("image_url 缺少 url")
				}
				parts = append(parts, contentPart{isImage: true, url: url, detail: detail})
			}
		}
		return parts, nil
	}
	return nil, errors.New("content 必须是字符串或内容块数组")
}

// parseImageURL：{url, detail}，也接受直接给字符串。
func parseImageURL(raw json.RawMessage) (url, detail string) {
	t := bytes.TrimSpace(raw)
	if isNull(t) {
		return "", ""
	}
	if t[0] == '"' {
		return rawString(t), ""
	}
	var o struct {
		URL    string `json:"url"`
		Detail string `json:"detail"`
	}
	_ = json.Unmarshal(t, &o)
	return o.URL, o.Detail
}

// joinText 拼接文本块（不插分隔符，空白由客户端决定）；图片忽略。
func joinText(parts []contentPart) string {
	var sb strings.Builder
	for _, p := range parts {
		if !p.isImage {
			sb.WriteString(p.text)
		}
	}
	return sb.String()
}
