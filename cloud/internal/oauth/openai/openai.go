// Package openai 实现 ChatGPT/Codex 订阅账号的 OpenAI OAuth：PKCE、授权链接、code 换 token、
// 刷新、JWT 声明解析（只解码、不验签）、~/.codex/auth.json 解析，以及 Codex 上游请求头与额度头
// （15_CLOUD_SERVICE.md §6）。所有上游地址由调用方从配置传入。
package openai

import (
	"bytes"
	"context"
	"crypto/rand"
	"crypto/sha256"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/url"
	"strconv"
	"strings"
	"sync"
	"time"
	"unicode/utf8"
)

const (
	ClientID         = "app_EMoamEEZ73f0CkXaXp7hrann"
	RedirectURI      = "http://localhost:1455/auth/callback"
	AuthorizeScope   = "openid profile email offline_access"
	RefreshScope     = "openid profile email"
	Originator       = "codex_cli_rs"
	DefaultUserAgent = "codex_cli_rs/0.153.2"

	// AuthClaim / ProfileClaim 是 OpenAI token 里的自定义声明名。
	AuthClaim    = "https://api.openai.com/auth"
	ProfileClaim = "https://api.openai.com/profile"
)

// ---------- PKCE 与授权链接 ----------

// PKCE 是一次授权会话的 verifier / S256 challenge / state。
type PKCE struct {
	Verifier  string
	Challenge string
	State     string
}

func NewPKCE() PKCE {
	v := randomToken(64)
	return PKCE{Verifier: v, Challenge: S256Challenge(v), State: randomToken(32)}
}

// S256Challenge = base64url(sha256(verifier))，无填充。
func S256Challenge(verifier string) string {
	sum := sha256.Sum256([]byte(verifier))
	return base64.RawURLEncoding.EncodeToString(sum[:])
}

func randomToken(n int) string {
	b := make([]byte, n)
	_, _ = rand.Read(b) // crypto/rand.Read 自 Go 1.24 起不会返回错误
	return base64.RawURLEncoding.EncodeToString(b)
}

// AuthorizeURL 生成 `{authBase}/oauth/authorize?...`（与 Codex CLI 相同的参数）。
func AuthorizeURL(authBase string, p PKCE) string {
	q := url.Values{}
	q.Set("response_type", "code")
	q.Set("client_id", ClientID)
	q.Set("redirect_uri", RedirectURI)
	q.Set("scope", AuthorizeScope)
	q.Set("code_challenge", p.Challenge)
	q.Set("code_challenge_method", "S256")
	q.Set("id_token_add_organizations", "true")
	q.Set("codex_cli_simplified_flow", "true")
	q.Set("state", p.State)
	q.Set("originator", Originator)
	// 空格编码成 %20（与 Codex CLI 一致）；其余值里的 '+' 已被 Encode 转义为 %2B。
	return strings.TrimRight(authBase, "/") + "/oauth/authorize?" + strings.ReplaceAll(q.Encode(), "+", "%20")
}

// ParseCallback 从浏览器回调地址（也接受裸 query 串）取 code 与 state；回调带 error 时返回错误。
func ParseCallback(raw string) (code, state string, err error) {
	raw = strings.TrimSpace(raw)
	if raw == "" {
		return "", "", errors.New("回调地址为空")
	}
	var q url.Values
	if u, perr := url.Parse(raw); perr == nil && (u.Scheme != "" || strings.HasPrefix(raw, "/")) {
		q = u.Query()
		if q.Get("code") == "" && q.Get("error") == "" && u.Fragment != "" {
			q, _ = url.ParseQuery(u.Fragment)
		}
	} else {
		q, _ = url.ParseQuery(strings.TrimPrefix(raw, "?"))
	}
	if e := q.Get("error"); e != "" {
		msg := e
		if d := q.Get("error_description"); d != "" {
			msg += ": " + d
		}
		return "", "", fmt.Errorf("授权失败（%s）", msg)
	}
	code = q.Get("code")
	if code == "" {
		return "", "", errors.New("回调地址里没有 code 参数")
	}
	return code, q.Get("state"), nil
}

// ---------- token 端点 ----------

// Tokens 是 /oauth/token 的响应（refresh 时 id_token / refresh_token 可能缺省）。
type Tokens struct {
	IDToken      string `json:"id_token"`
	AccessToken  string `json:"access_token"`
	RefreshToken string `json:"refresh_token"`
	ExpiresIn    int64  `json:"expires_in"`
}

// TokenError 是 token 端点的非 2xx 响应。
type TokenError struct {
	Status      int
	Code        string
	Description string
}

func (e *TokenError) Error() string {
	msg := e.Code
	if e.Description != "" {
		if msg != "" {
			msg += ": "
		}
		msg += e.Description
	}
	if msg == "" {
		msg = http.StatusText(e.Status)
	}
	return fmt.Sprintf("token 端点返回 HTTP %d（%s）", e.Status, msg)
}

// Client 调 `{AuthURL}/oauth/token`；HTTP 为 nil 时用 http.DefaultClient。
type Client struct {
	AuthURL string
	HTTP    *http.Client
}

// ExchangeCode 用授权码 + PKCE verifier 换 token（表单编码）。
func (c *Client) ExchangeCode(ctx context.Context, code, verifier string) (*Tokens, error) {
	form := url.Values{
		"grant_type":    {"authorization_code"},
		"code":          {code},
		"redirect_uri":  {RedirectURI},
		"client_id":     {ClientID},
		"code_verifier": {verifier},
	}
	return c.post(ctx, "application/x-www-form-urlencoded", []byte(form.Encode()))
}

// Refresh 用 refresh token 换新 token（JSON 编码）。
func (c *Client) Refresh(ctx context.Context, refreshToken string) (*Tokens, error) {
	body, _ := json.Marshal(map[string]string{
		"client_id":     ClientID,
		"grant_type":    "refresh_token",
		"refresh_token": refreshToken,
		"scope":         RefreshScope,
	})
	return c.post(ctx, "application/json", body)
}

func (c *Client) post(ctx context.Context, contentType string, body []byte) (*Tokens, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, strings.TrimRight(c.AuthURL, "/")+"/oauth/token", bytes.NewReader(body))
	if err != nil {
		return nil, err
	}
	req.Header.Set("Content-Type", contentType)
	req.Header.Set("Accept", "application/json")
	hc := c.HTTP
	if hc == nil {
		hc = http.DefaultClient
	}
	resp, err := hc.Do(req)
	if err != nil {
		return nil, fmt.Errorf("请求 token 端点失败: %w", err)
	}
	defer resp.Body.Close()
	raw, err := io.ReadAll(io.LimitReader(resp.Body, 1<<20))
	if err != nil {
		return nil, fmt.Errorf("读取 token 响应失败: %w", err)
	}
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return nil, parseTokenError(resp.StatusCode, raw)
	}
	var t Tokens
	if err := json.Unmarshal(raw, &t); err != nil {
		return nil, errors.New("token 响应不是合法 JSON")
	}
	if t.AccessToken == "" {
		return nil, errors.New("token 响应缺少 access_token")
	}
	return &t, nil
}

func parseTokenError(status int, body []byte) *TokenError {
	e := &TokenError{Status: status}
	var v struct {
		Error            json.RawMessage `json:"error"`
		ErrorDescription string          `json:"error_description"`
		Message          string          `json:"message"`
	}
	if json.Unmarshal(body, &v) == nil {
		var s string
		if json.Unmarshal(v.Error, &s) == nil {
			e.Code = s
		} else {
			var o struct {
				Code    string `json:"code"`
				Type    string `json:"type"`
				Message string `json:"message"`
			}
			if json.Unmarshal(v.Error, &o) == nil {
				e.Code = firstNonEmpty(o.Code, o.Type)
				e.Description = o.Message
			}
		}
		if e.Description == "" {
			e.Description = firstNonEmpty(v.ErrorDescription, v.Message)
		}
	}
	if e.Code == "" && e.Description == "" {
		e.Description = truncate(strings.TrimSpace(string(body)), 200)
	}
	return e
}

// ---------- JWT 声明 ----------

// DecodeJWT 解码 JWT 载荷（不验签；数字保留为 json.Number）。
func DecodeJWT(token string) (map[string]any, error) {
	parts := strings.Split(strings.TrimSpace(token), ".")
	if len(parts) < 2 || parts[1] == "" {
		return nil, errors.New("不是合法的 JWT")
	}
	seg := strings.TrimRight(parts[1], "=")
	b, err := base64.RawURLEncoding.DecodeString(seg)
	if err != nil {
		if b, err = base64.RawStdEncoding.DecodeString(seg); err != nil {
			return nil, errors.New("JWT 载荷不是合法 base64")
		}
	}
	dec := json.NewDecoder(bytes.NewReader(b))
	dec.UseNumber()
	var m map[string]any
	if err := dec.Decode(&m); err != nil || m == nil {
		return nil, errors.New("JWT 载荷不是合法 JSON")
	}
	return m, nil
}

// Claims 是从 id_token / access_token 取出的账号信息。
type Claims struct {
	Email     string
	AccountID string // chatgpt_account_id
	PlanType  string // chatgpt_plan_type
	UserID    string // chatgpt_user_id
	ExpiresAt time.Time
}

func ParseClaims(token string) (Claims, error) {
	var c Claims
	m, err := DecodeJWT(token)
	if err != nil {
		return c, err
	}
	c.Email = str(m["email"])
	if auth, ok := m[AuthClaim].(map[string]any); ok {
		c.AccountID = str(auth["chatgpt_account_id"])
		c.PlanType = str(auth["chatgpt_plan_type"])
		c.UserID = firstNonEmpty(str(auth["chatgpt_user_id"]), str(auth["user_id"]))
	}
	if c.Email == "" {
		if prof, ok := m[ProfileClaim].(map[string]any); ok {
			c.Email = str(prof["email"])
		}
	}
	if exp := num(m["exp"]); exp > 0 {
		c.ExpiresAt = time.Unix(int64(exp), 0).UTC()
	}
	return c, nil
}

// Identity 汇总一组 token 描述的上游账号。
type Identity struct {
	AccountID string
	Email     string
	PlanType  string
	// ExpiresAt 取 access_token 的 exp，其次 expiresIn 秒；都没有为 nil。
	ExpiresAt *time.Time
}

// IdentityFromTokens：账号/邮箱/套餐优先取 id_token，缺失时退到 access_token 的同名声明。
func IdentityFromTokens(idToken, accessToken string, expiresIn int64) Identity {
	var id Identity
	idc, _ := ParseClaims(idToken)
	atc, atErr := ParseClaims(accessToken)
	id.AccountID = firstNonEmpty(idc.AccountID, atc.AccountID)
	id.Email = firstNonEmpty(idc.Email, atc.Email)
	id.PlanType = firstNonEmpty(idc.PlanType, atc.PlanType)
	switch {
	case atErr == nil && !atc.ExpiresAt.IsZero():
		t := atc.ExpiresAt
		id.ExpiresAt = &t
	case expiresIn > 0:
		t := time.Now().Add(time.Duration(expiresIn) * time.Second).UTC()
		id.ExpiresAt = &t
	}
	return id
}

// ---------- ~/.codex/auth.json ----------

// AuthFile 是 auth.json 的规范化内容。接受 Codex CLI 的 `{"OPENAI_API_KEY", "tokens":{…}}`，
// 也接受 token 字段在顶层或 camelCase 的导出变体。
type AuthFile struct {
	APIKey       string
	IDToken      string
	AccessToken  string
	RefreshToken string
	AccountID    string
	Email        string
	LastRefresh  *time.Time
}

func (a *AuthFile) HasTokens() bool { return a.AccessToken != "" || a.RefreshToken != "" }

// ParseAuthJSON 解析 auth.json 原文（容忍 UTF-8 BOM 与首尾空白）。
func ParseAuthJSON(text []byte) (*AuthFile, error) {
	text = bytes.TrimSpace(bytes.TrimPrefix(bytes.TrimSpace(text), []byte("\xef\xbb\xbf")))
	if len(text) == 0 {
		return nil, errors.New("auth.json 内容为空")
	}
	var top map[string]json.RawMessage
	if err := json.Unmarshal(text, &top); err != nil || top == nil {
		return nil, errors.New("auth.json 不是合法的 JSON 对象")
	}
	a := &AuthFile{}
	a.APIKey = rawStr(top, "OPENAI_API_KEY", "openai_api_key", "apiKey", "api_key")
	a.Email = rawStr(top, "email")
	tokens := top
	if raw, ok := top["tokens"]; ok && string(raw) != "null" {
		var inner map[string]json.RawMessage
		if err := json.Unmarshal(raw, &inner); err != nil {
			return nil, errors.New("auth.json 的 tokens 字段不是对象")
		}
		tokens = inner
	}
	a.IDToken = rawStr(tokens, "id_token", "idToken")
	a.AccessToken = rawStr(tokens, "access_token", "accessToken")
	a.RefreshToken = rawStr(tokens, "refresh_token", "refreshToken")
	a.AccountID = rawStr(tokens, "account_id", "accountId", "chatgpt_account_id")
	if lr := rawStr(top, "last_refresh", "lastRefresh"); lr != "" {
		if t, err := time.Parse(time.RFC3339Nano, lr); err == nil {
			t = t.UTC()
			a.LastRefresh = &t
		}
	}
	if !a.HasTokens() && a.APIKey == "" {
		return nil, errors.New("auth.json 里既没有 tokens 也没有 OPENAI_API_KEY")
	}
	return a, nil
}

func rawStr(m map[string]json.RawMessage, keys ...string) string {
	for _, k := range keys {
		raw, ok := m[k]
		if !ok {
			continue
		}
		var s string
		if json.Unmarshal(raw, &s) == nil && strings.TrimSpace(s) != "" {
			return strings.TrimSpace(s)
		}
	}
	return ""
}

// ---------- Codex 请求头与额度头 ----------

// SetCodexHeaders 写 Codex 订阅上游要求的请求头；userAgent 不以 "codex" 开头时改用 DefaultUserAgent。
// Accept / Content-Type 由调用方按用途设置。
func SetCodexHeaders(h http.Header, accessToken, accountID, sessionID, userAgent string) {
	h.Set("Authorization", "Bearer "+accessToken)
	h.Set("chatgpt-account-id", accountID)
	h.Set("OpenAI-Beta", "responses=experimental")
	h.Set("originator", Originator)
	if sessionID != "" {
		h.Set("session_id", sessionID)
	}
	if !strings.HasPrefix(strings.ToLower(userAgent), "codex") {
		userAgent = DefaultUserAgent
	}
	h.Set("User-Agent", userAgent)
}

// RateWindow 是一组 x-codex-{primary|secondary}-* 头（字段缺失为 nil）。
type RateWindow struct {
	UsedPercent       *float64
	WindowMinutes     *int
	ResetAfterSeconds *int
}

// Exhausted 表示该窗口已用满（used-percent ≥ 100）。
func (w *RateWindow) Exhausted() bool {
	return w != nil && w.UsedPercent != nil && *w.UsedPercent >= 100
}

// ParseCodexRateLimits 解析 Codex 响应里的额度头；某个窗口一个头都没有时返回 nil。
func ParseCodexRateLimits(h http.Header) (primary, secondary *RateWindow) {
	return parseWindow(h, "primary"), parseWindow(h, "secondary")
}

func parseWindow(h http.Header, which string) *RateWindow {
	prefix := "x-codex-" + which + "-"
	var w RateWindow
	found := false
	if v, err := strconv.ParseFloat(strings.TrimSpace(h.Get(prefix+"used-percent")), 64); err == nil {
		w.UsedPercent = &v
		found = true
	}
	if v, err := strconv.ParseFloat(strings.TrimSpace(h.Get(prefix+"window-minutes")), 64); err == nil {
		n := int(v)
		w.WindowMinutes = &n
		found = true
	}
	if v, err := strconv.ParseFloat(strings.TrimSpace(h.Get(prefix+"reset-after-seconds")), 64); err == nil && v >= 0 {
		n := int(v)
		w.ResetAfterSeconds = &n
		found = true
	}
	if !found {
		return nil
	}
	return &w
}

// ---------- 上游 HTTP 客户端（代理） ----------

var (
	clientsMu sync.Mutex
	clients   = map[string]*http.Client{}
)

// HTTPClient 返回上游 HTTP 客户端：accountProxy 优先，其次 globalProxy，都为空时走环境代理。
// 同一代理地址复用同一个连接池。客户端不设整体超时（流式响应可能很长），由调用方的 ctx 控制。
func HTTPClient(accountProxy, globalProxy string) (*http.Client, error) {
	proxy := strings.TrimSpace(accountProxy)
	if proxy == "" {
		proxy = strings.TrimSpace(globalProxy)
	}
	clientsMu.Lock()
	defer clientsMu.Unlock()
	if c, ok := clients[proxy]; ok {
		return c, nil
	}
	t := &http.Transport{
		Proxy:                 http.ProxyFromEnvironment,
		DialContext:           (&net.Dialer{Timeout: 30 * time.Second, KeepAlive: 30 * time.Second}).DialContext,
		ForceAttemptHTTP2:     true,
		MaxIdleConns:          256,
		MaxIdleConnsPerHost:   64,
		IdleConnTimeout:       90 * time.Second,
		TLSHandshakeTimeout:   15 * time.Second,
		ExpectContinueTimeout: time.Second,
		ResponseHeaderTimeout: 10 * time.Minute,
	}
	if proxy != "" {
		u, err := ValidateProxyURL(proxy)
		if err != nil {
			return nil, err
		}
		t.Proxy = http.ProxyURL(u)
	}
	c := &http.Client{Transport: t}
	clients[proxy] = c
	return c, nil
}

// ValidateProxyURL 校验代理地址（http/https/socks5/socks5h）。错误信息不回显地址（可能含密码）。
func ValidateProxyURL(s string) (*url.URL, error) {
	u, err := url.Parse(strings.TrimSpace(s))
	if err != nil || u.Host == "" {
		return nil, errors.New("代理地址无效")
	}
	switch strings.ToLower(u.Scheme) {
	case "http", "https", "socks5", "socks5h":
		return u, nil
	}
	return nil, errors.New("代理协议只支持 http/https/socks5/socks5h")
}

// ---------- 小工具 ----------

func str(v any) string {
	s, _ := v.(string)
	return strings.TrimSpace(s)
}

func num(v any) float64 {
	switch n := v.(type) {
	case json.Number:
		f, _ := n.Float64()
		return f
	case float64:
		return n
	}
	return 0
}

func firstNonEmpty(vs ...string) string {
	for _, v := range vs {
		if v != "" {
			return v
		}
	}
	return ""
}

func truncate(s string, n int) string {
	if len(s) <= n {
		return s
	}
	for n > 0 && !utf8.RuneStart(s[n]) {
		n--
	}
	return s[:n] + "…"
}
