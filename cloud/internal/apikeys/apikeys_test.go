package apikeys_test

import (
	"context"
	"net/http"
	"strconv"
	"strings"
	"testing"
	"time"

	"forge-cloud/internal/apikeys"
	"forge-cloud/internal/auth"
	"forge-cloud/internal/billing"
	"forge-cloud/internal/cloudtest"
	"forge-cloud/internal/core"
)

type apiKey struct {
	ID          int64      `json:"id"`
	Name        string     `json:"name"`
	Kind        string     `json:"kind"`
	Prefix      string     `json:"prefix"`
	Status      string     `json:"status"`
	QuotaMicros int64      `json:"quotaMicros"`
	UsedMicros  int64      `json:"usedMicros"`
	ExpiresAt   *time.Time `json:"expiresAt"`
	DeviceName  string     `json:"deviceName"`
}

func createKey(t *testing.T, a *cloudtest.App, token string, body map[string]any) (apiKey, string) {
	t.Helper()
	var out struct {
		APIKey apiKey `json:"apiKey"`
		Key    string `json:"key"`
	}
	a.Do(http.MethodPost, "/api/v1/me/api-keys", token, body).OK(t).Decode(t, &out)
	return out.APIKey, out.Key
}

func listKeys(t *testing.T, a *cloudtest.App, token string) []apiKey {
	t.Helper()
	var out struct {
		Items []apiKey `json:"items"`
	}
	a.Do(http.MethodGet, "/api/v1/me/api-keys", token, nil).OK(t).Decode(t, &out)
	return out.Items
}

func authErr(a *cloudtest.App, raw string) *core.Error {
	_, err := a.APIKeys.AuthenticateAPIKey(context.Background(), raw)
	return core.AsError(err)
}

func TestGenerateKeyShape(t *testing.T) {
	raw, hash, prefix := apikeys.GenerateKey()
	if !strings.HasPrefix(raw, "sk-rf-") || len(raw) != 46 || prefix != raw[:12] || hash != core.SHA256Hex(raw) {
		t.Fatalf("raw=%q prefix=%q", raw, prefix)
	}
	raw2, _, _ := apikeys.GenerateKey()
	if raw == raw2 {
		t.Fatal("Key 应随机")
	}
}

func TestAPIKeyCRUDAndAuthentication(t *testing.T) {
	a := cloudtest.New(t)
	s := a.NewUser()
	other := a.NewUser()

	key, raw := createKey(t, a, s.AccessToken, map[string]any{"name": "codex-cli", "quotaMicros": 5_000_000})
	if key.Kind != "user" || key.Status != "active" || key.QuotaMicros != 5_000_000 || key.Prefix != raw[:12] || key.DeviceName != "" {
		t.Fatalf("apiKey = %+v", key)
	}
	if !strings.HasPrefix(raw, apikeys.KeyPrefix) || len(raw) != 46 {
		t.Fatalf("key = %q", raw)
	}

	items := listKeys(t, a, s.AccessToken)
	if len(items) != 2 {
		t.Fatalf("应包含自建 Key 与设备 Key: %+v", items)
	}
	var sawDevice bool
	for _, k := range items {
		if k.Kind == "device" {
			sawDevice = true
			if k.DeviceName == "" {
				t.Fatalf("设备 Key 缺 deviceName: %+v", k)
			}
		}
	}
	if !sawDevice {
		t.Fatal("列表缺设备 Key")
	}

	p := a.Principal(raw)
	if p.UserID != s.UserID || p.APIKeyID != key.ID || p.APIKeyName != "codex-cli" || p.APIKeyKind != "user" ||
		p.KeyQuota != 5_000_000 || p.Email != s.Email || p.Status != "active" || p.Group.Name != "default" {
		t.Fatalf("principal = %+v", p)
	}
	for _, bad := range []string{"", "sk-other-123", "sk-rf-doesnotexist", "Bearer " + raw} {
		if e := authErr(a, bad); e == nil || e.Status != 401 || e.Code != core.CodeInvalidAPIKey {
			t.Fatalf("%q 应 401 INVALID_API_KEY，得到 %v", bad, e)
		}
	}

	path := "/api/v1/me/api-keys/" + strconv.FormatInt(key.ID, 10)
	a.Do(http.MethodDelete, path, other.AccessToken, nil).Expect(t, 404, "API_KEY_NOT_FOUND")
	a.Do(http.MethodDelete, path, s.AccessToken, nil).OK(t)
	a.Do(http.MethodDelete, path, s.AccessToken, nil).OK(t)
	a.Do(http.MethodDelete, "/api/v1/me/api-keys/999999", s.AccessToken, nil).Expect(t, 404, "API_KEY_NOT_FOUND")
	if e := authErr(a, raw); e == nil || e.Code != core.CodeInvalidAPIKey {
		t.Fatalf("吊销后应 401，得到 %v", e)
	}
	if len(listKeys(t, a, s.AccessToken)) != 1 {
		t.Fatal("吊销的 Key 不应再出现在列表")
	}
}

func TestAPIKeyValidationAndLimit(t *testing.T) {
	a := cloudtest.New(t)
	s := a.NewUser()

	a.Do(http.MethodPost, "/api/v1/me/api-keys", s.AccessToken, map[string]any{"quotaMicros": -1}).Expect(t, 400, "INVALID_REQUEST")
	a.Do(http.MethodPost, "/api/v1/me/api-keys", s.AccessToken, map[string]any{"name": strings.Repeat("n", 65)}).Expect(t, 400, "INVALID_REQUEST")
	a.Do(http.MethodPost, "/api/v1/me/api-keys", s.AccessToken, map[string]any{"expiresAt": time.Now().Add(-time.Hour)}).Expect(t, 400, "INVALID_REQUEST")
	k, _ := createKey(t, a, s.AccessToken, map[string]any{})
	if k.Name != "API Key" {
		t.Fatalf("缺省名称 = %q", k.Name)
	}

	for i := 1; i < apikeys.MaxUserKeys; i++ {
		createKey(t, a, s.AccessToken, map[string]any{"name": "k" + strconv.Itoa(i)})
	}
	a.Do(http.MethodPost, "/api/v1/me/api-keys", s.AccessToken, map[string]any{"name": "overflow"}).Expect(t, 400, "API_KEY_LIMIT")
	// 设备 Key 不占自建 Key 名额；过期的自建 Key 也不占。
	a.Exec(`UPDATE api_keys SET expires_at = now() - interval '1 second' WHERE id = $1`, k.ID)
	createKey(t, a, s.AccessToken, map[string]any{"name": "after-expiry"})
	a.Do(http.MethodPost, "/api/v1/me/api-keys", s.AccessToken, map[string]any{"name": "overflow"}).Expect(t, 400, "API_KEY_LIMIT")
}

func TestExpiredDisabledAndSessionBoundKeys(t *testing.T) {
	a := cloudtest.New(t)
	s := a.NewUser()
	future := time.Now().Add(time.Hour)
	key, raw := createKey(t, a, s.AccessToken, map[string]any{"name": "temp", "expiresAt": future})
	if key.ExpiresAt == nil || key.ExpiresAt.Unix() != future.Unix() {
		t.Fatalf("expiresAt = %v", key.ExpiresAt)
	}
	a.Principal(raw)
	a.Exec(`UPDATE api_keys SET expires_at = now() - interval '1 second' WHERE id = $1`, key.ID)
	if e := authErr(a, raw); e == nil || e.Code != core.CodeInvalidAPIKey {
		t.Fatalf("过期 Key 应 401，得到 %v", e)
	}

	// 设备 Key 随会话失效。
	a.Principal(s.DeviceKey)
	sid := a.String(`SELECT session_id FROM api_keys WHERE id = $1`, s.DeviceKeyID)
	a.Exec(`UPDATE refresh_sessions SET revoked_at = now() WHERE id = $1`, sid)
	if e := authErr(a, s.DeviceKey); e == nil || e.Code != core.CodeInvalidAPIKey {
		t.Fatalf("会话吊销后设备 Key 应 401，得到 %v", e)
	}

	// 吊销的是当前登录会话，需重新登录后再测禁用用户下的 Key。
	s = a.Login(s.Email, cloudtest.DefaultPassword, s.DeviceID)
	_, live := createKey(t, a, s.AccessToken, map[string]any{"name": "live"})
	a.Exec(`UPDATE users SET status = 'disabled' WHERE id = $1`, s.UserID)
	if e := authErr(a, live); e == nil || e.Status != 403 || e.Code != core.CodeUserDisabled {
		t.Fatalf("禁用用户应 403 USER_DISABLED，得到 %v", e)
	}
	if _, err := a.APIKeys.PrincipalForUser(context.Background(), s.UserID); core.AsError(err) == nil || core.AsError(err).Code != core.CodeUserDisabled {
		t.Fatalf("PrincipalForUser 禁用用户应 403，得到 %v", err)
	}
	if _, err := a.APIKeys.PrincipalForUser(context.Background(), 987654); core.AsError(err) == nil || core.AsError(err).Status != 401 {
		t.Fatalf("不存在的用户应 401，得到 %v", err)
	}
}

func TestEffectiveGroupResolution(t *testing.T) {
	a := cloudtest.New(t)
	ctx := context.Background()
	s := a.NewUser()
	_, raw := createKey(t, a, s.AccessToken, map[string]any{"name": "g"})
	defaultID := a.Int64(`SELECT id FROM groups WHERE is_default`)

	p := a.Principal(raw)
	if p.Group.ID != defaultID || p.Group.RateMultiplier != 1 || p.UserConcurrency != 0 {
		t.Fatalf("初始应为默认分组: %+v", p.Group)
	}

	vip := a.CreateGroup("vip", 0.5, 7)
	a.Exec(`UPDATE groups SET allowed_models = '{gpt-5.5}' WHERE id = $1`, vip)
	vipPlan := a.CreatePlan("VIP", 30, 10_000_000, 0, vip)
	if _, err := billing.CreateSubscription(ctx, a.DB, s.UserID, vipPlan, 0); err != nil {
		t.Fatal(err)
	}
	p = a.Principal(raw)
	if p.Group.ID != vip || p.Group.RateMultiplier != 0.5 || p.UserConcurrency != 7 || !p.Group.AllowsModel("gpt-5.5") || p.Group.AllowsModel("other") {
		t.Fatalf("订阅分组应覆盖用户分组: %+v conc=%d", p.Group, p.UserConcurrency)
	}

	// 多个生效订阅：取最晚到期的那个的分组。
	ultra := a.CreateGroup("ultra", 0.25, 9)
	ultraPlan := a.CreatePlan("Ultra", 90, 10_000_000, 0, ultra)
	if _, err := billing.CreateSubscription(ctx, a.DB, s.UserID, ultraPlan, 0); err != nil {
		t.Fatal(err)
	}
	if p = a.Principal(raw); p.Group.ID != ultra {
		t.Fatalf("应取最晚到期订阅的分组，得到 %+v", p.Group)
	}

	a.Exec(`UPDATE users SET concurrency_override = 3 WHERE id = $1`, s.UserID)
	if p = a.Principal(raw); p.UserConcurrency != 3 {
		t.Fatalf("concurrency_override 应优先: %d", p.UserConcurrency)
	}

	jp, err := a.APIKeys.PrincipalForUser(ctx, s.UserID)
	if err != nil {
		t.Fatal(err)
	}
	if jp.APIKeyID != 0 || jp.Group.ID != ultra || jp.UserConcurrency != 3 {
		t.Fatalf("PrincipalForUser = %+v", jp)
	}

	a.Exec(`UPDATE subscriptions SET ends_at = now() - interval '1 second' WHERE user_id = $1`, s.UserID)
	if p = a.Principal(raw); p.Group.ID != defaultID {
		t.Fatalf("订阅过期后应回到用户分组，得到 %+v", p.Group)
	}

	mine := a.CreateGroup("mine", 2, 1)
	a.Exec(`UPDATE users SET group_id = $2 WHERE id = $1`, s.UserID, mine)
	if p = a.Principal(raw); p.Group.ID != mine || p.Group.RateMultiplier != 2 {
		t.Fatalf("应为用户分组: %+v", p.Group)
	}

	a.Exec(`UPDATE users SET group_id = NULL WHERE id = $1`, s.UserID)
	if p = a.Principal(raw); p.Group.ID != defaultID {
		t.Fatalf("无分组时应回落默认分组: %+v", p.Group)
	}
	a.UpdateSettings(`{"defaultGroupId": ` + strconv.FormatInt(vip, 10) + `}`)
	if p = a.Principal(raw); p.Group.ID != vip {
		t.Fatalf("系统设置指定的默认分组应优先于 is_default: %+v", p.Group)
	}
	a.UpdateSettings(`{"defaultGroupId": 424242}`)
	if p = a.Principal(raw); p.Group.ID != defaultID {
		t.Fatalf("设置指向不存在的分组时应回落 is_default: %+v", p.Group)
	}
}

func TestLastUsedAtThrottled(t *testing.T) {
	a := cloudtest.New(t)
	s := a.NewUser()
	key, raw := createKey(t, a, s.AccessToken, map[string]any{"name": "t"})

	a.Principal(raw)
	if a.Int64(`SELECT count(*) FROM api_keys WHERE id = $1 AND last_used_at IS NOT NULL`, key.ID) != 1 {
		t.Fatal("首次使用应写 last_used_at")
	}
	a.Exec(`UPDATE api_keys SET last_used_at = NULL WHERE id = $1`, key.ID)
	a.Principal(raw)
	if a.Int64(`SELECT count(*) FROM api_keys WHERE id = $1 AND last_used_at IS NULL`, key.ID) != 1 {
		t.Fatal("一分钟内不应重复写 last_used_at")
	}
}

func TestInsertDeviceKeyAndListKeys(t *testing.T) {
	a := cloudtest.New(t)
	ctx := context.Background()
	s := a.NewUser()
	sid := a.String(`SELECT id FROM refresh_sessions WHERE user_id = $1`, s.UserID)
	id, raw, prefix, err := apikeys.InsertDeviceKey(ctx, a.DB, s.UserID, sid, "Workstation")
	if err != nil {
		t.Fatal(err)
	}
	if prefix != raw[:12] || id == 0 {
		t.Fatalf("id=%d prefix=%q", id, prefix)
	}
	if _, err := auth.RevokeSession(ctx, a.DB, s.UserID, sid); err != nil {
		t.Fatal(err)
	}
	active, err := apikeys.ListKeys(ctx, a.DB, s.UserID, false)
	if err != nil {
		t.Fatal(err)
	}
	all, err := apikeys.ListKeys(ctx, a.DB, s.UserID, true)
	if err != nil {
		t.Fatal(err)
	}
	if len(active) != 0 || len(all) != 2 {
		t.Fatalf("active=%d all=%d", len(active), len(all))
	}
}
