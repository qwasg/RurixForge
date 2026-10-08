package convert

import (
	"testing"

	"forge-cloud/internal/core"
)

func TestUsageTaps(t *testing.T) {
	for _, name := range fixtures(t, "usage", ".sse") {
		t.Run(name, func(t *testing.T) {
			var tap UsageTap
			switch {
			case name == "chat_stream" || name == "chat_deepseek_stream":
				tap = NewChatUsageTap()
			case name == "responses":
				tap = NewResponsesUsageTap()
			case name == "messages":
				tap = NewMessagesUsageTap()
			default:
				t.Fatalf("未知 usage 夹具 %q", name)
			}
			for _, ev := range readEvents(t, fixture(t, "usage/"+name+".sse")) {
				tap.Feed(ev)
			}
			goldenUsage(t, "usage/"+name+".usage.json", tap.Usage())
		})
	}
}

func TestUsageFromBody(t *testing.T) {
	cases := []struct {
		name string
		fn   func([]byte) core.Usage
	}{
		{"chat_openai", ChatUsageFromBody},
		{"chat_deepseek", ChatUsageFromBody},
		{"responses", ResponsesUsageFromBody},
		{"messages", MessagesUsageFromBody},
		{"embeddings", EmbeddingsUsageFromBody},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			body := fixture(t, "usage/"+c.name+".json")
			goldenUsage(t, "usage/from_body/"+c.name+".usage.json", c.fn(body))
		})
	}
}

func TestErrorFromBody(t *testing.T) {
	for _, name := range fixtures(t, "error", ".json") {
		t.Run(name, func(t *testing.T) {
			body := fixture(t, "error/"+name+".json")
			e := ErrorFromBody(body)
			golden(t, "error/"+name+".golden.json", []byte(compact(t, e)+"\n"))
		})
	}
}
