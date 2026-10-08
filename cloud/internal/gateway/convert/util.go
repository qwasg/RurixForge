package convert

import (
	"bytes"
	"encoding/json"
	"errors"
	"math"
	"strconv"
	"strings"
	"time"

	"forge-cloud/internal/core"
)

// nowUnix / newID 生成 chat 块的 created 与兜底 id；测试替换为确定值。
var (
	nowUnix = func() int64 { return time.Now().Unix() }
	newID   = func() string { return core.RandomString(24) }
)

// marshalJSON 序列化且不转义 <>&（透传给上游/客户端的文本保持原样）。
func marshalJSON(v any) ([]byte, error) {
	var b bytes.Buffer
	enc := json.NewEncoder(&b)
	enc.SetEscapeHTML(false)
	if err := enc.Encode(v); err != nil {
		return nil, err
	}
	return bytes.TrimRight(b.Bytes(), "\n"), nil
}

// jsonBytes 用于本包自有的输出结构（无浮点/通道，RawMessage 均来自已校验的 JSON，序列化不会失败）。
func jsonBytes(v any) []byte {
	b, _ := marshalJSON(v)
	return b
}

// isNull：缺省或 JSON null。
func isNull(raw []byte) bool {
	t := bytes.TrimSpace(raw)
	return len(t) == 0 || string(t) == "null"
}

// rawString 把 JSON 值转成字符串：字符串取值，数字/布尔取字面量，对象/数组取紧凑原文，null 为空。
func rawString(raw []byte) string {
	t := bytes.TrimSpace(raw)
	if isNull(t) {
		return ""
	}
	if t[0] == '"' {
		var s string
		if json.Unmarshal(t, &s) == nil {
			return s
		}
		return ""
	}
	return string(compactJSON(t))
}

// compactJSON 去掉空白（非法 JSON 原样返回）。
func compactJSON(raw []byte) []byte {
	var b bytes.Buffer
	if json.Compact(&b, raw) != nil {
		return raw
	}
	return b.Bytes()
}

// rawNumber 宽松解析数字（JSON 数字或数字字符串）。
func rawNumber(raw []byte) (float64, bool) {
	t := bytes.TrimSpace(raw)
	if isNull(t) {
		return 0, false
	}
	if t[0] == '"' {
		var s string
		if json.Unmarshal(t, &s) != nil {
			return 0, false
		}
		f, err := strconv.ParseFloat(strings.TrimSpace(s), 64)
		if err != nil || math.IsNaN(f) || math.IsInf(f, 0) {
			return 0, false
		}
		return f, true
	}
	var f float64
	if json.Unmarshal(t, &f) != nil {
		return 0, false
	}
	return f, true
}

// tokens 宽松解析 token 数（整数、浮点或数字字符串；其它视为 0），避免个别上游的怪格式丢掉整段用量。
type tokens int64

func (t *tokens) UnmarshalJSON(b []byte) error {
	f, _ := rawNumber(b)
	*t = tokens(math.Round(f))
	return nil
}

func nonNeg(v int64) int64 {
	if v < 0 {
		return 0
	}
	return v
}

func firstNonEmpty(vs ...string) string {
	for _, v := range vs {
		if v != "" {
			return v
		}
	}
	return ""
}

// decodeObject 解码 JSON 对象：非对象或语法错误报错；字段类型不符只丢该字段（encoding/json 会继续解码其余字段）。
func decodeObject(data []byte, v any) error {
	t := bytes.TrimSpace(data)
	if len(t) == 0 || t[0] != '{' {
		return errors.New("不是 JSON 对象")
	}
	err := json.Unmarshal(t, v)
	var te *json.UnmarshalTypeError
	if err != nil && !errors.As(err, &te) {
		return err
	}
	return nil
}

// describeError 生成错误说明（用于 error 文本，不含敏感信息）。
func describeError(e *UpstreamError) string {
	if e == nil {
		return "未知错误"
	}
	var parts []string
	if e.Type != "" {
		parts = append(parts, e.Type)
	}
	if e.Code != "" && e.Code != e.Type {
		parts = append(parts, e.Code)
	}
	msg := firstNonEmpty(e.Message, "上游返回错误")
	if len(parts) == 0 {
		return msg
	}
	return strings.Join(parts, "/") + ": " + msg
}

// eventHead 读出事件 data 的 type（data 缺 type 时回落到 SSE event 行）。
// ok=false 表示 data 为空或 [DONE]，应忽略；err 表示 data 不是合法 JSON。
func eventHead(ev Event) (typ string, data []byte, ok bool, err error) {
	data = bytes.TrimSpace(ev.Data)
	if len(data) == 0 || string(data) == "[DONE]" {
		return "", nil, false, nil
	}
	var head struct {
		Type json.RawMessage `json:"type"`
	}
	if err := json.Unmarshal(data, &head); err != nil {
		return "", nil, false, err
	}
	typ = rawString(head.Type)
	if typ == "" {
		typ = ev.Event
	}
	return typ, data, true, nil
}
