// Package httpx 是各模块共用的 HTTP 工具：JSON 读写、错误响应、中间件、分页与客户端 IP。
package httpx

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"log/slog"
	"net"
	"net/http"
	"runtime/debug"
	"strconv"
	"strings"
	"time"

	"github.com/go-chi/chi/v5"

	"forge-cloud/internal/core"
)

// MaxJSONBody 是普通 JSON 请求体上限（网关自行控制更大的上限）。
const MaxJSONBody = 4 << 20

type ctxKey int

const requestIDKey ctxKey = 1

// RequestIDFrom 取当前请求 ID。
func RequestIDFrom(ctx context.Context) string {
	if v, ok := ctx.Value(requestIDKey).(string); ok {
		return v
	}
	return ""
}

// RequestID 生成请求 ID，写入 context 与 X-Request-Id 响应头。
func RequestID(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		id := "req_" + core.RandomString(20)
		w.Header().Set("X-Request-Id", id)
		next.ServeHTTP(w, r.WithContext(context.WithValue(r.Context(), requestIDKey, id)))
	})
}

type statusRecorder struct {
	http.ResponseWriter
	status int
	bytes  int64
}

func (s *statusRecorder) WriteHeader(code int) {
	if s.status == 0 {
		s.status = code
	}
	s.ResponseWriter.WriteHeader(code)
}

func (s *statusRecorder) Write(b []byte) (int, error) {
	if s.status == 0 {
		s.status = http.StatusOK
	}
	n, err := s.ResponseWriter.Write(b)
	s.bytes += int64(n)
	return n, err
}

// Flush 透传给底层 writer（SSE 必需）。
func (s *statusRecorder) Flush() {
	if f, ok := s.ResponseWriter.(http.Flusher); ok {
		f.Flush()
	}
}

// Unwrap 让 http.ResponseController 能找到底层 writer。
func (s *statusRecorder) Unwrap() http.ResponseWriter { return s.ResponseWriter }

// AccessLog 记录访问日志（不落请求/响应正文）。
func AccessLog(log *slog.Logger) func(http.Handler) http.Handler {
	return func(next http.Handler) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			start := time.Now()
			rec := &statusRecorder{ResponseWriter: w}
			next.ServeHTTP(rec, r)
			if r.URL.Path == "/healthz" {
				return
			}
			log.Info("http",
				"method", r.Method,
				"path", r.URL.Path,
				"status", rec.status,
				"bytes", rec.bytes,
				"ms", time.Since(start).Milliseconds(),
				"rid", RequestIDFrom(r.Context()),
			)
		})
	}
}

// Recoverer 把 panic 转成 500。
func Recoverer(log *slog.Logger) func(http.Handler) http.Handler {
	return func(next http.Handler) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			defer func() {
				if v := recover(); v != nil {
					if v == http.ErrAbortHandler {
						panic(v)
					}
					log.Error("panic", "err", v, "path", r.URL.Path, "stack", string(debug.Stack()))
					WriteError(w, core.E(http.StatusInternalServerError, "INTERNAL", "服务器内部错误"))
				}
			}()
			next.ServeHTTP(w, r)
		})
	}
}

// WriteJSON 写 JSON 响应。
func WriteJSON(w http.ResponseWriter, status int, v any) {
	w.Header().Set("Content-Type", "application/json; charset=utf-8")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(v)
}

// OK 写 {"ok":true}。
func OK(w http.ResponseWriter) { WriteJSON(w, http.StatusOK, map[string]any{"ok": true}) }

// WriteError 写 `{"error":{"code","message"}}`；非业务错误一律 500 INTERNAL（不外泄细节）。
func WriteError(w http.ResponseWriter, err error) {
	e := core.AsError(err)
	if e == nil {
		e = core.E(http.StatusInternalServerError, "INTERNAL", "服务器内部错误")
	}
	body := map[string]any{"error": map[string]any{"code": e.Code, "message": e.Message}}
	for k, v := range e.Extra {
		body[k] = v
	}
	if e.RetryAfter > 0 {
		w.Header().Set("Retry-After", strconv.Itoa(e.RetryAfter))
	}
	WriteJSON(w, e.Status, body)
}

// DecodeJSON 读 JSON 请求体（上限 maxBytes，<=0 用 MaxJSONBody）。
func DecodeJSON(r *http.Request, v any, maxBytes int64) error {
	if maxBytes <= 0 {
		maxBytes = MaxJSONBody
	}
	body := http.MaxBytesReader(nil, r.Body, maxBytes)
	defer body.Close()
	dec := json.NewDecoder(body)
	if err := dec.Decode(v); err != nil {
		var mbe *http.MaxBytesError
		if errors.As(err, &mbe) {
			return core.E(http.StatusRequestEntityTooLarge, "BODY_TOO_LARGE", "请求体过大")
		}
		if errors.Is(err, io.EOF) {
			return core.BadRequest("INVALID_JSON", "请求体为空")
		}
		return core.BadRequest("INVALID_JSON", "请求体不是合法 JSON")
	}
	return nil
}

// Pagination 解析 ?limit=&offset=（limit 默认 50，上限 200）。
func Pagination(r *http.Request) (limit, offset int) {
	limit = 50
	if v, err := strconv.Atoi(r.URL.Query().Get("limit")); err == nil && v > 0 {
		limit = v
	}
	if limit > 200 {
		limit = 200
	}
	if v, err := strconv.Atoi(r.URL.Query().Get("offset")); err == nil && v > 0 {
		offset = v
	}
	return limit, offset
}

// PathInt64 解析 chi 路径参数为 int64，失败返回 400 INVALID_ID。
func PathInt64(r *http.Request, name string) (int64, error) {
	v, err := strconv.ParseInt(chi.URLParam(r, name), 10, 64)
	if err != nil || v <= 0 {
		return 0, core.BadRequest("INVALID_ID", "ID 非法")
	}
	return v, nil
}

// QueryInt64 解析可选查询参数（缺省返回 0）。
func QueryInt64(r *http.Request, name string) int64 {
	v, _ := strconv.ParseInt(r.URL.Query().Get(name), 10, 64)
	return v
}

// QueryTime 解析可选 RFC3339 查询参数（缺省/非法返回零值）。
func QueryTime(r *http.Request, name string) time.Time {
	t, _ := time.Parse(time.RFC3339, r.URL.Query().Get(name))
	return t
}

// ClientIP 取客户端 IP；trustProxy 时取 X-Forwarded-For 首跳。
func ClientIP(r *http.Request, trustProxy bool) string {
	if trustProxy {
		if xff := r.Header.Get("X-Forwarded-For"); xff != "" {
			if i := strings.IndexByte(xff, ','); i >= 0 {
				xff = xff[:i]
			}
			if ip := strings.TrimSpace(xff); ip != "" {
				return ip
			}
		}
		if ip := strings.TrimSpace(r.Header.Get("X-Real-Ip")); ip != "" {
			return ip
		}
	}
	host, _, err := net.SplitHostPort(r.RemoteAddr)
	if err != nil {
		return r.RemoteAddr
	}
	return host
}

// NotImplemented 是脚手架占位 handler。
func NotImplemented(w http.ResponseWriter, _ *http.Request) {
	WriteError(w, core.E(http.StatusNotImplemented, "NOT_IMPLEMENTED", "尚未实现"))
}
