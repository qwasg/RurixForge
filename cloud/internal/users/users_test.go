package users_test

import (
	"bytes"
	"encoding/base64"
	"net/http"
	"strings"
	"testing"

	"forge-cloud/internal/cloudtest"
)

func TestMeAndProfile(t *testing.T) {
	a := cloudtest.New(t)
	s := a.NewUser()

	var me struct {
		User struct {
			ID        int64  `json:"id"`
			Email     string `json:"email"`
			HasAvatar bool   `json:"hasAvatar"`
			GroupID   *int64 `json:"groupId"`
		} `json:"user"`
		Subscriptions []any  `json:"subscriptions"`
		Currency      string `json:"currency"`
	}
	a.Do(http.MethodGet, "/api/v1/me", s.AccessToken, nil).OK(t).Decode(t, &me)
	if me.User.ID != s.UserID || me.User.Email != s.Email || me.User.HasAvatar || me.User.GroupID == nil {
		t.Fatalf("me = %+v", me.User)
	}
	if me.Subscriptions == nil || len(me.Subscriptions) != 0 || me.Currency != "USD" {
		t.Fatalf("subscriptions/currency = %v %q", me.Subscriptions, me.Currency)
	}

	planID := a.CreatePlan("Pro", 30, 5_000_000, 0, 0)
	a.Exec(`INSERT INTO subscriptions (user_id, plan_id, starts_at, ends_at, quota_micros) VALUES ($1, $2, now(), now() + interval '1 day', 5000000)`, s.UserID, planID)
	a.Do(http.MethodGet, "/api/v1/me", s.AccessToken, nil).OK(t).Decode(t, &me)
	if len(me.Subscriptions) != 1 {
		t.Fatalf("生效订阅应出现在 /me: %v", me.Subscriptions)
	}

	u := a.Do(http.MethodPatch, "/api/v1/me/profile", s.AccessToken, map[string]any{"nickname": "  新昵称  "}).OK(t).Map(t)
	if u["nickname"] != "新昵称" {
		t.Fatalf("nickname = %v", u["nickname"])
	}
	a.Do(http.MethodPatch, "/api/v1/me/profile", s.AccessToken, map[string]any{"nickname": strings.Repeat("长", 33)}).
		Expect(t, 400, "INVALID_NICKNAME")
	u = a.Do(http.MethodPatch, "/api/v1/me/profile", s.AccessToken, map[string]any{}).OK(t).Map(t)
	if u["nickname"] != "新昵称" {
		t.Fatal("空补丁不应改动昵称")
	}
}

func TestPasswordChangeRevokesOtherSessions(t *testing.T) {
	a := cloudtest.New(t)
	here := a.Register("pw@example.com", "password-1", "laptop")
	there := a.Login("pw@example.com", "password-1", "desktop")

	change := func(old, next string) *cloudtest.Response {
		return a.Do(http.MethodPost, "/api/v1/me/password", here.AccessToken, map[string]any{"oldPassword": old, "newPassword": next})
	}
	change("wrong-password", "password-2").Expect(t, 400, "INVALID_PASSWORD")
	change("password-1", "short").Expect(t, 400, "WEAK_PASSWORD")
	change("password-1", "password-2").OK(t)

	a.Do(http.MethodGet, "/api/v1/me", there.AccessToken, nil).Expect(t, 401, "UNAUTHORIZED")
	a.Do(http.MethodPost, "/api/v1/auth/refresh", "", map[string]any{"refreshToken": there.RefreshToken}).Expect(t, 401, "REFRESH_INVALID")
	if a.String(`SELECT status FROM api_keys WHERE id = $1`, there.DeviceKeyID) != "revoked" {
		t.Fatal("其它会话的设备 Key 应吊销")
	}
	a.Do(http.MethodGet, "/api/v1/me", here.AccessToken, nil).OK(t)
	if a.String(`SELECT status FROM api_keys WHERE id = $1`, here.DeviceKeyID) != "active" {
		t.Fatal("当前会话的设备 Key 应保留")
	}
	a.Do(http.MethodPost, "/api/v1/auth/refresh", "", map[string]any{"refreshToken": here.RefreshToken}).OK(t)
	a.Do(http.MethodPost, "/api/v1/auth/login", "", map[string]any{"email": "pw@example.com", "password": "password-1"}).
		Expect(t, 401, "INVALID_CREDENTIALS")
	a.Login("pw@example.com", "password-2", "desktop")
}

func dataURL(mime string, b []byte) string {
	return "data:" + mime + ";base64," + base64.StdEncoding.EncodeToString(b)
}

var tinyPNG = append([]byte("\x89PNG\r\n\x1a\n"), bytes.Repeat([]byte{0}, 32)...)

func TestAvatarLifecycle(t *testing.T) {
	a := cloudtest.New(t)
	s := a.NewUser()

	a.Do(http.MethodGet, "/api/v1/me/avatar", s.AccessToken, nil).Expect(t, 404, "AVATAR_NOT_FOUND")

	u := a.Do(http.MethodPut, "/api/v1/me/avatar", s.AccessToken, map[string]any{"dataUrl": dataURL("image/jpeg", tinyPNG)}).OK(t).Map(t)
	if u["hasAvatar"] != true || u["avatarVersion"].(float64) <= 0 {
		t.Fatalf("user = %v", u)
	}
	res := a.Do(http.MethodGet, "/api/v1/me/avatar", s.AccessToken, nil).OK(t)
	if ct := res.Header.Get("Content-Type"); ct != "image/png" {
		t.Fatalf("Content-Type 应按魔数判定为 image/png，得到 %q", ct)
	}
	if !bytes.Equal(res.Body, tinyPNG) {
		t.Fatal("头像内容不一致")
	}

	cases := []struct {
		name    string
		dataURL string
		status  int
		code    string
	}{
		{"not data url", "https://example.com/a.png", 400, "AVATAR_INVALID"},
		{"not base64 flag", "data:image/png," + string(tinyPNG), 400, "AVATAR_INVALID"},
		{"bad base64", "data:image/png;base64,@@@@", 400, "AVATAR_INVALID"},
		{"svg disguised", dataURL("image/png", []byte("<svg xmlns='http://www.w3.org/2000/svg'/>")), 400, "AVATAR_INVALID"},
		{"too large", dataURL("image/png", append(append([]byte{}, tinyPNG...), bytes.Repeat([]byte{1}, 512<<10)...)), 413, "AVATAR_TOO_LARGE"},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			a.Do(http.MethodPut, "/api/v1/me/avatar", s.AccessToken, map[string]any{"dataUrl": tc.dataURL}).Expect(t, tc.status, tc.code)
		})
	}
	for _, img := range []struct {
		mime string
		data []byte
	}{
		{"image/jpeg", []byte{0xFF, 0xD8, 0xFF, 0xE0, 0, 0}},
		{"image/gif", []byte("GIF89a......")},
		{"image/webp", []byte("RIFF\x00\x00\x00\x00WEBPVP8 ")},
	} {
		a.Do(http.MethodPut, "/api/v1/me/avatar", s.AccessToken, map[string]any{"dataUrl": dataURL("application/octet-stream", img.data)}).OK(t)
		if ct := a.Do(http.MethodGet, "/api/v1/me/avatar", s.AccessToken, nil).OK(t).Header.Get("Content-Type"); ct != img.mime {
			t.Fatalf("Content-Type = %q，期望 %q", ct, img.mime)
		}
	}

	u = a.Do(http.MethodDelete, "/api/v1/me/avatar", s.AccessToken, nil).OK(t).Map(t)
	if u["hasAvatar"] != false || u["avatarVersion"].(float64) != 0 {
		t.Fatalf("删除后 user = %v", u)
	}
	a.Do(http.MethodGet, "/api/v1/me/avatar", s.AccessToken, nil).Expect(t, 404, "AVATAR_NOT_FOUND")
}

func TestDevicesListAndRevoke(t *testing.T) {
	a := cloudtest.New(t)
	me := a.Register("dev@example.com", "password-1", "laptop")
	other := a.Login("dev@example.com", "password-1", "desktop")
	stranger := a.NewUser()

	var list struct {
		Items []struct {
			ID         string `json:"id"`
			DeviceID   string `json:"deviceId"`
			DeviceName string `json:"deviceName"`
			Platform   string `json:"platform"`
			AppVersion string `json:"appVersion"`
			IP         string `json:"ip"`
			Current    bool   `json:"current"`
		} `json:"items"`
	}
	a.Do(http.MethodGet, "/api/v1/me/devices", me.AccessToken, nil).OK(t).Decode(t, &list)
	if len(list.Items) != 2 {
		t.Fatalf("设备数 = %d", len(list.Items))
	}
	var otherID string
	current := 0
	for _, d := range list.Items {
		if d.Current {
			current++
			if d.DeviceID != "laptop" {
				t.Fatalf("current 标错了设备: %+v", d)
			}
		} else {
			otherID = d.ID
		}
		if d.Platform != "windows" || d.AppVersion != "0.1.0" || d.IP == "" || !strings.HasPrefix(d.DeviceName, "PC-") {
			t.Fatalf("设备字段缺失: %+v", d)
		}
	}
	if current != 1 || otherID == "" {
		t.Fatalf("current 数 = %d", current)
	}

	strangerSID := a.String(`SELECT id FROM refresh_sessions WHERE user_id = $1`, stranger.UserID)
	a.Do(http.MethodDelete, "/api/v1/me/devices/"+strangerSID, me.AccessToken, nil).Expect(t, 404, "DEVICE_NOT_FOUND")
	a.Do(http.MethodDelete, "/api/v1/me/devices/not-a-session", me.AccessToken, nil).Expect(t, 404, "DEVICE_NOT_FOUND")
	a.Do(http.MethodGet, "/api/v1/me", stranger.AccessToken, nil).OK(t)

	a.Do(http.MethodDelete, "/api/v1/me/devices/"+otherID, me.AccessToken, nil).OK(t)
	a.Do(http.MethodGet, "/api/v1/me", other.AccessToken, nil).Expect(t, 401, "UNAUTHORIZED")
	a.Do(http.MethodPost, "/api/v1/auth/refresh", "", map[string]any{"refreshToken": other.RefreshToken}).Expect(t, 401, "REFRESH_INVALID")
	if a.String(`SELECT status FROM api_keys WHERE id = $1`, other.DeviceKeyID) != "revoked" {
		t.Fatal("吊销设备后其设备 Key 应吊销")
	}
	a.Do(http.MethodGet, "/api/v1/me/devices", me.AccessToken, nil).OK(t).Decode(t, &list)
	if len(list.Items) != 1 || !list.Items[0].Current {
		t.Fatalf("吊销后设备列表 = %+v", list.Items)
	}
}
