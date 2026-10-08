package auth_test

import (
	"context"
	"net/http"
	"regexp"
	"strconv"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/golang-jwt/jwt/v5"

	"forge-cloud/internal/apikeys"
	"forge-cloud/internal/auth"
	"forge-cloud/internal/cloudtest"
	"forge-cloud/internal/config"
	"forge-cloud/internal/core"
)

func register(a *cloudtest.App, body map[string]any) *cloudtest.Response {
	a.T.Helper()
	return a.Do(http.MethodPost, "/api/v1/auth/register", "", body)
}

func login(a *cloudtest.App, email, password, device string) *cloudtest.Response {
	a.T.Helper()
	return a.Do(http.MethodPost, "/api/v1/auth/login", "", map[string]any{
		"email": email, "password": password, "device": cloudtest.Device(device),
	})
}

func refresh(a *cloudtest.App, token string) *cloudtest.Response {
	a.T.Helper()
	return a.Do(http.MethodPost, "/api/v1/auth/refresh", "", map[string]any{"refreshToken": token})
}

func keyStatus(a *cloudtest.App, id int64) string {
	a.T.Helper()
	return a.String(`SELECT status FROM api_keys WHERE id = $1`, id)
}

func TestRegisterIssuesTokensDeviceKeyAndBonus(t *testing.T) {
	a := cloudtest.New(t)
	a.UpdateSettings(`{"signupBonusMicros": 2500000}`)

	res := register(a, map[string]any{
		"email": "  Alice@Example.COM ", "password": "password-1", "nickname": "Alice",
		"device": cloudtest.Device("pc-1"),
	}).OK(t)
	var lr struct {
		AccessToken      string    `json:"accessToken"`
		AccessExpiresAt  time.Time `json:"accessExpiresAt"`
		RefreshToken     string    `json:"refreshToken"`
		RefreshExpiresAt time.Time `json:"refreshExpiresAt"`
		User             struct {
			ID            int64  `json:"id"`
			Email         string `json:"email"`
			Nickname      string `json:"nickname"`
			Role          string `json:"role"`
			GroupName     string `json:"groupName"`
			BalanceMicros int64  `json:"balanceMicros"`
		} `json:"user"`
		DeviceKey *struct {
			ID     int64  `json:"id"`
			Key    string `json:"key"`
			Prefix string `json:"prefix"`
		} `json:"deviceKey"`
	}
	res.Decode(t, &lr)
	if lr.User.Email != "alice@example.com" || lr.User.Nickname != "Alice" || lr.User.Role != "user" || lr.User.GroupName != "default" {
		t.Fatalf("user = %+v", lr.User)
	}
	if lr.User.BalanceMicros != 2_500_000 {
		t.Fatalf("注册赠送未到账: %d", lr.User.BalanceMicros)
	}
	if !strings.HasPrefix(lr.RefreshToken, "rt_") || len(lr.RefreshToken) != 51 {
		t.Fatalf("refresh token 形状不对: %q", lr.RefreshToken)
	}
	if d := time.Until(lr.AccessExpiresAt); d < 14*time.Minute || d > 16*time.Minute {
		t.Fatalf("access 过期时间 = %v", d)
	}
	if d := time.Until(lr.RefreshExpiresAt); d < 29*24*time.Hour || d > 31*24*time.Hour {
		t.Fatalf("refresh 过期时间 = %v", d)
	}
	if lr.DeviceKey == nil || !strings.HasPrefix(lr.DeviceKey.Key, apikeys.KeyPrefix) || len(lr.DeviceKey.Key) != 46 ||
		lr.DeviceKey.Prefix != lr.DeviceKey.Key[:12] {
		t.Fatalf("deviceKey = %+v", lr.DeviceKey)
	}
	if n := a.Int64(`SELECT count(*) FROM api_keys WHERE key_hash = $1 AND kind = 'device' AND session_id IS NOT NULL`,
		core.SHA256Hex(lr.DeviceKey.Key)); n != 1 {
		t.Fatalf("设备 Key 未按哈希落库")
	}
	if n := a.Int64(`SELECT count(*) FROM balance_ledger WHERE user_id = $1 AND kind = 'signup_bonus' AND delta_micros = 2500000 AND balance_after = 2500000`,
		lr.User.ID); n != 1 {
		t.Fatalf("缺少 signup_bonus 流水")
	}
	a.Do(http.MethodGet, "/api/v1/me", lr.AccessToken, nil).OK(t)

	p := a.Principal(lr.DeviceKey.Key)
	if p.UserID != lr.User.ID || p.APIKeyKind != "device" || p.APIKeyName != "PC-pc-1" {
		t.Fatalf("principal = %+v", p)
	}
}

func TestRegisterValidationAndModes(t *testing.T) {
	a := cloudtest.New(t)
	a.Register("taken@example.com", "password-1", "d1")

	cases := []struct {
		name   string
		body   map[string]any
		status int
		code   string
	}{
		{"invalid email", map[string]any{"email": "not-an-email", "password": "password-1"}, 400, "INVALID_EMAIL"},
		{"weak password", map[string]any{"email": "weak@example.com", "password": "short"}, 400, "WEAK_PASSWORD"},
		{"email taken", map[string]any{"email": "TAKEN@example.com", "password": "password-1"}, 409, "EMAIL_TAKEN"},
		{"nickname too long", map[string]any{"email": "nick@example.com", "password": "password-1", "nickname": strings.Repeat("字", 33)}, 400, "INVALID_NICKNAME"},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			register(a, tc.body).Expect(t, tc.status, tc.code)
		})
	}

	a.UpdateSettings(`{"registrationMode": "closed"}`)
	register(a, map[string]any{"email": "closed@example.com", "password": "password-1"}).Expect(t, 403, "REGISTRATION_CLOSED")

	a.UpdateSettings(`{"registrationMode": "invite"}`)
	register(a, map[string]any{"email": "inv@example.com", "password": "password-1"}).Expect(t, 400, "INVITE_CODE_REQUIRED")
	register(a, map[string]any{"email": "inv@example.com", "password": "password-1", "inviteCode": "NOPE"}).Expect(t, 400, "INVITE_CODE_INVALID")
	a.CreateRedeemCode("BAL-CODE", "balance", 1000, 0, 5, nil)
	register(a, map[string]any{"email": "inv@example.com", "password": "password-1", "inviteCode": "BAL-CODE"}).Expect(t, 400, "INVITE_CODE_INVALID")
	past := time.Now().Add(-time.Hour)
	a.CreateRedeemCode("INV-OLD", "invite", 0, 0, 5, &past)
	register(a, map[string]any{"email": "inv@example.com", "password": "password-1", "inviteCode": "INV-OLD"}).Expect(t, 400, "INVITE_CODE_INVALID")

	codeID := a.CreateRedeemCode("INV-1", "invite", 0, 0, 1, nil)
	res := register(a, map[string]any{"email": "inv@example.com", "password": "password-1", "inviteCode": "INV-1"}).OK(t)
	uid := int64(res.Map(t)["user"].(map[string]any)["id"].(float64))
	if a.Int64(`SELECT used_count FROM redeem_codes WHERE id = $1`, codeID) != 1 {
		t.Fatal("邀请码未消费")
	}
	if a.Int64(`SELECT count(*) FROM redeem_records WHERE code_id = $1 AND user_id = $2`, codeID, uid) != 1 {
		t.Fatal("缺少 redeem_records")
	}
	register(a, map[string]any{"email": "inv2@example.com", "password": "password-1", "inviteCode": "INV-1"}).Expect(t, 400, "INVITE_CODE_INVALID")
	if a.Int64(`SELECT count(*) FROM users WHERE email = 'inv2@example.com'`) != 0 {
		t.Fatal("失败的注册不应留下用户")
	}
}

func TestLoginErrorsRateLimitAndDisabled(t *testing.T) {
	a := cloudtest.New(t)
	s := a.Register("bob@example.com", "password-1", "d1")

	login(a, "bob@example.com", "wrong-password", "d1").Expect(t, 401, "INVALID_CREDENTIALS")
	login(a, "nobody@example.com", "password-1", "d1").Expect(t, 401, "INVALID_CREDENTIALS")
	login(a, "BOB@example.com", "password-1", "d2").OK(t)
	if a.Int64(`SELECT count(*) FROM users WHERE id = $1 AND last_login_at IS NOT NULL`, s.UserID) != 1 {
		t.Fatal("last_login_at 未更新")
	}

	for i := 0; i < 10; i++ {
		login(a, "victim@example.com", "guess-guess", "d1").Expect(t, 401, "INVALID_CREDENTIALS")
	}
	res := login(a, "victim@example.com", "guess-guess", "d1").Expect(t, 429, "TOO_MANY_ATTEMPTS")
	if res.Header.Get("Retry-After") == "" {
		t.Fatal("429 缺少 Retry-After")
	}
	// 计数按 IP+邮箱隔离。
	login(a, "bob@example.com", "password-1", "d3").OK(t)

	a.Exec(`UPDATE users SET status = 'disabled' WHERE id = $1`, s.UserID)
	login(a, "bob@example.com", "password-1", "d1").Expect(t, 403, "USER_DISABLED")
	a.Do(http.MethodGet, "/api/v1/me", s.AccessToken, nil).Expect(t, 403, "USER_DISABLED")
	if _, err := a.APIKeys.AuthenticateAPIKey(context.Background(), s.DeviceKey); core.AsError(err) == nil || core.AsError(err).Code != core.CodeUserDisabled {
		t.Fatalf("禁用用户的 Key 应 403，得到 %v", err)
	}
}

func TestReloginSameDeviceRevokesOldSessionAndKey(t *testing.T) {
	a := cloudtest.New(t)
	first := a.Register("carol@example.com", "password-1", "laptop")
	other := a.Login("carol@example.com", "password-1", "desktop")
	second := a.Login("carol@example.com", "password-1", "laptop")

	refresh(a, first.RefreshToken).Expect(t, 401, "REFRESH_INVALID")
	a.Do(http.MethodGet, "/api/v1/me", first.AccessToken, nil).Expect(t, 401, "UNAUTHORIZED")
	if keyStatus(a, first.DeviceKeyID) != "revoked" {
		t.Fatal("同设备重新登录后旧设备 Key 应吊销")
	}
	if _, err := a.APIKeys.AuthenticateAPIKey(context.Background(), first.DeviceKey); core.AsError(err) == nil || core.AsError(err).Code != core.CodeInvalidAPIKey {
		t.Fatalf("旧设备 Key 应 401，得到 %v", err)
	}
	a.Principal(second.DeviceKey)
	a.Principal(other.DeviceKey)
	a.Do(http.MethodGet, "/api/v1/me", other.AccessToken, nil).OK(t)

	res := a.Do(http.MethodPost, "/api/v1/auth/login", "", map[string]any{
		"email": "carol@example.com", "password": "password-1", "device": cloudtest.Device("console"), "issueDeviceKey": false,
	}).OK(t)
	if dk, ok := res.Map(t)["deviceKey"]; !ok || dk != nil {
		t.Fatalf("issueDeviceKey=false 时 deviceKey 应为 null: %s", res.Body)
	}
}

func TestRefreshRotationAndReuseDetection(t *testing.T) {
	a := cloudtest.New(t)
	s := a.Register("dave@example.com", "password-1", "pc")

	var p1 struct {
		AccessToken  string `json:"accessToken"`
		RefreshToken string `json:"refreshToken"`
	}
	refresh(a, s.RefreshToken).OK(t).Decode(t, &p1)
	if p1.RefreshToken == s.RefreshToken || !strings.HasPrefix(p1.RefreshToken, "rt_") {
		t.Fatalf("refresh token 未轮换: %q", p1.RefreshToken)
	}
	a.Do(http.MethodGet, "/api/v1/me", p1.AccessToken, nil).OK(t)

	var p2 struct {
		RefreshToken string `json:"refreshToken"`
	}
	refresh(a, p1.RefreshToken).OK(t).Decode(t, &p2)

	// p1 已被轮换掉：再次出示即复用 → 吊销整个会话与设备 Key。
	refresh(a, p1.RefreshToken).Expect(t, 401, "REFRESH_REUSED")
	refresh(a, p2.RefreshToken).Expect(t, 401, "REFRESH_INVALID")
	a.Do(http.MethodGet, "/api/v1/me", p1.AccessToken, nil).Expect(t, 401, "UNAUTHORIZED")
	if keyStatus(a, s.DeviceKeyID) != "revoked" {
		t.Fatal("复用检测后设备 Key 应吊销")
	}

	refresh(a, "rt_unknown").Expect(t, 401, "REFRESH_INVALID")
	refresh(a, "garbage").Expect(t, 401, "REFRESH_INVALID")

	s2 := a.Login("dave@example.com", "password-1", "pc2")
	a.Exec(`UPDATE refresh_sessions SET expires_at = now() - interval '1 minute' WHERE token_hash = $1`, core.SHA256Hex(s2.RefreshToken))
	refresh(a, s2.RefreshToken).Expect(t, 401, "REFRESH_INVALID")
	a.Do(http.MethodGet, "/api/v1/me", s2.AccessToken, nil).Expect(t, 401, "UNAUTHORIZED")
	if _, err := a.APIKeys.AuthenticateAPIKey(context.Background(), s2.DeviceKey); core.AsError(err) == nil {
		t.Fatal("会话过期后设备 Key 应失效")
	}
}

func TestLogoutRevokesSessionAndDeviceKey(t *testing.T) {
	a := cloudtest.New(t)
	s := a.Register("erin@example.com", "password-1", "pc")
	keep := a.Login("erin@example.com", "password-1", "phone")

	a.Do(http.MethodPost, "/api/v1/auth/logout", s.AccessToken, nil).OK(t)
	a.Do(http.MethodGet, "/api/v1/me", s.AccessToken, nil).Expect(t, 401, "UNAUTHORIZED")
	refresh(a, s.RefreshToken).Expect(t, 401, "REFRESH_INVALID")
	if keyStatus(a, s.DeviceKeyID) != "revoked" {
		t.Fatal("登出后设备 Key 应吊销")
	}
	a.Do(http.MethodGet, "/api/v1/me", keep.AccessToken, nil).OK(t)
	if keyStatus(a, keep.DeviceKeyID) != "active" {
		t.Fatal("其它设备不应受影响")
	}
}

func TestRequireUserAndAdminRejections(t *testing.T) {
	a := cloudtest.New(t)
	s := a.Register("frank@example.com", "password-1", "pc")
	sid := a.String(`SELECT id FROM refresh_sessions WHERE user_id = $1`, s.UserID)

	sign := func(secret []byte, iss string, exp time.Time) string {
		c := jwt.MapClaims{
			"sub": strconv.FormatInt(s.UserID, 10), "sid": sid, "role": "admin", "iss": iss,
			"iat": time.Now().Unix(), "exp": exp.Unix(),
		}
		tok, err := jwt.NewWithClaims(jwt.SigningMethodHS256, c).SignedString(secret)
		if err != nil {
			t.Fatal(err)
		}
		return tok
	}
	secret := a.Config.JWTSecret
	cases := []struct {
		name   string
		header string
	}{
		{"missing", ""},
		{"not bearer", "Basic abc"},
		{"garbage", "Bearer not-a-jwt"},
		{"wrong secret", "Bearer " + sign([]byte("another-secret-value"), "forge-cloud", time.Now().Add(time.Minute))},
		{"wrong issuer", "Bearer " + sign(secret, "evil", time.Now().Add(time.Minute))},
		{"expired", "Bearer " + sign(secret, "forge-cloud", time.Now().Add(-time.Minute))},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			req, _ := http.NewRequest(http.MethodGet, "/api/v1/me", nil)
			if tc.header != "" {
				req.Header.Set("Authorization", tc.header)
			}
			a.Serve(req).Expect(t, 401, "UNAUTHORIZED")
		})
	}

	// 正确签名的 token：role 以库内为准，普通用户进不了管理接口。
	forged := sign(secret, "forge-cloud", time.Now().Add(time.Minute))
	a.Do(http.MethodGet, "/api/v1/me", forged, nil).OK(t)
	a.Do(http.MethodGet, "/api/admin/dashboard", forged, nil).Expect(t, 403, "FORBIDDEN")
	a.Do(http.MethodGet, "/api/admin/dashboard", "", nil).Expect(t, 401, "UNAUTHORIZED")
}

func TestAuthConfig(t *testing.T) {
	a := cloudtest.New(t)
	m := a.Do(http.MethodGet, "/api/v1/auth/config", "", nil).OK(t).Map(t)
	if m["registrationMode"] != "open" || m["requireEmailVerify"] != false || m["smtpEnabled"] != false ||
		m["siteName"] != "RurixForge Cloud" || m["currency"] != "USD" {
		t.Fatalf("auth config = %v", m)
	}
	a.UpdateSettings(`{"registrationMode": "invite", "requireEmailVerify": true}`)
	m = a.Do(http.MethodGet, "/api/v1/auth/config", "", nil).OK(t).Map(t)
	if m["registrationMode"] != "invite" || m["requireEmailVerify"] != false {
		t.Fatalf("未配置 SMTP 时 requireEmailVerify 应为 false: %v", m)
	}
}

type fakeMailer struct {
	mu   sync.Mutex
	sent []string
}

func (f *fakeMailer) send(_ context.Context, to, subject, body string) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.sent = append(f.sent, to+"|"+body)
	return nil
}

var sixDigits = regexp.MustCompile(`\d{6}`)

func (f *fakeMailer) lastCode(t *testing.T) string {
	t.Helper()
	f.mu.Lock()
	defer f.mu.Unlock()
	if len(f.sent) == 0 {
		t.Fatal("没有发出邮件")
	}
	code := sixDigits.FindString(f.sent[len(f.sent)-1])
	if code == "" {
		t.Fatalf("邮件里没有验证码: %s", f.sent[len(f.sent)-1])
	}
	return code
}

func withSMTP(c *config.Config) {
	c.SMTP = config.SMTPConfig{Host: "smtp.invalid", Port: 587, From: "noreply@example.com"}
}

func TestEmailCodeRegisterAndPasswordReset(t *testing.T) {
	a := cloudtest.New(t, withSMTP)
	mail := &fakeMailer{}
	a.Auth.SetMailer(mail.send)
	a.UpdateSettings(`{"requireEmailVerify": true}`)

	emailCode := func(email, purpose string) *cloudtest.Response {
		return a.Do(http.MethodPost, "/api/v1/auth/email-code", "", map[string]any{"email": email, "purpose": purpose})
	}
	emailCode("new@example.com", "bogus").Expect(t, 400, "INVALID_PURPOSE")
	emailCode("new@example.com", "register").OK(t)
	code := mail.lastCode(t)
	emailCode("new@example.com", "register").Expect(t, 429, "TOO_MANY_ATTEMPTS")

	body := map[string]any{"email": "new@example.com", "password": "password-1", "device": cloudtest.Device("pc")}
	register(a, body).Expect(t, 400, "EMAIL_CODE_REQUIRED")
	wrong := "000000"
	if code == wrong {
		wrong = "111111"
	}
	body["emailCode"] = wrong
	register(a, body).Expect(t, 400, "EMAIL_CODE_INVALID")
	if a.Int64(`SELECT attempts FROM email_codes WHERE email = 'new@example.com'`) != 1 {
		t.Fatal("错误验证码应累加尝试次数")
	}
	body["emailCode"] = code
	s := toSessionFromResponse(t, register(a, body).OK(t))
	if a.Int64(`SELECT count(*) FROM users WHERE id = $1 AND email_verified`, s) != 1 {
		t.Fatal("验证后 email_verified 应为 true")
	}
	if a.Int64(`SELECT count(*) FROM email_codes WHERE email = 'new@example.com'`) != 0 {
		t.Fatal("验证码应在注册后消费")
	}

	// 注册目的：邮箱已存在 → 409；重置目的：邮箱不存在 → 静默成功且不发信。
	a.MR.FlushAll()
	emailCode("new@example.com", "register").Expect(t, 409, "EMAIL_TAKEN")
	sentBefore := len(mail.sent)
	emailCode("ghost@example.com", "reset").OK(t)
	if len(mail.sent) != sentBefore {
		t.Fatal("不存在的邮箱不应发信")
	}

	other := a.Login("new@example.com", "password-1", "phone")
	a.MR.FlushAll()
	emailCode("new@example.com", "reset").OK(t)
	resetCode := mail.lastCode(t)
	reset := func(code, pw string) *cloudtest.Response {
		return a.Do(http.MethodPost, "/api/v1/auth/password/reset", "", map[string]any{
			"email": "new@example.com", "code": code, "newPassword": pw,
		})
	}
	reset(resetCode, "short").Expect(t, 400, "WEAK_PASSWORD")
	reset("999999x", "password-2").Expect(t, 400, "EMAIL_CODE_INVALID")
	reset(resetCode, "password-2").OK(t)
	reset(resetCode, "password-3").Expect(t, 400, "EMAIL_CODE_INVALID")
	a.Do(http.MethodGet, "/api/v1/me", other.AccessToken, nil).Expect(t, 401, "UNAUTHORIZED")
	login(a, "new@example.com", "password-1", "pc").Expect(t, 401, "INVALID_CREDENTIALS")
	a.Login("new@example.com", "password-2", "pc")
}

func TestEmailCodeAttemptsExhausted(t *testing.T) {
	a := cloudtest.New(t, withSMTP)
	mail := &fakeMailer{}
	a.Auth.SetMailer(mail.send)
	a.UpdateSettings(`{"requireEmailVerify": true}`)
	a.Do(http.MethodPost, "/api/v1/auth/email-code", "", map[string]any{"email": "x@example.com", "purpose": "register"}).OK(t)
	code := mail.lastCode(t)
	wrong := "000000"
	if code == wrong {
		wrong = "111111"
	}
	body := map[string]any{"email": "x@example.com", "password": "password-1", "emailCode": wrong}
	for i := 0; i < 5; i++ {
		register(a, body).Expect(t, 400, "EMAIL_CODE_INVALID")
	}
	body["emailCode"] = code
	register(a, body).Expect(t, 400, "EMAIL_CODE_INVALID")
}

func TestEmailEndpointsWithoutSMTP(t *testing.T) {
	a := cloudtest.New(t)
	a.Do(http.MethodPost, "/api/v1/auth/email-code", "", map[string]any{"email": "a@example.com", "purpose": "register"}).
		Expect(t, 501, "SMTP_NOT_CONFIGURED")
	a.Do(http.MethodPost, "/api/v1/auth/password/reset", "", map[string]any{"email": "a@example.com", "code": "123456", "newPassword": "password-9"}).
		Expect(t, 501, "SMTP_NOT_CONFIGURED")
}

func toSessionFromResponse(t *testing.T, res *cloudtest.Response) int64 {
	t.Helper()
	return int64(res.Map(t)["user"].(map[string]any)["id"].(float64))
}

func TestBootstrapAdmin(t *testing.T) {
	ctx := context.Background()
	t.Run("creates when none exists", func(t *testing.T) {
		a := cloudtest.New(t, func(c *config.Config) {
			c.AdminEmail, c.AdminPassword = "root@example.com", "root-password"
		})
		if err := a.Auth.EnsureBootstrapAdmin(ctx); err != nil {
			t.Fatal(err)
		}
		if err := a.Auth.EnsureBootstrapAdmin(ctx); err != nil {
			t.Fatal(err)
		}
		if a.Int64(`SELECT count(*) FROM users WHERE role = 'admin'`) != 1 {
			t.Fatal("应恰好有一个管理员")
		}
		s := a.Login("root@example.com", "root-password", "console")
		a.Do(http.MethodGet, "/api/admin/dashboard", s.AccessToken, nil).OK(t)
	})
	t.Run("promotes existing email", func(t *testing.T) {
		a := cloudtest.New(t, func(c *config.Config) {
			c.AdminEmail, c.AdminPassword = "boss@example.com", "boss-password"
		})
		old := a.Register("boss@example.com", "user-password", "pc")
		if err := a.Auth.EnsureBootstrapAdmin(ctx); err != nil {
			t.Fatal(err)
		}
		if a.String(`SELECT role FROM users WHERE id = $1`, old.UserID) != "admin" {
			t.Fatal("已有邮箱应被提升为管理员")
		}
		a.Do(http.MethodGet, "/api/v1/me", old.AccessToken, nil).Expect(t, 401, "UNAUTHORIZED")
		a.Login("boss@example.com", "boss-password", "console")
	})
	t.Run("noop when admin exists or unset", func(t *testing.T) {
		a := cloudtest.New(t, func(c *config.Config) {
			c.AdminEmail, c.AdminPassword = "late@example.com", "late-password"
		})
		a.NewAdmin()
		if err := a.Auth.EnsureBootstrapAdmin(ctx); err != nil {
			t.Fatal(err)
		}
		if a.Int64(`SELECT count(*) FROM users WHERE email = 'late@example.com'`) != 0 {
			t.Fatal("已有管理员时不应再创建")
		}
		b := cloudtest.New(t)
		if err := b.Auth.EnsureBootstrapAdmin(ctx); err != nil {
			t.Fatal(err)
		}
		if b.Int64(`SELECT count(*) FROM users`) != 0 {
			t.Fatal("未配置时不应创建")
		}
	})
	t.Run("create admin validates input", func(t *testing.T) {
		a := cloudtest.New(t)
		if err := a.Auth.CreateAdmin(ctx, "bad", "password-1"); core.AsError(err) == nil || core.AsError(err).Code != "INVALID_EMAIL" {
			t.Fatalf("得到 %v", err)
		}
		if err := a.Auth.CreateAdmin(ctx, "ok@example.com", "short"); core.AsError(err) == nil || core.AsError(err).Code != "WEAK_PASSWORD" {
			t.Fatalf("得到 %v", err)
		}
	})
}

func TestPasswordHashing(t *testing.T) {
	h, err := auth.HashPassword("correct horse")
	if err != nil {
		t.Fatal(err)
	}
	if !strings.HasPrefix(h, "$argon2id$v=19$m=65536,t=3,p=2$") {
		t.Fatalf("PHC 串格式不对: %s", h)
	}
	if !auth.VerifyPassword("correct horse", h) || auth.VerifyPassword("wrong horse", h) {
		t.Fatal("校验结果不对")
	}
	h2, _ := auth.HashPassword("correct horse")
	if h == h2 {
		t.Fatal("盐应随机")
	}
	for _, bad := range []string{"", "$argon2id$v=19$m=65536,t=3,p=2$zz", "$bcrypt$x"} {
		if auth.VerifyPassword("x", bad) {
			t.Fatalf("非法哈希不应通过: %q", bad)
		}
	}
}
