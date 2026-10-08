// Package core 放跨模块共享的类型与接口（叶子包，不依赖其它 internal 包）。
//
// 模块边界：
//   - auth/apikeys/billing（用户侧）实现 PrincipalResolver 与 Biller；
//   - gateway/accounts（网关侧）只依赖这里的接口，不直接 import 用户侧实现。
package core

import (
	"context"
	"crypto/rand"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"math"
	"math/big"
	"net/http"
)

// Error 是业务错误：HTTP 状态 + UPPER_SNAKE 错误码 + 中文说明。
// 网关把 Code 转成小写作为 OpenAI 风格的 error.code。
type Error struct {
	Status  int
	Code    string
	Message string
	// Extra 会并入错误响应顶层（如 409 冲突附带 current）。
	Extra map[string]any
	// RetryAfter > 0 时网关写 Retry-After 头（秒）。
	RetryAfter int
}

func (e *Error) Error() string { return fmt.Sprintf("%s: %s", e.Code, e.Message) }

// E 构造业务错误。
func E(status int, code, message string) *Error {
	return &Error{Status: status, Code: code, Message: message}
}

func BadRequest(code, message string) *Error   { return E(http.StatusBadRequest, code, message) }
func Unauthorized(code, message string) *Error { return E(http.StatusUnauthorized, code, message) }
func Forbidden(code, message string) *Error    { return E(http.StatusForbidden, code, message) }
func NotFound(code, message string) *Error     { return E(http.StatusNotFound, code, message) }
func Conflict(code, message string) *Error     { return E(http.StatusConflict, code, message) }

// AsError 取出 *Error（非业务错误返回 nil）。
func AsError(err error) *Error {
	var e *Error
	if errors.As(err, &e) {
		return e
	}
	return nil
}

// 网关/计费共用错误码（UPPER_SNAKE；网关响应里转小写）。
const (
	CodeInvalidAPIKey       = "INVALID_API_KEY"
	CodeUserDisabled        = "USER_DISABLED"
	CodeInsufficientBalance = "INSUFFICIENT_BALANCE"
	// CodeIncludedUsageExhausted：套餐内额度用完且用户关闭了按量付费（§11.1）。
	CodeIncludedUsageExhausted = "INCLUDED_USAGE_EXHAUSTED"
	// CodeSpendLimitReached：本周期按量付费已达用户设置的上限（§11.1）。
	CodeSpendLimitReached    = "SPEND_LIMIT_REACHED"
	CodeKeyQuotaExceeded     = "KEY_QUOTA_EXCEEDED"
	CodeModelNotFound        = "MODEL_NOT_FOUND"
	CodeModelNotAllowed      = "MODEL_NOT_ALLOWED"
	CodeEndpointNotSupported = "ENDPOINT_NOT_SUPPORTED"
	CodeInvalidRequest       = "INVALID_REQUEST"
	CodeRateLimited          = "RATE_LIMITED"
	CodeConcurrencyLimited   = "CONCURRENCY_LIMITED"
	CodeNoAvailableAccount   = "NO_AVAILABLE_ACCOUNT"
	CodeUpstreamError        = "UPSTREAM_ERROR"
)

// Claims 是经 JWT 中间件校验后的登录态（放在请求 context 里）。
type Claims struct {
	UserID    int64
	SessionID string
	Role      string
	Email     string
}

func (c *Claims) IsAdmin() bool { return c != nil && c.Role == "admin" }

type claimsKey struct{}

func WithClaims(ctx context.Context, c *Claims) context.Context {
	return context.WithValue(ctx, claimsKey{}, c)
}

func ClaimsFrom(ctx context.Context) (*Claims, bool) {
	c, ok := ctx.Value(claimsKey{}).(*Claims)
	return c, ok && c != nil
}

// Group 是计费/限流分组。
type Group struct {
	ID               int64    `json:"id"`
	Name             string   `json:"name"`
	RateMultiplier   float64  `json:"rateMultiplier"`
	ConcurrencyLimit int      `json:"concurrencyLimit"`
	RPMLimit         int      `json:"rpmLimit"`
	TPMLimit         int      `json:"tpmLimit"`
	AllowedModels    []string `json:"allowedModels"`
	IsDefault        bool     `json:"isDefault"`
}

// AllowsModel：AllowedModels 为空表示允许全部启用模型。
func (g *Group) AllowsModel(id string) bool {
	if g == nil || len(g.AllowedModels) == 0 {
		return true
	}
	for _, m := range g.AllowedModels {
		if m == id {
			return true
		}
	}
	return false
}

// Principal 是网关/目录请求的调用方（API Key 或 JWT 用户解析而来）。
type Principal struct {
	UserID         int64
	Email          string
	Role           string
	Status         string
	APIKeyID       int64 // JWT 解析时为 0
	APIKeyName     string
	APIKeyKind     string // user|device
	KeyQuota       int64  // micros，0 = 不限
	KeyUsed        int64  // micros
	KeyConcurrency int    // 0 = 不限
	// UserConcurrency = users.concurrency_override（非空时）否则分组 ConcurrencyLimit；0 = 不限。
	UserConcurrency int
	BalanceMicros   int64
	// Group 是有效分组（生效订阅的套餐分组 > 用户分组 > 默认分组）。
	Group Group
}

// ModelCapabilities 对应 models.capabilities JSONB。
type ModelCapabilities struct {
	Vision           bool     `json:"vision"`
	ReasoningEfforts []string `json:"reasoningEfforts"`
	ThinkingMode     string   `json:"thinkingMode,omitempty"` // manual|adaptive；空值保留原目录语义。
	ThinkingAlwaysOn bool     `json:"thinkingAlwaysOn,omitempty"`
	ContextWindow    int      `json:"contextWindow"`
	MaxOutput        int      `json:"maxOutput"`
	Tools            bool     `json:"tools"`
	Responses        bool     `json:"responses"`
}

// Pricing：每 1M tokens 的 micros。
type Pricing struct {
	InputPer1M      int64 `json:"inputPer1M"`
	OutputPer1M     int64 `json:"outputPer1M"`
	CacheReadPer1M  int64 `json:"cacheReadPer1M"`
	CacheWritePer1M int64 `json:"cacheWritePer1M"`
}

// Scaled 返回乘以倍率后的价格（向上取整），用于目录展示。
func (p Pricing) Scaled(m float64) Pricing {
	s := func(v int64) int64 { return int64(math.Ceil(float64(v) * m)) }
	return Pricing{s(p.InputPer1M), s(p.OutputPer1M), s(p.CacheReadPer1M), s(p.CacheWritePer1M)}
}

// 用量池（§11.1）：api = 第三方模型（按 API 价计费），forge = 平台模型。
const (
	PoolAPI   = "api"
	PoolForge = "forge"
)

// NormalizePool 把空值/未知值归为 api。
func NormalizePool(p string) string {
	if p == PoolForge {
		return PoolForge
	}
	return PoolAPI
}

// Model 是对外模型目录条目。
type Model struct {
	ID            string            `json:"id"`
	DisplayName   string            `json:"displayName"`
	Platform      string            `json:"platform"` // openai|anthropic
	UpstreamModel string            `json:"upstreamModel"`
	Capabilities  ModelCapabilities `json:"capabilities"`
	Pricing       Pricing           `json:"pricing"`
	// Pool 是计费用量池（api|forge）。
	Pool      string `json:"pool"`
	Enabled   bool   `json:"enabled"`
	IsDefault bool   `json:"isDefault"`
	Sort      int    `json:"sort"`
}

// Usage 是一次请求的 token 用量（InputTokens 为不含缓存命中的计费输入）。
type Usage struct {
	InputTokens      int64 `json:"inputTokens"`
	OutputTokens     int64 `json:"outputTokens"`
	CacheReadTokens  int64 `json:"cacheReadTokens"`
	CacheWriteTokens int64 `json:"cacheWriteTokens"`
}

func (u Usage) IsZero() bool {
	return u.InputTokens == 0 && u.OutputTokens == 0 && u.CacheReadTokens == 0 && u.CacheWriteTokens == 0
}

// Total 用于 TPM 统计。
func (u Usage) Total() int64 {
	return u.InputTokens + u.OutputTokens + u.CacheReadTokens + u.CacheWriteTokens
}

// ComputeCost = ⌈Σ(tokens×price)/1e6 × multiplier⌉（micros）。
func ComputeCost(p Pricing, u Usage, multiplier float64) int64 {
	num := u.InputTokens*p.InputPer1M + u.OutputTokens*p.OutputPer1M +
		u.CacheReadTokens*p.CacheReadPer1M + u.CacheWriteTokens*p.CacheWritePer1M
	if num <= 0 || multiplier <= 0 {
		return 0
	}
	return int64(math.Ceil(float64(num) * multiplier / 1e6))
}

// UsageRecord 是网关交给计费结算的一次请求记录。
type UsageRecord struct {
	RequestID     string
	Principal     *Principal
	AccountID     int64 // 0 = 未选到账号
	Model         *Model
	UpstreamModel string
	Endpoint      string // chat|responses|messages|embeddings
	Stream        bool
	Usage         Usage
	Status        string // ok|error
	ErrorCode     string
	HTTPStatus    int
	LatencyMs     int
	FirstTokenMs  int
	SessionKey    string
	IP            string
}

// PrincipalResolver 由 apikeys 包实现。
type PrincipalResolver interface {
	// AuthenticateAPIKey 校验平台 Key（sk-rf-…）：不存在/吊销/过期 → INVALID_API_KEY(401)，
	// 用户禁用 → USER_DISABLED(403)。
	AuthenticateAPIKey(ctx context.Context, rawKey string) (*Principal, error)
	// PrincipalForUser 为 JWT 用户构造 Principal（APIKeyID=0），用于模型目录。
	PrincipalForUser(ctx context.Context, userID int64) (*Principal, error)
}

// Biller 由 billing 包实现。
type Biller interface {
	// Precheck：余额/套餐/Key 额度预检；不通过返回 INSUFFICIENT_BALANCE 或 KEY_QUOTA_EXCEEDED(402)。
	Precheck(ctx context.Context, p *Principal, m *Model) error
	// Settle：单事务结算并写 usage_logs/balance_ledger；返回本次费用（micros）。
	Settle(ctx context.Context, rec *UsageRecord) (int64, error)
}

// SHA256Hex 用于 API Key / refresh token 落库。
func SHA256Hex(s string) string {
	sum := sha256.Sum256([]byte(s))
	return hex.EncodeToString(sum[:])
}

const base62 = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"

// RandomString 生成 n 位 base62 随机串（crypto/rand）。
func RandomString(n int) string {
	out := make([]byte, n)
	max := big.NewInt(int64(len(base62)))
	for i := range out {
		v, err := rand.Int(rand.Reader, max)
		if err != nil {
			panic(err)
		}
		out[i] = base62[v.Int64()]
	}
	return string(out)
}
