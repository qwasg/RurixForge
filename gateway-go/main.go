package main

import (
	"context"
	"crypto/hmac"
	"crypto/sha256"
	"encoding/base64"
	"encoding/json"
	"errors"
	"io"
	"log"
	"net/http"
	"net/http/httputil"
	"net/url"
	"os"
	"strings"
	"time"
)

const (
	defaultPort   = "8102"
	defaultAgentd = "http://127.0.0.1:8103"
	defaultSecret = "forge-dev-secret"
	// 聚合健康检查的上游超时
	healthTimeout = 5 * time.Second
)

var (
	errTokenMalformed = errors.New("token 格式非法")
	errBadSignature   = errors.New("签名验证失败")
	errTokenExpired   = errors.New("token 已过期")
)

type server struct {
	upstream *url.URL
	secret   []byte
	proxy    *httputil.ReverseProxy
	client   *http.Client
}

func newServer(upstream *url.URL, secret []byte) *server {
	s := &server{
		upstream: upstream,
		secret:   secret,
		client:   &http.Client{Timeout: healthTimeout},
	}
	proxy := httputil.NewSingleHostReverseProxy(upstream)
	// 上游不可达时返回统一 502 JSON
	proxy.ErrorHandler = func(w http.ResponseWriter, _ *http.Request, err error) {
		writeJSONError(w, http.StatusBadGateway, "UPSTREAM_UNREACHABLE", "上游 forge-agentd 不可达: "+err.Error())
	}
	s.proxy = proxy
	return s
}

func (s *server) routes() http.Handler {
	mux := http.NewServeMux()
	mux.HandleFunc("/health", s.handleHealth)
	mux.Handle("/api/forge/", s.jwtMiddleware(s.proxy))
	return corsMiddleware(mux)
}

func main() {
	port := os.Getenv("FORGE_GATEWAY_PORT")
	if port == "" {
		port = defaultPort
	}
	agentd := os.Getenv("FORGE_AGENTD_ADDR")
	if agentd == "" {
		agentd = defaultAgentd
	}
	secret := os.Getenv("FORGE_GATEWAY_JWT_SECRET")
	if secret == "" {
		secret = defaultSecret
		log.Printf("[warn] FORGE_GATEWAY_JWT_SECRET 未设置，使用开发缺省密钥")
	}
	upstream, err := url.Parse(agentd)
	if err != nil {
		log.Fatalf("上游地址解析失败: %v", err)
	}
	s := newServer(upstream, []byte(secret))
	log.Printf("forge-gateway listening at http://127.0.0.1:%s", port)
	if err := http.ListenAndServe(":"+port, s.routes()); err != nil {
		log.Fatalf("服务退出: %v", err)
	}
}

// verifyJWT 手写 HS256 验签与 exp 校验（仅用标准库）
func verifyJWT(token string, secret []byte, now time.Time) error {
	parts := strings.Split(token, ".")
	if len(parts) != 3 {
		return errTokenMalformed
	}
	header, err := base64.RawURLEncoding.DecodeString(parts[0])
	if err != nil {
		return errTokenMalformed
	}
	var hdr struct {
		Alg string `json:"alg"`
	}
	if err := json.Unmarshal(header, &hdr); err != nil || hdr.Alg != "HS256" {
		return errTokenMalformed
	}
	payload, err := base64.RawURLEncoding.DecodeString(parts[1])
	if err != nil {
		return errTokenMalformed
	}
	sig, err := base64.RawURLEncoding.DecodeString(parts[2])
	if err != nil {
		return errTokenMalformed
	}
	mac := hmac.New(sha256.New, secret)
	mac.Write([]byte(parts[0] + "." + parts[1]))
	if !hmac.Equal(sig, mac.Sum(nil)) {
		return errBadSignature
	}
	var claims struct {
		Exp *int64 `json:"exp"`
	}
	if err := json.Unmarshal(payload, &claims); err != nil || claims.Exp == nil {
		return errTokenMalformed
	}
	if now.Unix() >= *claims.Exp {
		return errTokenExpired
	}
	return nil
}

// jwtMiddleware 校验 Authorization: Bearer <HS256 JWT>
func (s *server) jwtMiddleware(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		auth := r.Header.Get("Authorization")
		if !strings.HasPrefix(auth, "Bearer ") {
			writeJSONError(w, http.StatusUnauthorized, "UNAUTHORIZED", "缺少 Bearer token")
			return
		}
		if err := verifyJWT(strings.TrimPrefix(auth, "Bearer "), s.secret, time.Now()); err != nil {
			writeJSONError(w, http.StatusUnauthorized, "UNAUTHORIZED", err.Error())
			return
		}
		next.ServeHTTP(w, r)
	})
}

// corsMiddleware 全开放 CORS，OPTIONS 预检直接 204
func corsMiddleware(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Access-Control-Allow-Origin", "*")
		if r.Method == http.MethodOptions {
			w.Header().Set("Access-Control-Allow-Headers", "Authorization, Content-Type")
			w.Header().Set("Access-Control-Allow-Methods", "GET, POST, PUT, PATCH, DELETE, OPTIONS")
			w.WriteHeader(http.StatusNoContent)
			return
		}
		next.ServeHTTP(w, r)
	})
}

// handleHealth 聚合上游 /health，网关自身恒 200
func (s *server) handleHealth(w http.ResponseWriter, r *http.Request) {
	result := map[string]any{"gateway": "ok", "agentd": "unreachable"}
	ctx, cancel := context.WithTimeout(r.Context(), healthTimeout)
	defer cancel()
	upURL := strings.TrimRight(s.upstream.String(), "/") + "/health"
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, upURL, nil)
	if err == nil {
		resp, err := s.client.Do(req)
		if err == nil {
			defer resp.Body.Close()
			b, _ := io.ReadAll(io.LimitReader(resp.Body, 1<<20))
			if resp.StatusCode >= 200 && resp.StatusCode < 300 {
				result["agentd"] = "ok"
			}
			// 上游 body 为合法 JSON 时透传
			if json.Valid(b) {
				result["agentd_body"] = json.RawMessage(b)
			}
		}
	}
	w.Header().Set("Content-Type", "application/json")
	_ = json.NewEncoder(w).Encode(result)
}

func writeJSONError(w http.ResponseWriter, status int, code, message string) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(map[string]any{
		"error": map[string]string{"code": code, "message": message},
	})
}
