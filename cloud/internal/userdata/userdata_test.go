package userdata_test

import (
	"encoding/base64"
	"encoding/json"
	"fmt"
	"net/http"
	"strconv"
	"strings"
	"testing"
	"time"

	"forge-cloud/internal/cloudtest"
	"forge-cloud/internal/core"
)

func TestSettingsLWWAndConflict(t *testing.T) {
	a := cloudtest.New(t)
	token := a.NewUser().AccessToken

	items := a.Do(http.MethodGet, "/api/v1/me/settings", token, nil).OK(t).Map(t)["items"].(map[string]any)
	if len(items) != 0 {
		t.Fatalf("初始 settings 应为空: %v", items)
	}

	put := func(ns string, body map[string]any) *cloudtest.Response {
		return a.Do(http.MethodPut, "/api/v1/me/settings/"+ns, token, body)
	}
	v1 := put("appearance", map[string]any{"value": map[string]any{"theme": "dark"}, "baseVersion": 0}).OK(t).Map(t)
	if int64(v1["version"].(float64)) != 1 {
		t.Fatalf("version = %v", v1["version"])
	}

	conf := put("appearance", map[string]any{"value": map[string]any{"theme": "light"}, "baseVersion": 0}).
		Expect(t, 409, "SETTINGS_VERSION_CONFLICT").Map(t)
	cur := conf["current"].(map[string]any)
	if int64(cur["version"].(float64)) != 1 {
		t.Fatalf("conflict current = %v", cur)
	}

	put("appearance", map[string]any{"value": map[string]any{"theme": "light"}, "baseVersion": 1, "force": true}).OK(t)

	tooLarge, _ := json.Marshal(map[string]any{"value": map[string]string{"x": strings.Repeat("a", 64<<10)}, "baseVersion": 0})
	a.Do(http.MethodPut, "/api/v1/me/settings/composer", token, json.RawMessage(tooLarge)).Expect(t, 413, "SETTINGS_TOO_LARGE")
	put("Bad_NS", map[string]any{"value": map[string]any{"a": 1}, "baseVersion": 0}).Expect(t, 400, "INVALID_NAMESPACE")
}

func TestMemoriesCursorConflictsAndLimit(t *testing.T) {
	a := cloudtest.New(t)
	s := a.NewUser()
	token := s.AccessToken
	now := time.Now().UTC().Truncate(time.Microsecond)
	id := "mem-" + core.RandomString(8)

	sync := func(body map[string]any) map[string]any {
		return a.Do(http.MethodPut, "/api/v1/me/memories", token, body).OK(t).Map(t)
	}
	page := func(since int64) map[string]any {
		path := "/api/v1/me/memories?since=" + strconv.FormatInt(since, 10)
		return a.Do(http.MethodGet, path, token, nil).OK(t).Map(t)
	}

	r1 := sync(map[string]any{"items": []map[string]any{{
		"id": id, "scope": "global", "kind": "fact", "content": "alpha", "tags": []string{"t"},
		"updatedAt": now.Format(time.RFC3339Nano), "deleted": false,
	}}})
	if len(r1["applied"].([]any)) != 1 {
		t.Fatalf("applied = %v", r1["applied"])
	}
	cursor := int64(r1["cursor"].(float64))
	if cursor <= 0 {
		t.Fatal("cursor 应递增")
	}

	older := now.Add(-time.Hour)
	r2 := sync(map[string]any{"items": []map[string]any{{
		"id": id, "scope": "global", "kind": "fact", "content": "stale", "tags": []string{},
		"updatedAt": older.Format(time.RFC3339Nano), "deleted": false,
	}}})
	if len(r2["conflicts"].([]any)) != 1 {
		t.Fatalf("应冲突: %v", r2)
	}

	newer := now.Add(time.Minute)
	sync(map[string]any{"items": []map[string]any{{
		"id": id, "scope": "global", "kind": "fact", "content": "beta", "tags": []string{},
		"updatedAt": newer.Format(time.RFC3339Nano), "deleted": false,
	}}})
	if len(page(cursor - 1)["items"].([]any)) < 1 {
		t.Fatal("增量拉取应有更新")
	}

	sync(map[string]any{"items": []map[string]any{{
		"id": id, "scope": "global", "kind": "fact", "content": "", "tags": []string{},
		"updatedAt": newer.Add(time.Minute).Format(time.RFC3339Nano), "deleted": true,
	}}})

	// 活跃记忆上限（直接插 2000 条再尝试新增）。
	a.Exec(`INSERT INTO user_memories (user_id, id, scope, kind, content, tags, version, updated_at, deleted, change_seq)
		SELECT $1, 'bulk-' || g::text, 'global', 'fact', 'x', '{}', 1, now(), false, nextval('userdata_change_seq')
		FROM generate_series(1, 2000) g`, s.UserID)
	a.Do(http.MethodPut, "/api/v1/me/memories", token, map[string]any{"items": []map[string]any{{
		"id": "one-more", "scope": "global", "kind": "fact", "content": "nope", "tags": []string{},
		"updatedAt": time.Now().UTC().Format(time.RFC3339Nano), "deleted": false,
	}}}).Expect(t, 400, "MEMORY_LIMIT")
}

func TestSkillsLWWCursorAndValidation(t *testing.T) {
	a := cloudtest.New(t)
	token := a.NewUser().AccessToken
	name := "demo-skill"
	md := base64.StdEncoding.EncodeToString([]byte("# skill"))
	ts := time.Now().UTC().Truncate(time.Microsecond)

	put := func(at time.Time) map[string]any {
		return a.Do(http.MethodPut, "/api/v1/me/skills/"+name, token, map[string]any{
			"files": map[string]string{"SKILL.md": md}, "updatedAt": at.Format(time.RFC3339Nano),
		}).OK(t).Map(t)
	}
	r1 := put(ts)
	if r1["applied"] != true {
		t.Fatalf("首次应 applied: %v", r1)
	}
	cursor := int64(r1["cursor"].(float64))

	stale := ts.Add(-time.Hour)
	r2 := a.Do(http.MethodPut, "/api/v1/me/skills/"+name, token, map[string]any{
		"files": map[string]string{"SKILL.md": md}, "updatedAt": stale.Format(time.RFC3339Nano),
	}).OK(t).Map(t)
	if r2["applied"] != false {
		t.Fatalf("旧时间戳应 applied:false: %v", r2)
	}

	newer := ts.Add(time.Minute)
	put(newer)

	page := a.Do(http.MethodGet, "/api/v1/me/skills?since="+strconv.FormatInt(cursor-1, 10), token, nil).OK(t).Map(t)
	if len(page["items"].([]any)) == 0 {
		t.Fatal("skills 增量应有条目")
	}

	a.Do(http.MethodPut, "/api/v1/me/skills/Invalid", token, map[string]any{
		"files": map[string]string{"SKILL.md": md}, "updatedAt": newer.Format(time.RFC3339Nano),
	}).Expect(t, 400, "SKILL_INVALID")

	huge := base64.StdEncoding.EncodeToString(make([]byte, 2<<20+1))
	a.Do(http.MethodPut, "/api/v1/me/skills/"+name, token, map[string]any{
		"files": map[string]string{"SKILL.md": huge}, "updatedAt": newer.Add(time.Minute).Format(time.RFC3339Nano),
	}).Expect(t, 413, "SKILL_TOO_LARGE")

	delAt := newer.Add(2 * time.Minute)
	del := a.Do(http.MethodDelete, fmt.Sprintf("/api/v1/me/skills/%s?updatedAt=%s", name, delAt.Format(time.RFC3339Nano)), token, nil).OK(t).Map(t)
	if del["applied"] != true {
		t.Fatalf("delete: %v", del)
	}
}
