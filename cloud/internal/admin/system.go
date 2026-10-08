package admin

import (
	"bytes"
	"encoding/json"
	"fmt"
	"net/http"
	"sort"
	"strings"
	"time"

	"forge-cloud/internal/core"
	"forge-cloud/internal/httpx"
	"forge-cloud/internal/syssettings"
	"forge-cloud/internal/usage"
)

func (s *Service) handleUsage(w http.ResponseWriter, r *http.Request) {
	limit, offset := httpx.Pagination(r)
	qs := r.URL.Query()
	f := usage.Filter{
		UserID:    httpx.QueryInt64(r, "userId"),
		AccountID: httpx.QueryInt64(r, "accountId"),
		Model:     strings.TrimSpace(qs.Get("model")),
		Status:    qs.Get("status"),
		From:      httpx.QueryTime(r, "from"),
		To:        httpx.QueryTime(r, "to"),
	}
	ctx := r.Context()
	items, total, err := usage.ListAdmin(ctx, s.d.DB, f, limit, offset)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	sum, err := usage.Summarize(ctx, s.d.DB, f)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	httpx.WriteJSON(w, http.StatusOK, map[string]any{"items": items, "total": total, "summary": sum})
}

type settingsView struct {
	syssettings.Settings
	SMTPEnabled bool `json:"smtpEnabled"`
}

func (s *Service) settingsView(st syssettings.Settings) settingsView {
	return settingsView{Settings: st, SMTPEnabled: s.d.Config.SMTP.Enabled()}
}

func (s *Service) handleGetSettings(w http.ResponseWriter, r *http.Request) {
	st, err := s.d.Settings.Get(r.Context())
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	httpx.WriteJSON(w, http.StatusOK, s.settingsView(st))
}

// handlePutSettings 部分更新：忽略只读的 smtpEnabled；defaultGroupId=null 视同 0（用 is_default 分组）。
func (s *Service) handlePutSettings(w http.ResponseWriter, r *http.Request) {
	var patch map[string]json.RawMessage
	if err := httpx.DecodeJSON(r, &patch, 0); err != nil {
		httpx.WriteError(w, err)
		return
	}
	if patch == nil {
		httpx.WriteError(w, core.BadRequest("INVALID_JSON", "请求体必须是 JSON 对象"))
		return
	}
	delete(patch, "smtpEnabled")
	ctx := r.Context()
	if raw, ok := patch["defaultGroupId"]; ok {
		var id *int64
		if err := json.Unmarshal(raw, &id); err != nil {
			httpx.WriteError(w, core.BadRequest("INVALID_SETTINGS", "defaultGroupId 必须是整数或 null"))
			return
		}
		if id == nil || *id <= 0 {
			patch["defaultGroupId"] = json.RawMessage("0")
		} else {
			ok, err := groupExists(ctx, s.d.DB, *id)
			if err != nil {
				httpx.WriteError(w, err)
				return
			}
			if !ok {
				httpx.WriteError(w, errGroupNotFound())
				return
			}
		}
	}
	if raw, ok := patch["defaultModel"]; ok {
		var m string
		if err := json.Unmarshal(raw, &m); err != nil {
			httpx.WriteError(w, core.BadRequest("INVALID_SETTINGS", "defaultModel 必须是字符串"))
			return
		}
		if m != "" {
			var exists bool
			if err := s.d.DB.QueryRow(ctx, `SELECT EXISTS (SELECT 1 FROM models WHERE id = $1)`, m).Scan(&exists); err != nil {
				httpx.WriteError(w, err)
				return
			}
			if !exists {
				httpx.WriteError(w, core.BadRequest("MODEL_NOT_FOUND", "模型不存在"))
				return
			}
		}
	}
	body, err := json.Marshal(patch)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	st, err := s.d.Settings.Update(ctx, body)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	keys := make([]string, 0, len(patch))
	for k := range patch {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	s.audit(r, "settings.update", "settings", map[string]any{"keys": keys})
	httpx.WriteJSON(w, http.StatusOK, s.settingsView(st))
}

type auditLog struct {
	ID         int64           `json:"id"`
	ActorID    *int64          `json:"actorId"`
	ActorEmail string          `json:"actorEmail"`
	Action     string          `json:"action"`
	Target     string          `json:"target"`
	Detail     json.RawMessage `json:"detail"`
	IP         string          `json:"ip"`
	CreatedAt  time.Time       `json:"createdAt"`
}

// handleAuditLogs：action 精确匹配，或以「.」结尾时按前缀匹配（如 user.）；q 模糊匹配 action/target。
func (s *Service) handleAuditLogs(w http.ResponseWriter, r *http.Request) {
	limit, offset := httpx.Pagination(r)
	qs := r.URL.Query()
	var conds []string
	var args []any
	add := func(cond string, v any) {
		args = append(args, v)
		conds = append(conds, fmt.Sprintf(cond, len(args)))
	}
	if id := httpx.QueryInt64(r, "actorId"); id > 0 {
		add("a.actor_id = $%d", id)
	}
	if action := strings.TrimSpace(qs.Get("action")); action != "" {
		if strings.HasSuffix(action, ".") {
			add(`a.action LIKE $%d`, strings.NewReplacer(`\`, `\\`, `%`, `\%`, `_`, `\_`).Replace(action)+"%")
		} else {
			add("a.action = $%d", action)
		}
	}
	if q := strings.TrimSpace(qs.Get("q")); q != "" {
		args = append(args, likePattern(q))
		conds = append(conds, fmt.Sprintf("(a.action ILIKE $%d OR a.target ILIKE $%d)", len(args), len(args)))
	}
	where := ""
	if len(conds) > 0 {
		where = " WHERE " + strings.Join(conds, " AND ")
	}
	ctx := r.Context()
	var total int64
	if err := s.d.DB.QueryRow(ctx, `SELECT count(*) FROM audit_logs a`+where, args...).Scan(&total); err != nil {
		httpx.WriteError(w, err)
		return
	}
	rows, err := s.d.DB.Query(ctx, fmt.Sprintf(`SELECT a.id, a.actor_id, COALESCE(u.email, ''), a.action, a.target, a.detail, a.ip, a.created_at
		FROM audit_logs a LEFT JOIN users u ON u.id = a.actor_id%s
		ORDER BY a.id DESC LIMIT %d OFFSET %d`, where, limit, offset), args...)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	defer rows.Close()
	items := []auditLog{}
	for rows.Next() {
		var a auditLog
		var detail []byte
		if err := rows.Scan(&a.ID, &a.ActorID, &a.ActorEmail, &a.Action, &a.Target, &detail, &a.IP, &a.CreatedAt); err != nil {
			httpx.WriteError(w, err)
			return
		}
		if len(bytes.TrimSpace(detail)) == 0 {
			detail = []byte("{}")
		}
		a.Detail = detail
		a.CreatedAt = a.CreatedAt.UTC()
		items = append(items, a)
	}
	if err := rows.Err(); err != nil {
		httpx.WriteError(w, err)
		return
	}
	httpx.WriteJSON(w, http.StatusOK, map[string]any{"items": items, "total": total})
}
