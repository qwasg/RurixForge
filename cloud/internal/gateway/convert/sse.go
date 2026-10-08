package convert

import (
	"bufio"
	"bytes"
	"io"
)

// Event 是一条 SSE 事件：Event 可为空；Data 为多行 data 以 "\n" 拼接后的原文。
type Event struct {
	Event string
	Data  []byte
}

// SSEReader 按 SSE 规范切分事件（忽略注释行与 id/retry 字段）。单行上限 16 MiB。
type SSEReader struct {
	sc *bufio.Scanner
}

func NewSSEReader(r io.Reader) *SSEReader {
	sc := bufio.NewScanner(r)
	sc.Buffer(make([]byte, 0, 64*1024), 16<<20)
	return &SSEReader{sc: sc}
}

// Next 返回下一条事件；上游结束返回 io.EOF（结束前未以空行收尾的残余事件也会返回）。
func (r *SSEReader) Next() (Event, error) {
	var ev Event
	var data [][]byte
	have := false
	for r.sc.Scan() {
		line := r.sc.Bytes()
		if len(line) > 0 && line[len(line)-1] == '\r' {
			line = line[:len(line)-1]
		}
		if len(line) == 0 {
			if have {
				ev.Data = bytes.Join(data, []byte("\n"))
				return ev, nil
			}
			continue
		}
		if line[0] == ':' {
			continue
		}
		field, value := line, []byte(nil)
		if i := bytes.IndexByte(line, ':'); i >= 0 {
			field, value = line[:i], line[i+1:]
			if len(value) > 0 && value[0] == ' ' {
				value = value[1:]
			}
		}
		switch string(field) {
		case "event":
			ev.Event = string(value)
			have = true
		case "data":
			data = append(data, append([]byte(nil), value...))
			have = true
		}
	}
	if err := r.sc.Err(); err != nil {
		return Event{}, err
	}
	if have {
		ev.Data = bytes.Join(data, []byte("\n"))
		return ev, nil
	}
	return Event{}, io.EOF
}

// FormatSSE 生成一帧 SSE（event 为空则省略 event 行；data 内换行拆成多行 data）。
func FormatSSE(event string, data []byte) []byte {
	var b bytes.Buffer
	if event != "" {
		b.WriteString("event: ")
		b.WriteString(event)
		b.WriteByte('\n')
	}
	for _, line := range bytes.Split(data, []byte("\n")) {
		b.WriteString("data: ")
		b.Write(line)
		b.WriteByte('\n')
	}
	b.WriteByte('\n')
	return b.Bytes()
}

// DoneFrame 是 chat.completions 流的结束帧。
var DoneFrame = []byte("data: [DONE]\n\n")
