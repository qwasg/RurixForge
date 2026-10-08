package admin

import (
	"context"
	"fmt"
	"net/http"
	"strconv"
	"strings"
	"unicode/utf8"

	"github.com/jackc/pgx/v5"

	"forge-cloud/internal/apikeys"
	"forge-cloud/internal/auth"
	"forge-cloud/internal/billing"
	"forge-cloud/internal/core"
	"forge-cloud/internal/httpx"
)

func errGroupNotFound() error { return core.BadRequest("GROUP_NOT_FOUND", "分组不存在") }

func groupExists(ctx context.Context, q auth.Querier, id int64) (bool, error) {
	var ok bool
	err := q.QueryRow(ctx, `SELECT EXISTS (SELECT 1 FROM groups WHERE id = $1)`, id).Scan(&ok)
	return ok, err
}

func (s *Service) handleListUsers(w http.ResponseWriter, r *http.Request) {
	limit, offset := httpx.Pagination(r)
	qs := r.URL.Query()
	var conds []string
	var args []any
	if q := strings.TrimSpace(qs.Get("q")); q != "" {
		args = append(args, likePattern(q))
		cond := fmt.Sprintf("(u.email ILIKE $%d OR u.nickname ILIKE $%d", len(args), len(args))
		if id, err := strconv.ParseInt(q, 10, 64); err == nil {
			args = append(args, id)
			cond += fmt.Sprintf(" OR u.id = $%d", len(args))
		}
		conds = append(conds, cond+")")
	}
	if st := qs.Get("status"); st != "" {
		args = append(args, st)
		conds = append(conds, fmt.Sprintf("u.status = $%d", len(args)))
	}
	if role := qs.Get("role"); role != "" {
		args = append(args, role)
		conds = append(conds, fmt.Sprintf("u.role = $%d", len(args)))
	}
	where := ""
	if len(conds) > 0 {
		where = " WHERE " + strings.Join(conds, " AND ")
	}
	ctx := r.Context()
	var total int64
	if err := s.d.DB.QueryRow(ctx, `SELECT count(*) FROM users u`+where, args...).Scan(&total); err != nil {
		httpx.WriteError(w, err)
		return
	}
	rows, err := s.d.DB.Query(ctx, fmt.Sprintf(`%s%s ORDER BY u.id DESC LIMIT %d OFFSET %d`, auth.UserSelect, where, limit, offset), args...)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	defer rows.Close()
	items := []auth.AdminUser{}
	for rows.Next() {
		u, err := auth.ScanAdminUser(rows)
		if err != nil {
			httpx.WriteError(w, err)
			return
		}
		items = append(items, u)
	}
	if err := rows.Err(); err != nil {
		httpx.WriteError(w, err)
		return
	}
	httpx.WriteJSON(w, http.StatusOK, map[string]any{"items": items, "total": total})
}

type createUserRequest struct {
	Email         string `json:"email"`
	Password      string `json:"password"`
	Nickname      string `json:"nickname"`
	Role          string `json:"role"`
	GroupID       *int64 `json:"groupId"`
	BalanceMicros int64  `json:"balanceMicros"`
}

func validRole(role string) bool { return role == "user" || role == "admin" }

func (s *Service) handleCreateUser(w http.ResponseWriter, r *http.Request) {
	var req createUserRequest
	if err := httpx.DecodeJSON(r, &req, 0); err != nil {
		httpx.WriteError(w, err)
		return
	}
	email, err := auth.NormalizeEmail(req.Email)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	if err := auth.ValidatePassword(req.Password); err != nil {
		httpx.WriteError(w, err)
		return
	}
	nickname, err := auth.ValidateNickname(req.Nickname)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	role := req.Role
	if role == "" {
		role = "user"
	}
	if !validRole(role) {
		httpx.WriteError(w, errInvalid("role 只能是 user 或 admin"))
		return
	}
	ctx := r.Context()
	var groupID int64
	if req.GroupID != nil && *req.GroupID > 0 {
		ok, err := groupExists(ctx, s.d.DB, *req.GroupID)
		if err != nil {
			httpx.WriteError(w, err)
			return
		}
		if !ok {
			httpx.WriteError(w, errGroupNotFound())
			return
		}
		groupID = *req.GroupID
	} else if groupID, err = s.d.Settings.DefaultGroupID(ctx); err != nil {
		httpx.WriteError(w, err)
		return
	}
	hash, err := auth.HashPassword(req.Password)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	var id int64
	err = pgx.BeginFunc(ctx, s.d.DB, func(tx pgx.Tx) error {
		var err error
		id, err = auth.CreateUser(ctx, tx, auth.NewUser{
			Email: email, PasswordHash: hash, Nickname: nickname, Role: role, GroupID: groupID, EmailVerified: true,
		})
		if err != nil {
			return err
		}
		if req.BalanceMicros != 0 {
			_, err = billing.AdjustBalance(ctx, tx, id, req.BalanceMicros, "admin_adjust", "admin:"+strconv.FormatInt(actorID(r), 10), "初始余额")
		}
		return err
	})
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	s.audit(r, "user.create", target("user", id), map[string]any{
		"email": email, "role": role, "groupId": groupID, "balanceMicros": req.BalanceMicros,
	})
	u, err := auth.LoadAdminUser(ctx, s.d.DB, id)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	httpx.WriteJSON(w, http.StatusOK, u)
}

func (s *Service) handleGetUser(w http.ResponseWriter, r *http.Request) {
	id, err := httpx.PathInt64(r, "id")
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	ctx := r.Context()
	u, err := auth.LoadAdminUser(ctx, s.d.DB, id)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	subs, err := billing.ListSubscriptions(ctx, s.d.DB, id, false)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	ledger, _, err := billing.ListLedger(ctx, s.d.DB, id, 20, 0)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	devices, err := auth.ListDevices(ctx, s.d.DB, id, "")
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	keys, err := apikeys.ListKeys(ctx, s.d.DB, id, true)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	httpx.WriteJSON(w, http.StatusOK, map[string]any{
		"user": u, "subscriptions": subs, "ledger": ledger, "devices": devices, "apiKeys": keys,
	})
}

type patchUserRequest struct {
	Nickname            *string `json:"nickname"`
	Role                *string `json:"role"`
	Status              *string `json:"status"`
	GroupID             *int64  `json:"groupId"`
	ConcurrencyOverride *int    `json:"concurrencyOverride"`
}

func (s *Service) handlePatchUser(w http.ResponseWriter, r *http.Request) {
	id, err := httpx.PathInt64(r, "id")
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	var req patchUserRequest
	f, err := decodePatch(r, &req)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	ctx := r.Context()
	set := newUpdateSet(id)
	detail := map[string]any{}
	if req.Nickname != nil {
		nick, err := auth.ValidateNickname(*req.Nickname)
		if err != nil {
			httpx.WriteError(w, err)
			return
		}
		set.add("nickname", nick)
		detail["nickname"] = nick
	}
	self := id == actorID(r)
	if req.Role != nil {
		if !validRole(*req.Role) {
			httpx.WriteError(w, errInvalid("role 只能是 user 或 admin"))
			return
		}
		if self && *req.Role != "admin" {
			httpx.WriteError(w, core.BadRequest("CANNOT_MODIFY_SELF", "不能撤销自己的管理员身份"))
			return
		}
		set.add("role", *req.Role)
		detail["role"] = *req.Role
	}
	if req.Status != nil {
		if *req.Status != "active" && *req.Status != "disabled" {
			httpx.WriteError(w, errInvalid("status 只能是 active 或 disabled"))
			return
		}
		if self && *req.Status != "active" {
			httpx.WriteError(w, core.BadRequest("CANNOT_MODIFY_SELF", "不能禁用自己的账号"))
			return
		}
		set.add("status", *req.Status)
		detail["status"] = *req.Status
	}
	if f.has("groupId") {
		if req.GroupID == nil || *req.GroupID <= 0 {
			set.add("group_id", nil)
			detail["groupId"] = nil
		} else {
			ok, err := groupExists(ctx, s.d.DB, *req.GroupID)
			if err != nil {
				httpx.WriteError(w, err)
				return
			}
			if !ok {
				httpx.WriteError(w, errGroupNotFound())
				return
			}
			set.add("group_id", *req.GroupID)
			detail["groupId"] = *req.GroupID
		}
	}
	if f.has("concurrencyOverride") {
		if req.ConcurrencyOverride != nil && *req.ConcurrencyOverride < 0 {
			httpx.WriteError(w, errInvalid("concurrencyOverride 不能为负"))
			return
		}
		set.add("concurrency_override", req.ConcurrencyOverride)
		detail["concurrencyOverride"] = req.ConcurrencyOverride
	}
	if !set.empty() {
		set.raw("updated_at = now()")
		tag, err := s.d.DB.Exec(ctx, `UPDATE users SET `+set.clause()+` WHERE id = $1`, set.args...)
		if err != nil {
			httpx.WriteError(w, err)
			return
		}
		if tag.RowsAffected() == 0 {
			httpx.WriteError(w, core.NotFound("USER_NOT_FOUND", "用户不存在"))
			return
		}
		s.audit(r, "user.update", target("user", id), detail)
	}
	u, err := auth.LoadAdminUser(ctx, s.d.DB, id)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	httpx.WriteJSON(w, http.StatusOK, u)
}

func (s *Service) handleAdjustBalance(w http.ResponseWriter, r *http.Request) {
	id, err := httpx.PathInt64(r, "id")
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	var req struct {
		DeltaMicros int64  `json:"deltaMicros"`
		Note        string `json:"note"`
	}
	if err := httpx.DecodeJSON(r, &req, 0); err != nil {
		httpx.WriteError(w, err)
		return
	}
	note := strings.TrimSpace(req.Note)
	if req.DeltaMicros == 0 {
		httpx.WriteError(w, errInvalid("deltaMicros 不能为 0"))
		return
	}
	if utf8.RuneCountInString(note) > 200 {
		httpx.WriteError(w, errInvalid("备注不能超过 200 个字"))
		return
	}
	ctx := r.Context()
	var after int64
	err = pgx.BeginFunc(ctx, s.d.DB, func(tx pgx.Tx) error {
		var err error
		after, err = billing.AdjustBalance(ctx, tx, id, req.DeltaMicros, "admin_adjust", "admin:"+strconv.FormatInt(actorID(r), 10), note)
		return err
	})
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	s.audit(r, "user.balance", target("user", id), map[string]any{"deltaMicros": req.DeltaMicros, "note": note, "balanceMicros": after})
	httpx.WriteJSON(w, http.StatusOK, map[string]any{"balanceMicros": after})
}

func (s *Service) handleSetPassword(w http.ResponseWriter, r *http.Request) {
	id, err := httpx.PathInt64(r, "id")
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	var req struct {
		Password string `json:"password"`
	}
	if err := httpx.DecodeJSON(r, &req, 0); err != nil {
		httpx.WriteError(w, err)
		return
	}
	if err := auth.ValidatePassword(req.Password); err != nil {
		httpx.WriteError(w, err)
		return
	}
	hash, err := auth.HashPassword(req.Password)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	ctx := r.Context()
	err = pgx.BeginFunc(ctx, s.d.DB, func(tx pgx.Tx) error {
		tag, err := tx.Exec(ctx, `UPDATE users SET password_hash = $2, updated_at = now() WHERE id = $1`, id, hash)
		if err != nil {
			return err
		}
		if tag.RowsAffected() == 0 {
			return core.NotFound("USER_NOT_FOUND", "用户不存在")
		}
		return auth.RevokeUserSessions(ctx, tx, id, "")
	})
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	s.audit(r, "user.password", target("user", id), nil)
	httpx.OK(w)
}

func (s *Service) handleGrantSubscription(w http.ResponseWriter, r *http.Request) {
	id, err := httpx.PathInt64(r, "id")
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	var req struct {
		PlanID int64 `json:"planId"`
		Days   int   `json:"days"`
	}
	if err := httpx.DecodeJSON(r, &req, 0); err != nil {
		httpx.WriteError(w, err)
		return
	}
	if req.PlanID <= 0 {
		httpx.WriteError(w, errInvalid("planId 必填"))
		return
	}
	if req.Days < 0 || req.Days > 3650 {
		httpx.WriteError(w, errInvalid("days 范围 0–3650（0 表示用套餐周期）"))
		return
	}
	ctx := r.Context()
	var sub billing.Subscription
	err = pgx.BeginFunc(ctx, s.d.DB, func(tx pgx.Tx) error {
		if _, err := auth.LoadAdminUser(ctx, tx, id); err != nil {
			return err
		}
		var err error
		sub, err = billing.CreateSubscription(ctx, tx, id, req.PlanID, req.Days)
		return err
	})
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	s.audit(r, "user.subscription.grant", target("user", id), map[string]any{
		"planId": req.PlanID, "days": req.Days, "subscriptionId": sub.ID,
	})
	httpx.WriteJSON(w, http.StatusOK, sub)
}

func (s *Service) handleCancelSubscription(w http.ResponseWriter, r *http.Request) {
	id, err := httpx.PathInt64(r, "id")
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	subID, err := httpx.PathInt64(r, "subId")
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	tag, err := s.d.DB.Exec(r.Context(),
		`UPDATE subscriptions SET status = 'cancelled' WHERE id = $1 AND user_id = $2`, subID, id)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	if tag.RowsAffected() == 0 {
		httpx.WriteError(w, core.NotFound("SUBSCRIPTION_NOT_FOUND", "订阅不存在"))
		return
	}
	s.audit(r, "user.subscription.cancel", target("user", id), map[string]any{"subscriptionId": subID})
	httpx.OK(w)
}
