package admin_test

import (
	"net/http"
	"strconv"
	"testing"

	"forge-cloud/internal/cloudtest"
)

func TestDashboardShape(t *testing.T) {
	a := cloudtest.New(t)
	admin := a.NewAdmin()
	m := a.Do(http.MethodGet, "/api/admin/dashboard", admin.AccessToken, nil).OK(t).Map(t)

	for _, key := range []string{"users", "accounts", "today", "daily", "topModels", "currency"} {
		if _, ok := m[key]; !ok {
			t.Fatalf("dashboard 缺少 %q: %v", key, m)
		}
	}
	users := m["users"].(map[string]any)
	if _, ok := users["total"]; !ok || users["active7d"] == nil {
		t.Fatalf("users 形状不对: %v", users)
	}
	if m["currency"] != "USD" {
		t.Fatalf("currency = %v", m["currency"])
	}
}

func TestGroupsCRUD(t *testing.T) {
	a := cloudtest.New(t)
	admin := a.NewAdmin()

	list := a.Do(http.MethodGet, "/api/admin/groups", admin.AccessToken, nil).OK(t).Map(t)
	items := list["items"].([]any)
	if len(items) < 1 {
		t.Fatal("应至少有默认分组")
	}

	created := a.Do(http.MethodPost, "/api/admin/groups", admin.AccessToken, map[string]any{
		"name": "test-group", "rateMultiplier": 1.5, "concurrencyLimit": 2,
	}).OK(t).Map(t)
	gid := int64(created["id"].(float64))

	patched := a.Do(http.MethodPatch, "/api/admin/groups/"+formatID(gid), admin.AccessToken, map[string]any{
		"description": "desc", "rpmLimit": 100,
	}).OK(t).Map(t)
	if patched["description"] != "desc" || int(patched["rpmLimit"].(float64)) != 100 {
		t.Fatalf("patch = %v", patched)
	}

	planID := a.CreatePlan("GPlan", 30, 1_000_000, 0, gid)
	_ = planID
	a.Do(http.MethodDelete, "/api/admin/groups/"+formatID(gid), admin.AccessToken, nil).Expect(t, 409, "GROUP_IN_USE")

	a.Exec(`UPDATE plans SET group_id = NULL WHERE id = $1`, planID)
	a.Do(http.MethodDelete, "/api/admin/groups/"+formatID(gid), admin.AccessToken, nil).OK(t)
}

func TestPlansCRUD(t *testing.T) {
	a := cloudtest.New(t)
	admin := a.NewAdmin()

	created := a.Do(http.MethodPost, "/api/admin/plans", admin.AccessToken, map[string]any{
		"name": "AdminPlan", "periodDays": 30, "quotaMicros": 5_000_000, "dailyLimitMicros": 100_000, "enabled": true,
	}).OK(t).Map(t)
	pid := int64(created["id"].(float64))

	list := a.Do(http.MethodGet, "/api/admin/plans", admin.AccessToken, nil).OK(t).Map(t)
	found := false
	for _, it := range list["items"].([]any) {
		row := it.(map[string]any)
		if int64(row["id"].(float64)) == pid {
			found = true
			if row["name"] != "AdminPlan" {
				t.Fatalf("plan row = %v", row)
			}
		}
	}
	if !found {
		t.Fatal("新建套餐应出现在列表")
	}

	a.Do(http.MethodPatch, "/api/admin/plans/"+formatID(pid), admin.AccessToken, map[string]any{
		"enabled": false,
	}).OK(t)
	a.Do(http.MethodDelete, "/api/admin/plans/"+formatID(pid), admin.AccessToken, nil).OK(t)
}

func TestAdminUsersAndSettings(t *testing.T) {
	a := cloudtest.New(t)
	admin := a.NewAdmin()
	email := a.UniqueEmail("managed")

	created := a.Do(http.MethodPost, "/api/admin/users", admin.AccessToken, map[string]any{
		"email": email, "password": cloudtest.DefaultPassword, "nickname": "u", "role": "user",
	}).OK(t).Map(t)
	uid := int64(created["id"].(float64))

	detail := a.Do(http.MethodGet, "/api/admin/users/"+formatID(uid), admin.AccessToken, nil).OK(t).Map(t)
	if detail["user"] == nil || detail["devices"] == nil || detail["apiKeys"] == nil {
		t.Fatalf("user detail 形状不对: %v", detail)
	}

	a.Do(http.MethodPost, "/api/admin/users/"+formatID(uid)+"/balance", admin.AccessToken, map[string]any{
		"deltaMicros": 500_000, "note": "gift",
	}).OK(t)

	a.Do(http.MethodPatch, "/api/admin/users/"+formatID(uid), admin.AccessToken, map[string]any{
		"nickname": "U2",
	}).OK(t)

	st := a.Do(http.MethodGet, "/api/admin/settings", admin.AccessToken, nil).OK(t).Map(t)
	for _, k := range []string{"siteName", "currency", "registrationMode", "smtpEnabled"} {
		if _, ok := st[k]; !ok {
			t.Fatalf("settings 缺少 %q: %v", k, st)
		}
	}
	a.Do(http.MethodPut, "/api/admin/settings", admin.AccessToken, map[string]any{
		"siteName": "Test Cloud",
	}).OK(t)
	if a.Do(http.MethodGet, "/api/admin/settings", admin.AccessToken, nil).OK(t).Map(t)["siteName"] != "Test Cloud" {
		t.Fatal("settings 未更新")
	}

	// 非管理员不可访问。
	user := a.NewUser()
	a.Do(http.MethodGet, "/api/admin/dashboard", user.AccessToken, nil).Expect(t, 403, "FORBIDDEN")
}

func TestRedeemCodesAdmin(t *testing.T) {
	a := cloudtest.New(t)
	admin := a.NewAdmin()
	out := a.Do(http.MethodPost, "/api/admin/redeem-codes", admin.AccessToken, map[string]any{
		"kind": "balance", "valueMicros": 1_000_000, "count": 2, "maxUses": 1, "batch": "b1", "prefix": "T",
	}).OK(t).Map(t)
	if len(out["items"].([]any)) != 2 || out["batch"] != "b1" {
		t.Fatalf("create redeem = %v", out)
	}
}

func formatID(id int64) string {
	return strconv.FormatInt(id, 10)
}
