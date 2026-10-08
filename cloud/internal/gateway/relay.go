package gateway

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"strconv"
	"strings"
	"time"

	"forge-cloud/internal/accounts"
	"forge-cloud/internal/core"
	"forge-cloud/internal/gateway/convert"
)

// relay 把 2xx 上游响应交给客户端。返回非 nil 仅当尚未向客户端写出任何字节且可换号重试。
func (c *call) relay(a *accounts.Account, p *plan, resp *http.Response) *upstreamFailure {
	if p.sse && !isSSE(resp.Header.Get("Content-Type")) {
		if a.IsCodex() {
			body, _ := io.ReadAll(io.LimitReader(resp.Body, maxErrorBody))
			c.fail(core.E(http.StatusBadGateway, core.CodeUpstreamError, "上游返回了非流式响应："+upstreamMessage(resp.StatusCode, body)))
			return nil
		}
		// 部分兼容上游忽略 stream=true 直接回 JSON：原样透传。
		if p.mode == relayStream {
			return c.relayJSON(a, p, resp)
		}
	}
	switch p.mode {
	case relayStream:
		return c.relayStream(a, p, resp)
	case relayJSON:
		return c.relayJSON(a, p, resp)
	case relayCollect:
		return c.relayCollect(a, resp)
	case relayChatStream, relayChatCollect:
		return c.relayToChat(a, p, resp)
	case relayChatFromMessages:
		return c.relayChatFromMessages(a, resp)
	}
	c.fail(core.E(http.StatusInternalServerError, "INTERNAL", "未知的回传方式"))
	return nil
}

func isSSE(ct string) bool { return strings.HasPrefix(strings.ToLower(strings.TrimSpace(ct)), "text/event-stream") }

// ---------- 写客户端 ----------

func (c *call) startStream() {
	h := c.w.Header()
	h.Set("Content-Type", "text/event-stream; charset=utf-8")
	h.Set("Cache-Control", "no-cache")
	h.Set("X-Accel-Buffering", "no")
	c.w.WriteHeader(http.StatusOK)
	c.started = true
	c.status = http.StatusOK
}

// writeFrame 写出一段流并立即 flush；客户端已断开返回 false。
func (c *call) writeFrame(b []byte) bool {
	if len(b) == 0 {
		return true
	}
	if !c.started {
		c.startStream()
	}
	if _, err := c.w.Write(b); err != nil {
		return false
	}
	if err := c.rc.Flush(); err != nil && !errors.Is(err, http.ErrNotSupported) {
		return false
	}
	if c.firstToken == 0 {
		c.firstToken = time.Since(c.start)
	}
	return true
}

// writeWhole 一次性写出非流式响应（带 Content-Length，客户端读完即可结束）。
func (c *call) writeWhole(status int, contentType string, body []byte) {
	h := c.w.Header()
	h.Set("Content-Type", contentType)
	h.Set("Content-Length", strconv.Itoa(len(body)))
	c.w.WriteHeader(status)
	c.started = true
	c.status = status
	if _, err := c.w.Write(body); err != nil {
		c.clientClosed()
	}
}

// readFailure 处理读上游流/体时的错误：客户端断开 → client_closed；尚未写出 → 冷却换号；已写出 → 流内报错。
func (c *call) readFailure(a *accounts.Account, proto string, err error) *upstreamFailure {
	if c.ctx.Err() != nil {
		c.clientClosed()
		return nil
	}
	if !c.started {
		_ = c.s.d.Accounts.Cooldown(context.WithoutCancel(c.ctx), a.ID, networkCooldown, "读取上游响应失败："+err.Error())
		return &upstreamFailure{message: "读取上游响应失败"}
	}
	c.midStreamError(proto, "上游连接中断")
	return nil
}

func (c *call) midStreamError(proto, message string) {
	c.errCode = strings.ToLower(core.CodeUpstreamError)
	if !c.writeFrame(streamErrorFrame(proto, message)) {
		c.clientClosed()
	}
}

// inStreamError 处理上游在流内报告的错误（尚未写出字节时）：限流类冷却换号，其余 502。
func (c *call) inStreamError(a *accounts.Account, ue *convert.UpstreamError) *upstreamFailure {
	msg := firstNonEmpty(ue.Message, ue.Type, ue.Code, "上游返回错误")
	if isRateLimitError(ue) {
		d := 60 * time.Second
		if ue.ResetsInSeconds > 0 {
			d = clampCooldown(time.Duration(ue.ResetsInSeconds) * time.Second)
		}
		_ = c.s.d.Accounts.Cooldown(context.WithoutCancel(c.ctx), a.ID, d, "上游限流："+msg)
		return &upstreamFailure{status: http.StatusTooManyRequests, message: msg, retryAfter: d}
	}
	c.fail(core.E(http.StatusBadGateway, core.CodeUpstreamError, msg))
	return nil
}

// ---------- 透传 ----------

func newTap(proto string) convert.UsageTap {
	switch proto {
	case epChat:
		return convert.NewChatUsageTap()
	case epResponses:
		return convert.NewResponsesUsageTap()
	case epMessages:
		return convert.NewMessagesUsageTap()
	}
	return nopTap{}
}

type nopTap struct{}

func (nopTap) Feed(convert.Event)  {}
func (nopTap) Usage() core.Usage { return core.Usage{} }

func usageFromBody(proto string, body []byte) core.Usage {
	switch proto {
	case epChat:
		return convert.ChatUsageFromBody(body)
	case epResponses:
		return convert.ResponsesUsageFromBody(body)
	case epMessages:
		return convert.MessagesUsageFromBody(body)
	case epEmbeddings:
		return convert.EmbeddingsUsageFromBody(body)
	}
	return core.Usage{}
}

// isUsageOnlyChunk：网关注入 include_usage 后上游追加的「choices 为空 + usage」末块。
func isUsageOnlyChunk(data []byte) bool {
	if !bytes.Contains(data, []byte(`"usage"`)) {
		return false
	}
	var v struct {
		Choices []json.RawMessage `json:"choices"`
		Usage   json.RawMessage   `json:"usage"`
	}
	if json.Unmarshal(data, &v) != nil {
		return false
	}
	return len(v.Choices) == 0 && len(v.Usage) > 0 && string(v.Usage) != "null"
}

func (c *call) relayStream(a *accounts.Account, p *plan, resp *http.Response) *upstreamFailure {
	tap := newTap(p.proto)
	hideUsage := p.proto == epChat && !c.includeUsage
	rd := convert.NewSSEReader(resp.Body)
	for {
		ev, err := rd.Next()
		if errors.Is(err, io.EOF) {
			break
		}
		if err != nil {
			c.usage = tap.Usage()
			return c.readFailure(a, p.proto, err)
		}
		tap.Feed(ev)
		if hideUsage && isUsageOnlyChunk(ev.Data) {
			continue
		}
		if !c.writeFrame(convert.FormatSSE(ev.Event, ev.Data)) {
			c.usage = tap.Usage()
			c.clientClosed()
			return nil
		}
	}
	c.usage = tap.Usage()
	if !c.started {
		return c.readFailure(a, p.proto, errors.New("上游流为空"))
	}
	return nil
}

func (c *call) relayJSON(a *accounts.Account, p *plan, resp *http.Response) *upstreamFailure {
	body, err := io.ReadAll(io.LimitReader(resp.Body, maxUpstreamBody))
	if err != nil {
		return c.readFailure(a, p.proto, err)
	}
	c.usage = usageFromBody(p.proto, body)
	ct := resp.Header.Get("Content-Type")
	if ct == "" {
		ct = "application/json"
	}
	c.writeWhole(resp.StatusCode, ct, body)
	return nil
}

// ---------- Codex：Responses SSE → 非流式 response ----------

func (c *call) relayCollect(a *accounts.Account, resp *http.Response) *upstreamFailure {
	col := convert.NewResponsesCollector()
	rd := convert.NewSSEReader(resp.Body)
	for {
		ev, err := rd.Next()
		if errors.Is(err, io.EOF) {
			break
		}
		if err != nil {
			c.usage = col.Usage()
			return c.readFailure(a, epResponses, err)
		}
		col.Feed(ev)
	}
	c.usage = col.Usage()
	if ue := col.UpstreamError(); ue != nil {
		return c.inStreamError(a, ue)
	}
	out, err := col.Result()
	if err != nil {
		c.fail(core.E(http.StatusBadGateway, core.CodeUpstreamError, "上游响应不完整："+err.Error()))
		return nil
	}
	c.writeWhole(http.StatusOK, "application/json", out)
	return nil
}

// ---------- 转换：上游 SSE → chat.completions ----------

func (c *call) newToChat(proto string, includeUsage bool) convert.ToChat {
	if proto == epMessages {
		return convert.NewMessagesToChat(c.model.ID, includeUsage)
	}
	return convert.NewResponsesToChat(c.model.ID, includeUsage)
}

func (c *call) relayToChat(a *accounts.Account, p *plan, resp *http.Response) *upstreamFailure {
	streaming := p.mode == relayChatStream
	tc := c.newToChat(p.proto, streaming && c.includeUsage)
	rd := convert.NewSSEReader(resp.Body)
	for {
		ev, err := rd.Next()
		if errors.Is(err, io.EOF) {
			break
		}
		if err != nil {
			c.usage = tc.Usage()
			return c.readFailure(a, epChat, err)
		}
		frames, err := tc.Feed(ev)
		if err != nil {
			c.usage = tc.Usage()
			if !c.started {
				c.fail(core.E(http.StatusBadGateway, core.CodeUpstreamError, "协议转换失败："+err.Error()))
			} else {
				c.midStreamError(epChat, "协议转换失败")
			}
			return nil
		}
		if ue := tc.UpstreamError(); ue != nil && !c.started {
			c.usage = tc.Usage()
			return c.inStreamError(a, ue)
		}
		if streaming && !c.writeFrame(frames) {
			c.usage = tc.Usage()
			c.clientClosed()
			return nil
		}
	}
	c.usage = tc.Usage()
	if ue := tc.UpstreamError(); ue != nil {
		if !c.started {
			return c.inStreamError(a, ue)
		}
		c.midStreamError(epChat, firstNonEmpty(ue.Message, ue.Type, "上游返回错误"))
		if !c.writeFrame(convert.DoneFrame) {
			c.clientClosed()
		}
		return nil
	}
	if !tc.Done() && !c.started {
		return c.readFailure(a, epChat, errors.New("上游流在完成前结束"))
	}
	tail := tc.Finish()
	c.usage = tc.Usage()
	if !tc.Done() {
		c.errCode = strings.ToLower(core.CodeUpstreamError)
	}
	if streaming {
		if !c.writeFrame(tail) {
			c.clientClosed()
		}
		return nil
	}
	out := tc.Completion()
	if len(out) == 0 {
		c.fail(core.E(http.StatusBadGateway, core.CodeUpstreamError, "上游响应不完整"))
		return nil
	}
	c.writeWhole(http.StatusOK, "application/json", out)
	return nil
}

// ---------- 转换：Anthropic 非流式 → chat.completion ----------

func (c *call) relayChatFromMessages(a *accounts.Account, resp *http.Response) *upstreamFailure {
	body, err := io.ReadAll(io.LimitReader(resp.Body, maxUpstreamBody))
	if err != nil {
		return c.readFailure(a, epChat, err)
	}
	out, usage, err := convert.ChatCompletionFromMessages(body, c.model.ID)
	if err != nil {
		c.fail(core.E(http.StatusBadGateway, core.CodeUpstreamError, "协议转换失败："+err.Error()))
		return nil
	}
	c.usage = usage
	c.writeWhole(http.StatusOK, "application/json", out)
	return nil
}
