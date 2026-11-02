package gateway

import (
	"errors"
	"io"
	"time"

	"forge-cloud/internal/gateway/convert"
)

const streamKeepaliveEvery = 20 * time.Second

var errClientStreamClosed = errors.New("客户端流已断开")

// streamEvents 在请求协程中写保活注释，读协程只读上游，避免并发写 ResponseWriter。
// 首帧前保留 HTTP 错误和换号机会；非流式请求不写 SSE 注释。
func (c *call) streamEvents(body io.ReadCloser, streaming bool, interval time.Duration) (func() (convert.Event, error), func()) {
	rd := convert.NewSSEReader(body)
	if !streaming {
		return rd.Next, func() {}
	}
	type result struct {
		event convert.Event
		err   error
	}
	results := make(chan result)
	done := make(chan struct{})
	finished := make(chan struct{})
	go func() {
		defer close(finished)
		for {
			ev, err := rd.Next()
			select {
			case results <- result{ev, err}:
			case <-done:
				return
			}
			if err != nil {
				return
			}
		}
	}()
	ticker := time.NewTicker(interval)
	next := func() (convert.Event, error) {
		for {
			select {
			case res := <-results:
				return res.event, res.err
			case <-ticker.C:
				if c.started && !c.writeFrame([]byte(": keep-alive\n\n")) {
					return convert.Event{}, errClientStreamClosed
				}
			case <-c.ctx.Done():
				return convert.Event{}, c.ctx.Err()
			}
		}
	}
	stop := func() {
		ticker.Stop()
		close(done)
		// 中止阻塞的 Read，完成帧之后不等待上游主动断开连接。
		_ = body.Close()
		<-finished
	}
	return next, stop
}
