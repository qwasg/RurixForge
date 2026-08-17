package main

import (
	"crypto/hmac"
	"crypto/sha256"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"net/url"
	"strings"
	"sync"
	"testing"
	"time"
)

const testSecret = "test-secret"

// fakeUpstream 记录收到的路径与 body，并返回 200 {"ok":true}
type fakeUpstream struct {
	srv      *httptest.Server
	mu       sync.Mutex
	lastPath string
	lastBody []byte
}

func newFakeUpstream(t *testing.T) *fakeUpstream {
	t.Helper()
	f := &fakeUpstream{}
	f.srv = httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		body, _ := io.ReadAll(r.Body)
		f.mu.Lock()
		f.lastPath = r.URL.Path
		f.lastBody = body
		f.mu.Unlock()
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write([]byte(`{"ok":true}`))
	}))
	t.Cleanup(f.srv.Close)
	return f
}

// signJWT 在测试内自签 HS256 token
func signJWT(t *testing.T, secret string, exp int64) string {
	t.Helper()
	header := base64.RawURLEncoding.EncodeToString([]byte(`{"alg":"HS256","typ":"JWT"}`))
	payload := base64.RawURLEncoding.EncodeToString([]byte(fmt.Sprintf(`{"sub":"tester","exp":%d}`, exp)))
	input := header + "." + payload
	mac := hmac.New(sha256.New, []byte(secret))
	mac.Write([]byte(input))
	return input + "." + base64.RawURLEncoding.EncodeToString(mac.Sum(nil))
}

func newGateway(t *testing.T, upstreamURL string) *httptest.Server {
	t.Helper()
	up, err := url.Parse(upstreamURL)
	if err != nil {
		t.Fatalf("解析上游地址失败: %v", err)
	}
	gw := httptest.NewServer(newServer(up, []byte(testSecret)).routes())
	t.Cleanup(gw.Close)
	return gw
}

func doReq(t *testing.T, method, rawURL, token, body string) *http.Response {
	t.Helper()
	var rd io.Reader
	if body != "" {
		rd = strings.NewReader(body)
	}
	req, err := http.NewRequest(method, rawURL, rd)
	if err != nil {
		t.Fatalf("构造请求失败: %v", err)
	}
	if token != "" {
		req.Header.Set("Authorization", "Bearer "+token)
	}
	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatalf("请求失败: %v", err)
	}
	return resp
}

func assertErrCode(t *testing.T, resp *http.Response, code string) {
	t.Helper()
	var got struct {
		Error struct {
			Code string `json:"code"`
		} `json:"error"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&got); err != nil {
		t.Fatalf("解析错误响应失败: %v", err)
	}
	if got.Error.Code != code {
		t.Fatalf("错误码期望 %s，实际 %s", code, got.Error.Code)
	}
}

func validToken(t *testing.T) string {
	return signJWT(t, testSecret, time.Now().Add(time.Hour).Unix())
}

// 无 JWT 访问 /api/forge/x → 401
func TestProxyRequiresJWT(t *testing.T) {
	up := newFakeUpstream(t)
	gw := newGateway(t, up.srv.URL)

	resp := doReq(t, http.MethodGet, gw.URL+"/api/forge/x", "", "")
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusUnauthorized {
		t.Fatalf("无 JWT 期望 401，实际 %d", resp.StatusCode)
	}
	assertErrCode(t, resp, "UNAUTHORIZED")
}

// 合法 HS256 JWT → 200，且上游收到正确路径与 body
func TestProxyWithValidJWT(t *testing.T) {
	up := newFakeUpstream(t)
	gw := newGateway(t, up.srv.URL)

	body := `{"msg":"hi"}`
	resp := doReq(t, http.MethodPost, gw.URL+"/api/forge/x", validToken(t), body)
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("合法 JWT 期望 200，实际 %d", resp.StatusCode)
	}
	b, _ := io.ReadAll(resp.Body)
	if !strings.Contains(string(b), `"ok":true`) {
		t.Fatalf("响应应为上游透传，实际 %s", b)
	}
	up.mu.Lock()
	defer up.mu.Unlock()
	if up.lastPath != "/api/forge/x" {
		t.Fatalf("上游收到路径错误: %s", up.lastPath)
	}
	if string(up.lastBody) != body {
		t.Fatalf("上游收到 body 错误: %s", up.lastBody)
	}
}

// 坏签名 → 401
func TestProxyBadSignature(t *testing.T) {
	up := newFakeUpstream(t)
	gw := newGateway(t, up.srv.URL)

	token := signJWT(t, "wrong-secret", time.Now().Add(time.Hour).Unix())
	resp := doReq(t, http.MethodGet, gw.URL+"/api/forge/x", token, "")
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusUnauthorized {
		t.Fatalf("坏签名期望 401，实际 %d", resp.StatusCode)
	}
	assertErrCode(t, resp, "UNAUTHORIZED")
}

// exp 过期 → 401
func TestProxyExpiredToken(t *testing.T) {
	up := newFakeUpstream(t)
	gw := newGateway(t, up.srv.URL)

	token := signJWT(t, testSecret, time.Now().Add(-time.Hour).Unix())
	resp := doReq(t, http.MethodGet, gw.URL+"/api/forge/x", token, "")
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusUnauthorized {
		t.Fatalf("过期 token 期望 401，实际 %d", resp.StatusCode)
	}
	assertErrCode(t, resp, "UNAUTHORIZED")
}

// /health 无 JWT → 200 且 agentd="ok"
func TestHealthAggregatesUpstream(t *testing.T) {
	up := newFakeUpstream(t)
	gw := newGateway(t, up.srv.URL)

	resp := doReq(t, http.MethodGet, gw.URL+"/health", "", "")
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("/health 期望 200，实际 %d", resp.StatusCode)
	}
	var got struct {
		Gateway string `json:"gateway"`
		Agentd  string `json:"agentd"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&got); err != nil {
		t.Fatalf("解析 /health 响应失败: %v", err)
	}
	if got.Gateway != "ok" || got.Agentd != "ok" {
		t.Fatalf("/health 字段错误: %+v", got)
	}
}

// 上游挂掉 → /health agentd="unreachable"，/api/forge/* → 502
func TestUpstreamDown(t *testing.T) {
	down := httptest.NewServer(http.NotFoundHandler())
	downURL := down.URL
	down.Close() // 关闭以获得一个确定拒绝连接的端口

	gw := newGateway(t, downURL)

	resp := doReq(t, http.MethodGet, gw.URL+"/health", "", "")
	var h struct {
		Agentd string `json:"agentd"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&h); err != nil {
		t.Fatalf("解析 /health 响应失败: %v", err)
	}
	resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("上游挂掉时 /health 自身仍应 200，实际 %d", resp.StatusCode)
	}
	if h.Agentd != "unreachable" {
		t.Fatalf("上游挂掉时 agentd 应为 unreachable，实际 %s", h.Agentd)
	}

	resp2 := doReq(t, http.MethodGet, gw.URL+"/api/forge/x", validToken(t), "")
	defer resp2.Body.Close()
	if resp2.StatusCode != http.StatusBadGateway {
		t.Fatalf("上游挂掉时反代期望 502，实际 %d", resp2.StatusCode)
	}
	assertErrCode(t, resp2, "UPSTREAM_UNREACHABLE")
}
