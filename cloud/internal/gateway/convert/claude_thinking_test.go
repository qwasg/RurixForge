package convert

import (
	"encoding/json"
	"reflect"
	"strings"
	"testing"
)

func TestLatestClaudeAdaptiveThinking(t *testing.T) {
	for _, model := range []string{"claude-sonnet-5-5", "claude-opus-5-5", "claude-fable-5-1"} {
		t.Run(model, func(t *testing.T) {
			for _, effort := range []string{"low", "medium", "high", "xhigh", "max"} {
				body, _ := json.Marshal(map[string]any{"model": model, "reasoning_effort": effort, "max_tokens": 2048,
					"temperature": 0.3, "top_p": 0.8, "messages": []any{map[string]any{"role": "user", "content": "Solve this."}}})
				out, err := ChatToMessagesRequest(body, ChatToMessagesOptions{})
				if err != nil {
					t.Fatal(err)
				}
				var req anthRequest
				if err := json.Unmarshal(out, &req); err != nil {
					t.Fatal(err)
				}
				if req.Thinking == nil || req.Thinking.Type != "adaptive" || req.Thinking.Display != "summarized" || req.Thinking.BudgetTokens != 0 {
					t.Fatalf("invalid thinking: %s", out)
				}
				if req.OutputConfig == nil || req.OutputConfig.Effort != effort || req.MaxTokens != 2048 || len(req.Temperature) != 0 || len(req.TopP) != 0 {
					t.Fatalf("wrong effort/output limit/sampling: %s", out)
				}
			}
		})
	}
}

func TestLatestClaudeThinkingOffAndForcedTools(t *testing.T) {
	for _, model := range []string{"claude-sonnet-5-5", "claude-opus-5-5", "claude-fable-5-1"} {
		body := []byte(`{"messages":[{"role":"user","content":"Hello"}],"reasoning_effort":"max","thinking_enabled":false}`)
		out, err := ChatToMessagesRequest(body, ChatToMessagesOptions{Model: model})
		if err != nil {
			t.Fatal(err)
		}
		var req anthRequest
		_ = json.Unmarshal(out, &req)
		if req.OutputConfig.Effort != "low" {
			t.Fatalf("off must lower effort: %s", out)
		}
		if model == "claude-sonnet-5-5" {
			if req.Thinking.Type != "between_tools" || req.Thinking.Display != "" {
				t.Fatalf("invalid Sonnet off: %s", out)
			}
		} else if req.Thinking.Type != "adaptive" || req.Thinking.Display != "omitted" {
			t.Fatalf("always-on cannot be disabled: %s", out)
		}
		_, err = ChatToMessagesRequest([]byte(`{"messages":[{"role":"user","content":"Call a tool"}],"tools":[{"type":"function","function":{"name":"lookup"}}],"tool_choice":"required"}`), ChatToMessagesOptions{Model: model})
		if err == nil || !strings.Contains(err.Error(), "tool_choice=auto") {
			t.Fatalf("forced tools should fail clearly: %v", err)
		}
	}
}

func TestSignedThinkingToolRoundTrip(t *testing.T) {
	native := []byte(`{"id":"msg_signed","type":"message","content":[{"type":"thinking","thinking":"original summary","signature":"opaque-sig+/="},{"type":"redacted_thinking","data":"opaque-redacted+/="},{"type":"text","text":"Checking."},{"type":"tool_use","id":"toolu_1","name":"lookup","input":{"city":"Taipei"}}],"stop_reason":"tool_use"}`)
	chat, _, err := ChatCompletionFromMessages(native, "claude-haiku-4-5")
	if err != nil {
		t.Fatal(err)
	}
	var response struct {
		Choices []struct {
			Message json.RawMessage `json:"message"`
		} `json:"choices"`
	}
	_ = json.Unmarshal(chat, &response)
	body, _ := json.Marshal(map[string]any{"model": "claude-haiku-4-5", "reasoning_effort": "low", "thinking_enabled": true,
		"messages": []any{map[string]any{"role": "user", "content": "Weather?"}, response.Choices[0].Message, map[string]any{"role": "tool", "tool_call_id": "toolu_1", "content": "22 C"}}})
	out, err := ChatToMessagesRequest(body, ChatToMessagesOptions{ThinkingMode: "manual"})
	if err != nil {
		t.Fatal(err)
	}
	var req struct {
		Thinking *anthThinking `json:"thinking"`
		Messages []struct {
			Content json.RawMessage `json:"content"`
		} `json:"messages"`
	}
	_ = json.Unmarshal(out, &req)
	if req.Thinking == nil || req.Thinking.Type != "enabled" || req.Thinking.Display != "summarized" {
		t.Fatalf("tool result lost thinking: %s", out)
	}
	var source struct {
		Content any `json:"content"`
	}
	_ = json.Unmarshal(native, &source)
	var preserved any
	_ = json.Unmarshal(req.Messages[1].Content, &preserved)
	if !reflect.DeepEqual(source.Content, preserved) {
		t.Fatalf("thinking/signature/block order was changed: %s", out)
	}
}

func TestStreamSignedThinkingReassemblesNativeBlocks(t *testing.T) {
	c := NewMessagesToChat("claude-opus-5-5", false)
	frames := []string{
		`{"type":"message_start","message":{"id":"msg_stream"}}`,
		`{"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"","signature":""}}`,
		`{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"summary"}}`,
		`{"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"sig-part-"}}`,
		`{"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"two"}}`,
		`{"type":"content_block_stop","index":0}`,
		`{"type":"content_block_start","index":1,"content_block":{"type":"redacted_thinking","data":"encrypted"}}`,
		`{"type":"content_block_stop","index":1}`,
		`{"type":"content_block_start","index":2,"content_block":{"type":"tool_use","id":"toolu_1","name":"lookup","input":{}}}`,
		`{"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"{\"city\":"}}`,
		`{"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"\"Taipei\"}"}}`,
		`{"type":"content_block_stop","index":2}`,
		`{"type":"message_stop"}`,
	}
	var stream []byte
	for _, data := range frames {
		out, err := c.Feed(Event{Data: []byte(data)})
		if err != nil {
			t.Fatal(err)
		}
		stream = append(stream, out...)
	}
	if !strings.Contains(string(stream), `"anthropic_content"`) {
		t.Fatal("signed stream lacks native history extension")
	}
	var response struct {
		Choices []struct {
			Message struct {
				Content []map[string]any `json:"anthropic_content"`
			} `json:"message"`
		} `json:"choices"`
	}
	if err := json.Unmarshal(c.Completion(), &response); err != nil {
		t.Fatal(err)
	}
	blocks := response.Choices[0].Message.Content
	if len(blocks) != 3 || blocks[0]["thinking"] != "summary" || blocks[0]["signature"] != "sig-part-two" || blocks[1]["data"] != "encrypted" || !reflect.DeepEqual(blocks[2]["input"], map[string]any{"city": "Taipei"}) {
		t.Fatalf("invalid native history: %#v", blocks)
	}
}
