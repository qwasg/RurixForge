// Package gateway：/v1 模型网关（鉴权、限流、并发槽、选号、协议转换、流式代理、用量截取、结算）
// 与 /api/v1/models/catalog（15_CLOUD_SERVICE.md §3.3、§4）。
//
// 管线：鉴权 → 模型存在且在有效分组内 → 端点与平台兼容 → 计费预检 → RPM/TPM → 用户/Key 并发槽 →
// 选号（粘性 → 优先级 → 负载率 → 权重随机）+ 换号重试 → 转发与回传（截取用量）→ 结算。
// 进入选号阶段后无论成败都会结算（usage_logs），并释放槽位、累加 TPM、更新账号 last_used_at。
package gateway

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"log/slog"
	"net/http"
	"strings"
	"sync"
	"time"

	"github.com/go-chi/chi/v5"
	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/redis/go-redis/v9"

	"forge-cloud/internal/accounts"
	"forge-cloud/internal/config"
	"forge-cloud/internal/core"
	"forge-cloud/internal/httpx"
	"forge-cloud/internal/ratelimit"
	"forge-cloud/internal/scheduler"
	"forge-cloud/internal/syssettings"
)

type Deps struct {
	DB         *pgxpool.Pool
	Redis      *redis.Client
	Config     *config.Config
	Log        *slog.Logger
	Settings   *syssettings.Store
	Accounts   *accounts.Service
	Principals core.PrincipalResolver
	Biller     core.Biller
}

type Service struct {
	d       Deps
	limiter *ratelimit.Limiter
	sched   *scheduler.Scheduler
}

func New(d Deps) *Service {
	lim := ratelimit.New(d.Redis)
	return &Service{d: d, limiter: lim, sched: scheduler.New(d.Redis, lim)}
}

// Scheduler 暴露调度器（测试可替换随机源）。
func (s *Service) Scheduler() *scheduler.Scheduler { return s.sched }

// Limiter 暴露限流器。
func (s *Service) Limiter() *ratelimit.Limiter { return s.limiter }

const (
	epChat       = "chat"
	epResponses  = "responses"
	epMessages   = "messages"
	epEmbeddings = "embeddings"

	maxRequestBody  = 32 << 20
	maxUpstreamBody = 64 << 20
	maxErrorBody    = 64 << 10
	maxSessionKey   = 256

	slotRenewEvery   = 5 * time.Minute
	networkCooldown  = 30 * time.Second
	serverCooldown   = 60 * time.Second
	finalizeTimeout  = 10 * time.Second
	statusClientGone = 499
)

// MountGateway 挂 /models、/chat/completions、/responses、/messages、/embeddings（相对 /v1）。
func (s *Service) MountGateway(r chi.Router) {
	r.Get("/models", s.handleModels)
	r.Post("/chat/completions", s.endpoint(epChat))
	r.Post("/responses", s.endpoint(epResponses))
	r.Post("/messages", s.endpoint(epMessages))
	r.Post("/embeddings", s.endpoint(epEmbeddings))
}

// MountCatalog 挂 /models/catalog（相对 /api/v1，已在 RequireUser 分组内）。
func (s *Service) MountCatalog(r chi.Router) {
	r.Get("/models/catalog", s.handleCatalog)
}

// requestID 取请求 ID（无中间件时自行生成），并写 X-Forge-Request-Id。
func requestID(w http.ResponseWriter, r *http.Request) string {
	rid := httpx.RequestIDFrom(r.Context())
	if rid == "" {
		rid = "req_" + core.RandomString(20)
		w.Header().Set("X-Request-Id", rid)
	}
	w.Header().Set("X-Forge-Request-Id", rid)
	return rid
}

// authenticate：`Authorization: Bearer sk-rf-…` 或 `x-api-key`。
func (s *Service) authenticate(r *http.Request) (*core.Principal, error) {
	key := ""
	if h := r.Header.Get("Authorization"); len(h) > 7 && strings.EqualFold(h[:7], "bearer ") {
		key = strings.TrimSpace(h[7:])
	}
	if key == "" {
		key = strings.TrimSpace(r.Header.Get("x-api-key"))
	}
	if key == "" {
		return nil, core.Unauthorized(core.CodeInvalidAPIKey, "缺少 API Key")
	}
	p, err := s.d.Principals.AuthenticateAPIKey(r.Context(), key)
	if err != nil {
		return nil, err
	}
	if p == nil {
		return nil, core.Unauthorized(core.CodeInvalidAPIKey, "API Key 无效")
	}
	if p.Status != "" && p.Status != "active" {
		return nil, core.Forbidden(core.CodeUserDisabled, "用户已被禁用")
	}
	return p, nil
}

// call 是一次网关请求的状态。
type call struct {
	s        *Service
	w        http.ResponseWriter
	rc       *http.ResponseController
	r        *http.Request
	ctx      context.Context
	endpoint string
	start    time.Time
	rid      string
	ip       string
	settings syssettings.Settings

	p            *core.Principal
	headModel    string
	model        *core.Model
	body         []byte
	stream       bool
	includeUsage bool
	sessionKey   string

	randomSession string
	userSlot      string
	keySlot       string
	leaseMu       sync.Mutex
	lease         *scheduler.Lease
	quotas        []quotaRecord

	started       bool
	status        int
	errCode       string
	usage         core.Usage
	accountID     int64
	upstreamModel string
	firstToken    time.Duration
}

type quotaRecord struct {
	accountID int64
	header    http.Header
}

// upstreamFailure 是可换号重试的一次失败（账号已冷却/标错）。
type upstreamFailure struct {
	status     int // 上游 HTTP 状态；0 = 网络错误
	message    string
	retryAfter time.Duration
}

func (s *Service) endpoint(ep string) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		c := &call{
			s: s, w: w, rc: http.NewResponseController(w), r: r, ctx: r.Context(), endpoint: ep,
			start: time.Now(), ip: httpx.ClientIP(r, s.d.Config.TrustProxy),
		}
		c.rid = requestID(w, r)
		c.serve()
	}
}

func (c *call) log() *slog.Logger {
	return c.s.d.Log.With("rid", c.rid, "endpoint", c.endpoint)
}

// fail 写错误响应（尚未写出字节时）并记录结果。
func (c *call) fail(e *core.Error) {
	c.status = e.Status
	c.errCode = strings.ToLower(e.Code)
	if c.started {
		return
	}
	c.started = true
	writeGatewayError(c.w, c.endpoint, e)
}

func (c *call) failErr(err error) {
	e := core.AsError(err)
	if e == nil {
		c.log().Error("gateway internal error", "err", err)
		e = core.E(http.StatusInternalServerError, "INTERNAL", "服务器内部错误")
	}
	c.fail(e)
}

func (c *call) serve() {
	p, err := c.s.authenticate(c.r)
	if err != nil {
		c.failErr(err)
		return
	}
	c.p = p
	if !c.readBody() {
		return
	}
	m, err := c.s.d.Accounts.Model(c.ctx, c.headModel)
	if err != nil {
		c.failErr(err)
		return
	}
	if !p.Group.AllowsModel(m.ID) {
		c.fail(core.Forbidden(core.CodeModelNotAllowed, "当前分组不能使用该模型"))
		return
	}
	c.model = m
	if !platformSupports(m.Platform, c.endpoint) {
		c.fail(core.BadRequest(core.CodeEndpointNotSupported, "该模型不支持此端点"))
		return
	}
	if err := c.s.d.Biller.Precheck(c.ctx, p, m); err != nil {
		c.failErr(err)
		return
	}
	if c.settings, err = c.s.d.Settings.Get(c.ctx); err != nil {
		c.failErr(err)
		return
	}
	if !c.checkRateLimits() || !c.acquireSlots() {
		return
	}
	stop := c.keepalive()
	defer func() {
		stop()
		c.finalize()
	}()
	c.dispatch()
}

func (c *call) readBody() bool {
	body, err := io.ReadAll(http.MaxBytesReader(c.w, c.r.Body, maxRequestBody))
	if err != nil {
		var mbe *http.MaxBytesError
		if errors.As(err, &mbe) {
			c.fail(core.E(http.StatusRequestEntityTooLarge, core.CodeInvalidRequest, "请求体超过 32 MiB"))
		} else {
			c.fail(core.BadRequest(core.CodeInvalidRequest, "读取请求体失败"))
		}
		return false
	}
	var head struct {
		Model          string `json:"model"`
		Stream         bool   `json:"stream"`
		PromptCacheKey string `json:"prompt_cache_key"`
		StreamOptions  *struct {
			IncludeUsage bool `json:"include_usage"`
		} `json:"stream_options"`
	}
	if err := json.Unmarshal(body, &head); err != nil {
		c.fail(core.BadRequest(core.CodeInvalidRequest, "请求体不是合法 JSON"))
		return false
	}
	c.headModel = strings.TrimSpace(head.Model)
	if c.headModel == "" {
		c.fail(core.BadRequest(core.CodeInvalidRequest, "缺少 model"))
		return false
	}
	c.body = body
	c.stream = head.Stream && c.endpoint != epEmbeddings
	c.includeUsage = head.StreamOptions != nil && head.StreamOptions.IncludeUsage
	c.sessionKey = firstNonEmpty(
		c.r.Header.Get("X-Forge-Session"), c.r.Header.Get("session_id"),
		c.r.Header.Get("conversation_id"), head.PromptCacheKey,
	)
	if len(c.sessionKey) > maxSessionKey {
		c.sessionKey = c.sessionKey[:maxSessionKey]
	}
	return true
}

func (c *call) checkRateLimits() bool {
	g := c.p.Group
	ok, retry, err := c.s.limiter.CheckRPM(c.ctx, c.p.UserID, g.RPMLimit)
	if err == nil && ok {
		ok, retry, err = c.s.limiter.CheckTPM(c.ctx, c.p.UserID, g.TPMLimit)
	}
	if err != nil {
		c.failErr(err)
		return false
	}
	if !ok {
		c.fail(&core.Error{Status: http.StatusTooManyRequests, Code: core.CodeRateLimited,
			Message: "请求过于频繁，请稍后再试", RetryAfter: retry})
		return false
	}
	return true
}

func (c *call) acquireSlots() bool {
	lim := c.s.limiter
	concurrencyLimited := &core.Error{Status: http.StatusTooManyRequests, Code: core.CodeConcurrencyLimited,
		Message: "并发请求数超过上限", RetryAfter: 1}
	userSlot := ratelimit.UserSlotKey(c.p.UserID)
	ok, err := lim.Acquire(c.ctx, userSlot, c.p.UserConcurrency, c.rid)
	if err != nil {
		c.failErr(err)
		return false
	}
	if !ok {
		c.fail(concurrencyLimited)
		return false
	}
	c.userSlot = userSlot
	if c.p.APIKeyID > 0 {
		keySlot := ratelimit.KeySlotKey(c.p.APIKeyID)
		ok, err = lim.Acquire(c.ctx, keySlot, c.p.KeyConcurrency, c.rid)
		if err != nil || !ok {
			_ = lim.Release(context.WithoutCancel(c.ctx), userSlot, c.rid)
			c.userSlot = ""
			if err != nil {
				c.failErr(err)
			} else {
				c.fail(concurrencyLimited)
			}
			return false
		}
		c.keySlot = keySlot
	}
	return true
}

// keepalive 为长请求周期性续租用户/Key/账号并发槽（租约 15 分钟）。
func (c *call) keepalive() func() {
	done := make(chan struct{})
	go func() {
		t := time.NewTicker(slotRenewEvery)
		defer t.Stop()
		for {
			select {
			case <-done:
				return
			case <-t.C:
				ctx := context.WithoutCancel(c.ctx)
				_ = c.s.limiter.Extend(ctx, c.userSlot, c.rid)
				if c.keySlot != "" {
					_ = c.s.limiter.Extend(ctx, c.keySlot, c.rid)
				}
				c.leaseMu.Lock()
				l := c.lease
				c.leaseMu.Unlock()
				if l != nil {
					_ = c.s.sched.Extend(ctx, l)
				}
			}
		}
	}()
	return func() { close(done) }
}

func (c *call) setLease(l *scheduler.Lease) {
	c.leaseMu.Lock()
	c.lease = l
	c.leaseMu.Unlock()
}

func (c *call) dispatch() {
	cands, err := c.s.d.Accounts.Candidates(c.ctx, c.p.Group.ID, c.model.Platform)
	if err != nil {
		c.failErr(err)
		return
	}
	if len(cands) == 0 {
		c.fail(core.E(http.StatusServiceUnavailable, core.CodeNoAvailableAccount, "当前没有可用的上游账号"))
		return
	}
	byID := map[int64]*accounts.Account{}
	var pool []scheduler.Candidate
	modelSupported := false
	for _, a := range cands {
		if !a.SupportsModel(c.model) {
			continue
		}
		modelSupported = true
		if a.SupportsEndpoint(c.endpoint) {
			byID[a.ID] = a
			pool = append(pool, scheduler.Candidate{ID: a.ID, Priority: a.Priority, Weight: a.Weight, ConcurrencyLimit: a.ConcurrencyLimit})
		}
	}
	if len(pool) == 0 {
		if !modelSupported {
			c.fail(core.E(http.StatusServiceUnavailable, core.CodeNoAvailableAccount, "当前没有支持该模型的上游账号"))
			return
		}
		c.fail(core.BadRequest(core.CodeEndpointNotSupported, "该模型的上游账号不支持此端点"))
		return
	}
	maxAttempts := 1 + max(c.settings.MaxFailoverRetries, 0)
	exclude := map[int64]bool{}
	var last *upstreamFailure
	for attempt := 0; attempt < maxAttempts; attempt++ {
		lease, err := c.s.sched.Pick(c.ctx, pool, scheduler.Request{
			UserID: c.p.UserID, SessionKey: c.sessionKey, Member: c.rid, Exclude: exclude,
			StickyTTL: time.Duration(c.settings.StickyTTLSeconds) * time.Second,
		})
		if err != nil {
			switch {
			case errors.Is(err, scheduler.ErrNoCandidates):
			case errors.Is(err, scheduler.ErrAllBusy) && last == nil:
				c.fail(&core.Error{Status: http.StatusServiceUnavailable, Code: core.CodeNoAvailableAccount,
					Message: "上游账号并发已满，请稍后重试", RetryAfter: 1})
				return
			case errors.Is(err, scheduler.ErrAllBusy):
			case c.ctx.Err() != nil:
				c.clientClosed()
				return
			default:
				c.failErr(err)
				return
			}
			break
		}
		a := byID[lease.AccountID]
		c.accountID = a.ID
		c.setLease(lease)
		failure := c.attempt(a)
		c.setLease(nil)
		_ = c.s.sched.Release(context.WithoutCancel(c.ctx), lease)
		if failure == nil {
			return
		}
		last = failure
		exclude[a.ID] = true
		if c.ctx.Err() != nil {
			c.clientClosed()
			return
		}
		c.log().Info("upstream failover", "account", a.ID, "status", failure.status, "msg", failure.message)
	}
	c.exhausted(last)
}

// exhausted：重试用尽，转发最后一次上游状态（429/5xx 原样，其余 502）与信息。
func (c *call) exhausted(last *upstreamFailure) {
	if last == nil {
		c.fail(core.E(http.StatusServiceUnavailable, core.CodeNoAvailableAccount, "当前没有可用的上游账号"))
		return
	}
	e := &core.Error{Status: http.StatusBadGateway, Code: core.CodeUpstreamError, Message: "上游请求失败：" + last.message}
	switch {
	case last.status == http.StatusTooManyRequests:
		e.Status = http.StatusTooManyRequests
		e.RetryAfter = retryAfterSeconds(last.retryAfter)
	case last.status >= 500 && last.status <= 599:
		e.Status = last.status
	}
	c.fail(e)
}

// attempt 在一个账号上完成请求。返回非 nil 表示可换号重试（账号已冷却/标错）；nil 表示已结束（成功或已写错误）。
func (c *call) attempt(a *accounts.Account) *upstreamFailure {
	acc := c.s.d.Accounts
	bg := context.WithoutCancel(c.ctx)
	creds, err := acc.FreshCredentials(c.ctx, a)
	if err != nil {
		msg := "获取上游凭据失败：" + err.Error()
		if e := core.AsError(err); e != nil {
			msg = "获取上游凭据失败：" + e.Message
		}
		_ = acc.Cooldown(bg, a.ID, serverCooldown, msg)
		return &upstreamFailure{message: msg}
	}
	client, err := acc.HTTPClient(a)
	if err != nil {
		_ = acc.MarkError(bg, a.ID, "上游 HTTP 客户端配置错误："+err.Error())
		return &upstreamFailure{message: "上游代理配置错误"}
	}
	c.upstreamModel = a.UpstreamModel(c.model)
	p, err := c.buildPlan(a, c.upstreamModel)
	if err != nil {
		c.failErr(err)
		return nil
	}
	refreshed := false
	for {
		actx, cancel := context.WithCancel(c.ctx)
		req, err := c.newRequest(actx, a, creds, p)
		if err != nil {
			cancel()
			c.failErr(err)
			return nil
		}
		resp, err := client.Do(req)
		if err != nil {
			cancel()
			if c.ctx.Err() != nil {
				c.clientClosed()
				return nil
			}
			_ = acc.Cooldown(bg, a.ID, networkCooldown, "上游网络错误："+err.Error())
			return &upstreamFailure{message: "上游网络错误"}
		}
		if a.IsCodex() && hasCodexQuota(resp.Header) {
			c.quotas = append(c.quotas, quotaRecord{accountID: a.ID, header: resp.Header.Clone()})
		}
		if resp.StatusCode == http.StatusUnauthorized && a.AuthType == accounts.AuthOAuth && !refreshed {
			drain(resp)
			cancel()
			refreshed = true
			nc, err := acc.RefreshAfterUnauthorized(c.ctx, a, creds.AccessToken)
			if err != nil {
				_ = acc.Cooldown(bg, a.ID, serverCooldown, "上游 401，刷新 token 失败")
				return &upstreamFailure{status: http.StatusUnauthorized, message: "上游鉴权失败"}
			}
			creds = nc
			continue
		}
		var failure *upstreamFailure
		if resp.StatusCode >= 400 {
			failure = c.upstreamStatusError(a, resp)
		} else {
			failure = c.relay(a, p, resp)
		}
		resp.Body.Close()
		cancel()
		return failure
	}
}

func hasCodexQuota(h http.Header) bool {
	for k := range h {
		if strings.HasPrefix(strings.ToLower(k), "x-codex-") {
			return true
		}
	}
	return false
}

func drain(resp *http.Response) {
	_, _ = io.Copy(io.Discard, io.LimitReader(resp.Body, maxErrorBody))
	resp.Body.Close()
}

// upstreamStatusError 处理上游非 2xx：429/5xx 冷却后换号，401 标错后换号，其余 4xx 直接转发给客户端。
func (c *call) upstreamStatusError(a *accounts.Account, resp *http.Response) *upstreamFailure {
	acc := c.s.d.Accounts
	bg := context.WithoutCancel(c.ctx)
	body, _ := io.ReadAll(io.LimitReader(resp.Body, maxErrorBody))
	status := resp.StatusCode
	msg := upstreamMessage(status, body)
	switch {
	case status == http.StatusTooManyRequests:
		d := cooldownFor429(resp.Header, body)
		_ = acc.Cooldown(bg, a.ID, d, "上游 429："+msg)
		return &upstreamFailure{status: status, message: msg, retryAfter: d}
	case status >= 500:
		_ = acc.Cooldown(bg, a.ID, serverCooldown, "上游 "+resp.Status+"："+msg)
		return &upstreamFailure{status: status, message: msg}
	case status == http.StatusUnauthorized:
		_ = acc.MarkError(bg, a.ID, "上游鉴权失败（401）："+msg)
		return &upstreamFailure{status: status, message: "上游鉴权失败"}
	}
	out := status
	code := core.CodeInvalidRequest
	if status == http.StatusForbidden {
		out, code = http.StatusBadGateway, core.CodeUpstreamError
	}
	c.fail(&core.Error{Status: out, Code: code, Message: msg})
	return nil
}

// clientClosed 记录客户端断开（不再写响应）。
func (c *call) clientClosed() {
	c.errCode = "client_closed"
	c.status = statusClientGone
}

// finalize 结算并释放资源（不跟随客户端取消）。
func (c *call) finalize() {
	ctx, cancel := context.WithTimeout(context.WithoutCancel(c.ctx), finalizeTimeout)
	defer cancel()
	status := "ok"
	if c.errCode != "" || c.status >= 400 || c.status == 0 {
		status = "error"
		if c.errCode == "" {
			c.errCode = strings.ToLower(core.CodeUpstreamError)
		}
	}
	rec := &core.UsageRecord{
		RequestID: c.rid, Principal: c.p, AccountID: c.accountID, Model: c.model, UpstreamModel: c.upstreamModel,
		Endpoint: c.endpoint, Stream: c.stream, Usage: c.usage, Status: status, ErrorCode: c.errCode,
		HTTPStatus: c.status, LatencyMs: int(time.Since(c.start).Milliseconds()),
		FirstTokenMs: int(c.firstToken.Milliseconds()), SessionKey: c.sessionKey, IP: c.ip,
	}
	if _, err := c.s.d.Biller.Settle(ctx, rec); err != nil {
		c.log().Error("settle failed", "err", err, "account", c.accountID)
	}
	if c.userSlot != "" {
		_ = c.s.limiter.Release(ctx, c.userSlot, c.rid)
	}
	if c.keySlot != "" {
		_ = c.s.limiter.Release(ctx, c.keySlot, c.rid)
	}
	if err := c.s.limiter.AddTokens(ctx, c.p.UserID, c.usage.Total()); err != nil {
		c.log().Warn("tpm add failed", "err", err)
	}
	if c.accountID > 0 {
		_ = c.s.d.Accounts.TouchUsed(ctx, c.accountID)
	}
	for _, q := range c.quotas {
		if err := c.s.d.Accounts.RecordCodexQuota(ctx, q.accountID, q.header); err != nil {
			c.log().Warn("record codex quota failed", "err", err, "account", q.accountID)
		}
	}
}
