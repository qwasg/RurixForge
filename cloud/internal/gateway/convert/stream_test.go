package convert

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"forge-cloud/internal/core"
)

type streamMeta struct {
	Model           string `json:"model"`
	IncludeUsage    *bool  `json:"includeUsage"`
	WantFeedError   bool   `json:"wantFeedError"`
	WantUpstreamErr string `json:"wantUpstreamError"`
	WantDone        *bool  `json:"wantDone"`
}

func loadStreamMeta(t *testing.T, dir, name string) streamMeta {
	t.Helper()
	path := filepath.Join("testdata", dir, name+".meta.json")
	b, err := os.ReadFile(path)
	if err != nil {
		m := streamMeta{Model: "forge-test"}
		inc := !strings.Contains(name, "no_usage")
		m.IncludeUsage = &inc
		return m
	}
	var m streamMeta
	if err := json.Unmarshal(b, &m); err != nil {
		t.Fatalf("meta %s: %v", path, err)
	}
	if m.Model == "" {
		m.Model = "forge-test"
	}
	if m.IncludeUsage == nil {
		inc := !strings.Contains(name, "no_usage")
		m.IncludeUsage = &inc
	}
	return m
}

func runStreamToChat(t *testing.T, dir string, mk func(model string, includeUsage bool) ToChat) {
	t.Helper()
	for _, name := range fixtures(t, dir, ".sse") {
		t.Run(name, func(t *testing.T) {
			meta := loadStreamMeta(t, dir, name)
			evs := readEvents(t, fixture(t, dir+"/"+name+".sse"))
			tc := mk(meta.Model, *meta.IncludeUsage)
			var out []byte
			var feedErr error
			for _, ev := range evs {
				frames, err := tc.Feed(ev)
				if err != nil {
					feedErr = err
					break
				}
				out = append(out, frames...)
			}
			if meta.WantFeedError {
				if feedErr == nil {
					t.Fatal("期望 Feed 报错")
				}
				return
			}
			if feedErr != nil {
				t.Fatalf("Feed: %v", feedErr)
			}
			out = append(out, tc.Finish()...)
			golden(t, dir+"/"+name+".golden.sse", out)

			if meta.WantUpstreamErr != "" {
				ue := tc.UpstreamError()
				if ue == nil {
					t.Fatalf("期望 UpstreamError 含 %q", meta.WantUpstreamErr)
				}
				if !strings.Contains(ue.Message+ue.Code+ue.Type, meta.WantUpstreamErr) {
					t.Fatalf("UpstreamError %+v 不含 %q", ue, meta.WantUpstreamErr)
				}
			}
			wantDone := true
			if meta.WantDone != nil {
				wantDone = *meta.WantDone
			}
			if tc.Done() != wantDone {
				t.Fatalf("Done()=%v want %v", tc.Done(), wantDone)
			}
		})
	}
}

func TestResponsesToChat(t *testing.T) {
	runStreamToChat(t, "responses2chat", NewResponsesToChat)
}

func TestMessagesToChat(t *testing.T) {
	runStreamToChat(t, "messages2chat", NewMessagesToChat)
}

type nonstreamMeta struct {
	Model     string `json:"model"`
	WantError string `json:"wantError"`
}

func loadNonstreamMeta(t *testing.T, name string, body []byte) nonstreamMeta {
	t.Helper()
	path := filepath.Join("testdata", "nonstream", name+".meta.json")
	var m nonstreamMeta
	if b, err := os.ReadFile(path); err == nil {
		_ = json.Unmarshal(b, &m)
	}
	if m.Model == "" {
		var probe struct {
			Model string `json:"model"`
		}
		_ = json.Unmarshal(body, &probe)
		m.Model = probe.Model
	}
	if m.Model == "" {
		m.Model = "forge-test"
	}
	return m
}

func TestChatCompletionFromResponses(t *testing.T) {
	for _, name := range fixtures(t, "nonstream", ".json") {
		if !strings.HasPrefix(name, "responses_") {
			continue
		}
		t.Run(name, func(t *testing.T) {
			body := fixture(t, "nonstream/"+name+".json")
			meta := loadNonstreamMeta(t, name, body)
			out, usage, err := ChatCompletionFromResponses(body, meta.Model)
			if meta.WantError != "" {
				if err == nil {
					t.Fatal("期望转换失败")
				}
				if !strings.Contains(err.Error(), meta.WantError) {
					t.Fatalf("错误 %q 不含 %q", err, meta.WantError)
				}
				return
			}
			if err != nil {
				t.Fatal(err)
			}
			golden(t, "nonstream/"+name+".golden.json", []byte(pretty(t, out)+"\n"))
			goldenUsage(t, "nonstream/"+name+".usage.json", usage)
		})
	}
}

func TestChatCompletionFromMessages(t *testing.T) {
	for _, name := range fixtures(t, "nonstream", ".json") {
		if !strings.HasPrefix(name, "messages_") {
			continue
		}
		t.Run(name, func(t *testing.T) {
			body := fixture(t, "nonstream/"+name+".json")
			meta := loadNonstreamMeta(t, name, body)
			out, usage, err := ChatCompletionFromMessages(body, meta.Model)
			if meta.WantError != "" {
				if err == nil {
					t.Fatal("期望转换失败")
				}
				if !strings.Contains(err.Error(), meta.WantError) {
					t.Fatalf("错误 %q 不含 %q", err, meta.WantError)
				}
				return
			}
			if err != nil {
				t.Fatal(err)
			}
			golden(t, "nonstream/"+name+".golden.json", []byte(pretty(t, out)+"\n"))
			goldenUsage(t, "nonstream/"+name+".usage.json", usage)
		})
	}
}

func goldenUsage(t *testing.T, name string, u core.Usage) {
	t.Helper()
	golden(t, name, []byte(compact(t, u)+"\n"))
}

type collectorMeta struct {
	WantError string `json:"wantError"`
}

func TestResponsesCollector(t *testing.T) {
	for _, name := range fixtures(t, "collector", ".sse") {
		t.Run(name, func(t *testing.T) {
			var meta collectorMeta
			if b, err := os.ReadFile(filepath.Join("testdata", "collector", name+".meta.json")); err == nil {
				_ = json.Unmarshal(b, &meta)
			}
			col := NewResponsesCollector()
			for _, ev := range readEvents(t, fixture(t, "collector/"+name+".sse")) {
				col.Feed(ev)
			}
			out, err := col.Result()
			if meta.WantError != "" {
				if err == nil {
					t.Fatal("期望 Result 失败")
				}
				if !strings.Contains(err.Error(), meta.WantError) {
					t.Fatalf("错误 %q 不含 %q", err, meta.WantError)
				}
				return
			}
			if err != nil {
				t.Fatal(err)
			}
			golden(t, "collector/"+name+".golden.json", []byte(pretty(t, out)+"\n"))
			goldenUsage(t, "collector/"+name+".usage.json", col.Usage())
		})
	}
}
