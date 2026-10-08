package admin

import (
	"context"
	"net/http"
	"strings"
	"unicode/utf8"

	"github.com/jackc/pgx/v5"

	"forge-cloud/internal/core"
	"forge-cloud/internal/httpx"
)

// Group 是管理端分组形状（带引用计数）。
type Group struct {
	ID               int64    `json:"id"`
	Name             string   `json:"name"`
	Description      string   `json:"description"`
	RateMultiplier   float64  `json:"rateMultiplier"`
	ConcurrencyLimit int      `json:"concurrencyLimit"`
	RPMLimit         int      `json:"rpmLimit"`
	TPMLimit         int      `json:"tpmLimit"`
	AllowedModels    []string `json:"allowedModels"`
	IsDefault        bool     `json:"isDefault"`
	UserCount        int64    `json:"userCount"`
	AccountCount     int64    `json:"accountCount"`
}

const groupSelect = `SELECT g.id, g.name, g.description, g.rate_multiplier, g.concurrency_limit, g.rpm_limit, g.tpm_limit,
	g.allowed_models, g.is_default,
	(SELECT count(*) FROM users u WHERE u.group_id = g.id),
	(SELECT count(*) FROM account_groups ag WHERE ag.group_id = g.id)
	FROM groups g`

func scanGroup(row pgx.Row) (Group, error) {
	var g Group
	err := row.Scan(&g.ID, &g.Name, &g.Description, &g.RateMultiplier, &g.ConcurrencyLimit, &g.RPMLimit, &g.TPMLimit,
		&g.AllowedModels, &g.IsDefault, &g.UserCount, &g.AccountCount)
	if g.AllowedModels == nil {
		g.AllowedModels = []string{}
	}
	return g, err
}

func loadGroup(ctx context.Context, q pgx.Tx, id int64) (Group, error) {
	g, err := scanGroup(q.QueryRow(ctx, groupSelect+` WHERE g.id = $1`, id))
	if isNoRows(err) {
		return g, core.NotFound("GROUP_NOT_FOUND", "分组不存在")
	}
	return g, err
}

func (s *Service) handleListGroups(w http.ResponseWriter, r *http.Request) {
	rows, err := s.d.DB.Query(r.Context(), groupSelect+` ORDER BY g.id`)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	defer rows.Close()
	items := []Group{}
	for rows.Next() {
		g, err := scanGroup(rows)
		if err != nil {
			httpx.WriteError(w, err)
			return
		}
		items = append(items, g)
	}
	if err := rows.Err(); err != nil {
		httpx.WriteError(w, err)
		return
	}
	httpx.WriteJSON(w, http.StatusOK, map[string]any{"items": items})
}

type groupInput struct {
	Name             *string   `json:"name"`
	Description      *string   `json:"description"`
	RateMultiplier   *float64  `json:"rateMultiplier"`
	ConcurrencyLimit *int      `json:"concurrencyLimit"`
	RPMLimit         *int      `json:"rpmLimit"`
	TPMLimit         *int      `json:"tpmLimit"`
	AllowedModels    *[]string `json:"allowedModels"`
	IsDefault        *bool     `json:"isDefault"`
}

// apply 校验并把提供的字段写进 set；返回审计用的变更摘要。
func (in *groupInput) apply(set *updateSet) (map[string]any, error) {
	detail := map[string]any{}
	if in.Name != nil {
		name := strings.TrimSpace(*in.Name)
		if name == "" || utf8.RuneCountInString(name) > 64 {
			return nil, errInvalid("分组名不能为空且不超过 64 个字")
		}
		set.add("name", name)
		detail["name"] = name
	}
	if in.Description != nil {
		d := strings.TrimSpace(*in.Description)
		if utf8.RuneCountInString(d) > 500 {
			return nil, errInvalid("描述不能超过 500 个字")
		}
		set.add("description", d)
	}
	if in.RateMultiplier != nil {
		if *in.RateMultiplier < 0 || *in.RateMultiplier > 1000 {
			return nil, errInvalid("rateMultiplier 范围 0–1000")
		}
		set.add("rate_multiplier", *in.RateMultiplier)
		detail["rateMultiplier"] = *in.RateMultiplier
	}
	for _, f := range []struct {
		col  string
		key  string
		v    *int
		name string
	}{
		{"concurrency_limit", "concurrencyLimit", in.ConcurrencyLimit, "concurrencyLimit"},
		{"rpm_limit", "rpmLimit", in.RPMLimit, "rpmLimit"},
		{"tpm_limit", "tpmLimit", in.TPMLimit, "tpmLimit"},
	} {
		if f.v == nil {
			continue
		}
		if *f.v < 0 {
			return nil, errInvalid(f.name + " 不能为负")
		}
		set.add(f.col, *f.v)
		detail[f.key] = *f.v
	}
	if in.AllowedModels != nil {
		models := normalizeModels(*in.AllowedModels)
		set.add("allowed_models", models)
		detail["allowedModels"] = models
	}
	return detail, nil
}

func normalizeModels(in []string) []string {
	out := []string{}
	seen := map[string]bool{}
	for _, m := range in {
		m = strings.TrimSpace(m)
		if m == "" || seen[m] {
			continue
		}
		seen[m] = true
		out = append(out, m)
	}
	return out
}

// setDefaultGroup 把 id 设为唯一默认分组（先清掉其它的，满足部分唯一索引）。
func setDefaultGroup(ctx context.Context, tx pgx.Tx, id int64) error {
	if _, err := tx.Exec(ctx, `UPDATE groups SET is_default = FALSE, updated_at = now() WHERE is_default AND id <> $1`, id); err != nil {
		return err
	}
	_, err := tx.Exec(ctx, `UPDATE groups SET is_default = TRUE, updated_at = now() WHERE id = $1`, id)
	return err
}

func errGroupExists() error { return core.Conflict("GROUP_EXISTS", "分组名已存在") }

func (s *Service) handleCreateGroup(w http.ResponseWriter, r *http.Request) {
	var in groupInput
	if err := httpx.DecodeJSON(r, &in, 0); err != nil {
		httpx.WriteError(w, err)
		return
	}
	if in.Name == nil {
		httpx.WriteError(w, errInvalid("分组名必填"))
		return
	}
	set := newUpdateSet(nil)
	detail, err := in.apply(set)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	ctx := r.Context()
	var g Group
	err = pgx.BeginFunc(ctx, s.d.DB, func(tx pgx.Tx) error {
		var id int64
		if err := tx.QueryRow(ctx, `INSERT INTO groups (name) VALUES ($1) RETURNING id`, strings.TrimSpace(*in.Name)).Scan(&id); err != nil {
			if isUniqueViolation(err) {
				return errGroupExists()
			}
			return err
		}
		set.args[0] = id
		if _, err := tx.Exec(ctx, `UPDATE groups SET `+set.clause()+` WHERE id = $1`, set.args...); err != nil {
			return err
		}
		if in.IsDefault != nil && *in.IsDefault {
			if err := setDefaultGroup(ctx, tx, id); err != nil {
				return err
			}
		}
		var err error
		g, err = loadGroup(ctx, tx, id)
		return err
	})
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	detail["isDefault"] = g.IsDefault
	s.audit(r, "group.create", target("group", g.ID), detail)
	httpx.WriteJSON(w, http.StatusOK, g)
}

func (s *Service) handlePatchGroup(w http.ResponseWriter, r *http.Request) {
	id, err := httpx.PathInt64(r, "id")
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	var in groupInput
	if err := httpx.DecodeJSON(r, &in, 0); err != nil {
		httpx.WriteError(w, err)
		return
	}
	set := newUpdateSet(id)
	detail, err := in.apply(set)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	ctx := r.Context()
	var g Group
	err = pgx.BeginFunc(ctx, s.d.DB, func(tx pgx.Tx) error {
		cur, err := loadGroup(ctx, tx, id)
		if err != nil {
			return err
		}
		if in.IsDefault != nil && !*in.IsDefault && cur.IsDefault {
			return core.BadRequest("GROUP_DEFAULT_REQUIRED", "请先把其它分组设为默认")
		}
		if !set.empty() {
			set.raw("updated_at = now()")
			if _, err := tx.Exec(ctx, `UPDATE groups SET `+set.clause()+` WHERE id = $1`, set.args...); err != nil {
				if isUniqueViolation(err) {
					return errGroupExists()
				}
				return err
			}
		}
		if in.IsDefault != nil && *in.IsDefault && !cur.IsDefault {
			if err := setDefaultGroup(ctx, tx, id); err != nil {
				return err
			}
			detail["isDefault"] = true
		}
		g, err = loadGroup(ctx, tx, id)
		return err
	})
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	s.audit(r, "group.update", target("group", id), detail)
	httpx.WriteJSON(w, http.StatusOK, g)
}

// handleDeleteGroup：默认分组（含系统设置指定的）→ 409 GROUP_IS_DEFAULT；被用户或套餐引用 → 409 GROUP_IN_USE。
func (s *Service) handleDeleteGroup(w http.ResponseWriter, r *http.Request) {
	id, err := httpx.PathInt64(r, "id")
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	ctx := r.Context()
	st, err := s.d.Settings.Get(ctx)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	var name string
	err = pgx.BeginFunc(ctx, s.d.DB, func(tx pgx.Tx) error {
		var isDefault bool
		err := tx.QueryRow(ctx, `SELECT name, is_default FROM groups WHERE id = $1 FOR UPDATE`, id).Scan(&name, &isDefault)
		if isNoRows(err) {
			return core.NotFound("GROUP_NOT_FOUND", "分组不存在")
		}
		if err != nil {
			return err
		}
		if isDefault || st.DefaultGroupID == id {
			return core.Conflict("GROUP_IS_DEFAULT", "不能删除默认分组")
		}
		var inUse bool
		if err := tx.QueryRow(ctx,
			`SELECT EXISTS (SELECT 1 FROM users WHERE group_id = $1) OR EXISTS (SELECT 1 FROM plans WHERE group_id = $1)`,
			id).Scan(&inUse); err != nil {
			return err
		}
		if inUse {
			return core.Conflict("GROUP_IN_USE", "分组仍被用户或套餐引用")
		}
		_, err = tx.Exec(ctx, `DELETE FROM groups WHERE id = $1`, id)
		return err
	})
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	s.audit(r, "group.delete", target("group", id), map[string]any{"name": name})
	httpx.OK(w)
}
