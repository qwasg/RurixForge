package gateway

import (
	"encoding/json"
	"net/http"
	"strconv"
	"strings"

	"forge-cloud/internal/core"
	"forge-cloud/internal/gateway/convert"
)

// writeGatewayError 按端点写错误：/v1/messages 用 Anthropic 形状，其余用 OpenAI 形状（code 为小写错误码）。
func writeGatewayError(w http.ResponseWriter, endpoint string, e *core.Error) {
	if e.RetryAfter > 0 {
		w.Header().Set("Retry-After", strconv.Itoa(e.RetryAfter))
	}
	var body any
	if endpoint == epMessages {
		body = map[string]any{"type": "error", "error": map[string]any{"type": anthropicType(e.Status), "message": e.Message}}
	} else {
		body = map[string]any{"error": map[string]any{
			"message": e.Message, "type": openAIType(e.Status), "code": strings.ToLower(e.Code),
		}}
	}
	b, _ := json.Marshal(body)
	w.Header().Set("Content-Type", "application/json; charset=utf-8")
	w.Header().Set("Content-Length", strconv.Itoa(len(b)))
	w.WriteHeader(e.Status)
	_, _ = w.Write(b)
}

func openAIType(status int) string {
	switch {
	case status == http.StatusUnauthorized:
		return "authentication_error"
	case status == http.StatusPaymentRequired:
		return "insufficient_quota"
	case status == http.StatusForbidden:
		return "permission_error"
	case status == http.StatusTooManyRequests:
		return "rate_limit_error"
	case status == http.StatusBadGateway:
		return "upstream_error"
	case status >= 500:
		return "server_error"
	}
	return "invalid_request_error"
}

func anthropicType(status int) string {
	switch status {
	case http.StatusUnauthorized:
		return "authentication_error"
	case http.StatusPaymentRequired:
		return "billing_error"
	case http.StatusForbidden:
		return "permission_error"
	case http.StatusNotFound:
		return "not_found_error"
	case http.StatusRequestEntityTooLarge:
		return "request_too_large"
	case http.StatusTooManyRequests:
		return "rate_limit_error"
	case http.StatusServiceUnavailable, 529:
		return "overloaded_error"
	}
	if status >= 500 {
		return "api_error"
	}
	return "invalid_request_error"
}

// upstreamError 解析上游错误体：优先用转换器的解析，缺省回落到本地的宽松解析。
func upstreamError(body []byte) *convert.UpstreamError {
	if ue := convert.ErrorFromBody(body); ue != nil {
		return ue
	}
	return parseUpstreamError(body)
}

// parseUpstreamError 识别 OpenAI `{"error":{…}}`、Anthropic `{"type":"error","error":{…}}`、
// Codex `{"detail":"…"}` 与 `{"error":"…"}`。
func parseUpstreamError(body []byte) *convert.UpstreamError {
	var v struct {
		Error   json.RawMessage `json:"error"`
		Detail  json.RawMessage `json:"detail"`
		Message string          `json:"message"`
	}
	if json.Unmarshal(body, &v) != nil {
		return nil
	}
	var s string
	if len(v.Error) > 0 {
		if json.Unmarshal(v.Error, &s) == nil && s != "" {
			return &convert.UpstreamError{Message: s}
		}
		var o struct {
			Type            string          `json:"type"`
			Code            json.RawMessage `json:"code"`
			Message         string          `json:"message"`
			ResetsInSeconds float64         `json:"resets_in_seconds"`
		}
		if json.Unmarshal(v.Error, &o) == nil {
			ue := &convert.UpstreamError{Type: o.Type, Message: o.Message, ResetsInSeconds: int(o.ResetsInSeconds)}
			if json.Unmarshal(o.Code, &s) == nil {
				ue.Code = s
			}
			if ue.Type != "" || ue.Message != "" || ue.Code != "" {
				return ue
			}
		}
	}
	if len(v.Detail) > 0 && json.Unmarshal(v.Detail, &s) == nil && s != "" {
		return &convert.UpstreamError{Message: s}
	}
	if v.Message != "" {
		return &convert.UpstreamError{Message: v.Message}
	}
	return nil
}

// upstreamMessage 返回可读的上游错误信息。
func upstreamMessage(status int, body []byte) string {
	if ue := upstreamError(body); ue != nil {
		if m := firstNonEmpty(ue.Message, ue.Type, ue.Code); m != "" {
			return m
		}
	}
	if t := strings.TrimSpace(string(body)); t != "" && len(t) <= 300 && !strings.HasPrefix(t, "<") {
		return t
	}
	if status > 0 {
		return "HTTP " + strconv.Itoa(status) + " " + http.StatusText(status)
	}
	return "上游无响应"
}

// isRateLimitError：流内/响应体里的限流类错误（换号而不是直接报错）。
func isRateLimitError(ue *convert.UpstreamError) bool {
	if ue == nil {
		return false
	}
	for _, v := range []string{ue.Type, ue.Code} {
		switch v {
		case "usage_limit_reached", "rate_limit_exceeded", "rate_limit_error", "insufficient_quota":
			return true
		}
	}
	return ue.ResetsInSeconds > 0
}

// streamErrorFrame 生成写给流式客户端的错误帧（已写出字节后上游出错时使用）。
func streamErrorFrame(proto, message string) []byte {
	switch proto {
	case "responses":
		b, _ := json.Marshal(map[string]any{"type": "error", "code": "upstream_error", "message": message})
		return convert.FormatSSE("error", b)
	case "messages":
		b, _ := json.Marshal(map[string]any{"type": "error", "error": map[string]any{"type": "api_error", "message": message}})
		return convert.FormatSSE("error", b)
	}
	b, _ := json.Marshal(map[string]any{"error": map[string]any{"message": message, "type": "upstream_error", "code": "upstream_error"}})
	return convert.FormatSSE("", b)
}

func firstNonEmpty(vs ...string) string {
	for _, v := range vs {
		if strings.TrimSpace(v) != "" {
			return strings.TrimSpace(v)
		}
	}
	return ""
}
