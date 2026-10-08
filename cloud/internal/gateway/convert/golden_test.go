package convert

import (
	"bytes"
	"encoding/json"
	"errors"
	"flag"
	"io"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

var update = flag.Bool("update", false, "用当前输出重写 testdata 下的 golden 文件")

const (
	testNow = int64(1700000000)
	testID  = "test0000"
)

// TestMain 固定 created 与兜底 id，golden 输出因此确定。
func TestMain(m *testing.M) {
	nowUnix = func() int64 { return testNow }
	newID = func() string { return testID }
	os.Exit(m.Run())
}

func fixture(t *testing.T, name string) []byte {
	t.Helper()
	b, err := os.ReadFile(filepath.Join("testdata", filepath.FromSlash(name)))
	if err != nil {
		t.Fatalf("读取夹具: %v", err)
	}
	return b
}

// fixtures 列出 testdata/<dir> 下扩展名为 ext 的夹具（去掉扩展名，跳过 golden）。
func fixtures(t *testing.T, dir, ext string) []string {
	t.Helper()
	matches, err := filepath.Glob(filepath.Join("testdata", dir, "*"+ext))
	if err != nil {
		t.Fatal(err)
	}
	var names []string
	for _, m := range matches {
		base := filepath.Base(m)
		if strings.Contains(base, ".golden") || strings.Contains(base, ".usage.") || strings.Contains(base, ".meta.") {
			continue
		}
		names = append(names, strings.TrimSuffix(base, ext))
	}
	if len(names) == 0 {
		t.Fatalf("testdata/%s 下没有 %s 夹具", dir, ext)
	}
	return names
}

// golden 与 testdata/<name> 比较；带 -update 时改为写入。换行统一为 \n（Windows 检出可能带 CRLF）。
func golden(t *testing.T, name string, got []byte) {
	t.Helper()
	path := filepath.Join("testdata", filepath.FromSlash(name))
	got = lf(got)
	if *update {
		if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
			t.Fatalf("建 golden 目录: %v", err)
		}
		if err := os.WriteFile(path, got, 0o644); err != nil {
			t.Fatalf("写 golden: %v", err)
		}
		return
	}
	want, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("读 golden %s: %v（新增夹具先用 -update 生成）", path, err)
	}
	if !bytes.Equal(lf(want), got) {
		t.Errorf("%s 不一致；确认新输出正确后用 -update 重写。\n----- got -----\n%s\n----- want -----\n%s", path, got, want)
	}
}

func lf(b []byte) []byte { return bytes.ReplaceAll(b, []byte("\r\n"), []byte("\n")) }

// pretty 缩进 JSON，同时校验输出是合法 JSON。
func pretty(t *testing.T, b []byte) string {
	t.Helper()
	var out bytes.Buffer
	if err := json.Indent(&out, b, "", "  "); err != nil {
		t.Fatalf("输出不是合法 JSON: %v\n%s", err, b)
	}
	return out.String()
}

func compact(t *testing.T, v any) string {
	t.Helper()
	b, err := json.Marshal(v)
	if err != nil {
		t.Fatal(err)
	}
	return string(b)
}

func readEvents(t *testing.T, data []byte) []Event {
	t.Helper()
	r := NewSSEReader(bytes.NewReader(data))
	var evs []Event
	for {
		ev, err := r.Next()
		if errors.Is(err, io.EOF) {
			return evs
		}
		if err != nil {
			t.Fatalf("读取 SSE: %v", err)
		}
		evs = append(evs, ev)
	}
}
