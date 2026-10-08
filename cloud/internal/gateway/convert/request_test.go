package convert

import (
	"encoding/json"
	"strings"
	"testing"
)

// requestFixture：options 为转换选项，request 为 chat.completions 请求体；wantError 非空时期望转换失败且错误含该子串。
type requestFixture struct {
	Options   json.RawMessage `json:"options"`
	Request   json.RawMessage `json:"request"`
	WantError string          `json:"wantError"`
}

// 字段与 ChatToResponsesOptions / ChatToMessagesOptions 一一对应（可直接类型转换）。
type responsesOptionsFixture struct {
	Model          string `json:"model"`
	Instructions   string `json:"instructions"`
	PromptCacheKey string `json:"promptCacheKey"`
	Stream         bool   `json:"stream"`
	Store          *bool  `json:"store"`
	DropSampling   bool   `json:"dropSampling"`
}

type messagesOptionsFixture struct {
	Model            string `json:"model"`
	DefaultMaxTokens int    `json:"defaultMaxTokens"`
	ThinkingMode     string `json:"thinkingMode"`
	ThinkingAlwaysOn bool   `json:"thinkingAlwaysOn"`
}

func loadRequestFixture(t *testing.T, name string, opts any) requestFixture {
	t.Helper()
	var f requestFixture
	if err := json.Unmarshal(fixture(t, name), &f); err != nil {
		t.Fatalf("夹具 %s 无效: %v", name, err)
	}
	if len(f.Options) > 0 {
		if err := json.Unmarshal(f.Options, opts); err != nil {
			t.Fatalf("夹具 %s 的 options 无效: %v", name, err)
		}
	}
	return f
}

func checkRequestResult(t *testing.T, base, wantErr string, out []byte, err error) {
	t.Helper()
	if wantErr != "" {
		if err == nil {
			t.Fatalf("期望错误含 %q，实际成功: %s", wantErr, out)
		}
		if !strings.Contains(err.Error(), wantErr) {
			t.Fatalf("错误 %q 不含 %q", err, wantErr)
		}
		return
	}
	if err != nil {
		t.Fatalf("转换失败: %v", err)
	}
	golden(t, base+".golden.json", []byte(pretty(t, out)+"\n"))
}

func TestChatToResponsesRequest(t *testing.T) {
	for _, name := range fixtures(t, "chat2responses", ".json") {
		t.Run(name, func(t *testing.T) {
			var o responsesOptionsFixture
			f := loadRequestFixture(t, "chat2responses/"+name+".json", &o)
			out, err := ChatToResponsesRequest(f.Request, ChatToResponsesOptions(o))
			checkRequestResult(t, "chat2responses/"+name, f.WantError, out, err)
		})
	}
}

func TestChatToMessagesRequest(t *testing.T) {
	for _, name := range fixtures(t, "chat2messages", ".json") {
		t.Run(name, func(t *testing.T) {
			var o messagesOptionsFixture
			f := loadRequestFixture(t, "chat2messages/"+name+".json", &o)
			out, err := ChatToMessagesRequest(f.Request, ChatToMessagesOptions(o))
			checkRequestResult(t, "chat2messages/"+name, f.WantError, out, err)
		})
	}
}

func TestRequestConvertersRejectMalformedBodies(t *testing.T) {
	bodies := []string{``, `[]`, `"text"`, `{"messages": [`, `{"messages": "hi"}`, `{"stream": "yes"}`}
	for _, body := range bodies {
		if _, err := ChatToResponsesRequest([]byte(body), ChatToResponsesOptions{}); err == nil {
			t.Errorf("ChatToResponsesRequest(%q) 应报错", body)
		}
		if _, err := ChatToMessagesRequest([]byte(body), ChatToMessagesOptions{}); err == nil {
			t.Errorf("ChatToMessagesRequest(%q) 应报错", body)
		}
	}
}
