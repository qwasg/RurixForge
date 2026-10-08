package admin

import (
	"context"
	"net/http"
	"regexp"
	"strings"
	"unicode/utf8"

	"github.com/jackc/pgx/v5"

	"forge-cloud/internal/core"
	"forge-cloud/internal/httpx"
)

// Plan 是管理端套餐形状；tier 非空的是会员档位（§11），为空的是普通额度包。
type Plan struct {
	ID                int64    `json:"id"`
	Name              string   `json:"name"`
	Description       string   `json:"description"`
	PriceMicros       int64    `json:"priceMicros"`
	PeriodDays        int      `json:"periodDays"`
	QuotaMicros       int64    `json:"quotaMicros"`
	DailyLimitMicros  int64    `json:"dailyLimitMicros"`
	GroupID           *int64   `json:"groupId"`
	Enabled           bool     `json:"enabled"`
	Tier              string   `json:"tier"`
	TierRank          int      `json:"tierRank"`
	Tagline           string   `json:"tagline"`
	Features          []string `json:"features"`
	PriceYearlyMicros int64    `json:"priceYearlyMicros"`
	ForgeQuotaMicros  int64    `json:"forgeQuotaMicros"`
	Highlight         bool     `json:"highlight"`
	// SubscriberCount：当前生效中的订阅数（只读）。
	SubscriberCount int64 `json:"subscriberCount"`
}

const planSelect = `SELECT id, name, description, price_micros, period_days, quota_micros, daily_limit_micros, group_id, enabled,
	tier, tier_rank, tagline, features, price_yearly_micros, forge_quota_micros, highlight,
	(SELECT count(*) FROM subscriptions s WHERE s.plan_id = plans.id AND s.status = 'active' AND s.ends_at > now())
	FROM plans`

func scanPlan(row pgx.Row) (Plan, error) {
	var p Plan
	err := row.Scan(&p.ID, &p.Name, &p.Description, &p.PriceMicros, &p.PeriodDays, &p.QuotaMicros, &p.DailyLimitMicros,
		&p.GroupID, &p.Enabled, &p.Tier, &p.TierRank, &p.Tagline, &p.Features, &p.PriceYearlyMicros,
		&p.ForgeQuotaMicros, &p.Highlight, &p.SubscriberCount)
	if p.Features == nil {
		p.Features = []string{}
	}
	return p, err
}

var tierPattern = regexp.MustCompile(`^[a-z][a-z0-9_]{0,31}$`)

// planWriteErr 把档位标识唯一约束冲突转成 409 TIER_TAKEN。
func planWriteErr(err error) error {
	if isUniqueViolation(err) {
		return core.Conflict("TIER_TAKEN", "该档位标识已被其它套餐使用")
	}
	return err
}

func loadPlan(ctx context.Context, q pgx.Tx, id int64) (Plan, error) {
	p, err := scanPlan(q.QueryRow(ctx, planSelect+` WHERE id = $1`, id))
	if isNoRows(err) {
		return p, core.NotFound("PLAN_NOT_FOUND", "套餐不存在")
	}
	return p, err
}

func (s *Service) handleListPlans(w http.ResponseWriter, r *http.Request) {
	rows, err := s.d.DB.Query(r.Context(), planSelect+` ORDER BY id`)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	defer rows.Close()
	items := []Plan{}
	for rows.Next() {
		p, err := scanPlan(rows)
		if err != nil {
			httpx.WriteError(w, err)
			return
		}
		items = append(items, p)
	}
	if err := rows.Err(); err != nil {
		httpx.WriteError(w, err)
		return
	}
	httpx.WriteJSON(w, http.StatusOK, map[string]any{"items": items})
}

type planInput struct {
	Name              *string   `json:"name"`
	Description       *string   `json:"description"`
	PriceMicros       *int64    `json:"priceMicros"`
	PeriodDays        *int      `json:"periodDays"`
	QuotaMicros       *int64    `json:"quotaMicros"`
	DailyLimitMicros  *int64    `json:"dailyLimitMicros"`
	GroupID           *int64    `json:"groupId"`
	Enabled           *bool     `json:"enabled"`
	Tier              *string   `json:"tier"`
	TierRank          *int      `json:"tierRank"`
	Tagline           *string   `json:"tagline"`
	Features          *[]string `json:"features"`
	PriceYearlyMicros *int64    `json:"priceYearlyMicros"`
	ForgeQuotaMicros  *int64    `json:"forgeQuotaMicros"`
	Highlight         *bool     `json:"highlight"`
}

// applyTier 校验并写入会员档位字段（§11.3）。
func (in *planInput) applyTier(set *updateSet, detail map[string]any) error {
	if in.Tier != nil {
		t := strings.ToLower(strings.TrimSpace(*in.Tier))
		if t != "" && !tierPattern.MatchString(t) {
			return errInvalid("tier 须为小写字母开头的字母/数字/下划线（≤ 32 位），留空表示普通额度包")
		}
		set.add("tier", t)
		detail["tier"] = t
	}
	if in.TierRank != nil {
		if *in.TierRank < 0 || *in.TierRank > 1000 {
			return errInvalid("tierRank 范围 0–1000")
		}
		set.add("tier_rank", *in.TierRank)
		detail["tierRank"] = *in.TierRank
	}
	if in.Tagline != nil {
		t := strings.TrimSpace(*in.Tagline)
		if utf8.RuneCountInString(t) > 64 {
			return errInvalid("标语不能超过 64 个字")
		}
		set.add("tagline", t)
	}
	if in.Features != nil {
		feats := make([]string, 0, len(*in.Features))
		for _, f := range *in.Features {
			if f = strings.TrimSpace(f); f == "" {
				continue
			}
			if utf8.RuneCountInString(f) > 80 {
				return errInvalid("每条权益不能超过 80 个字")
			}
			feats = append(feats, f)
		}
		if len(feats) > 12 {
			return errInvalid("权益最多 12 条")
		}
		set.add("features", feats)
		detail["features"] = len(feats)
	}
	if in.Highlight != nil {
		set.add("highlight", *in.Highlight)
		detail["highlight"] = *in.Highlight
	}
	return nil
}

func (in *planInput) apply(ctx context.Context, s *Service, f fields, set *updateSet) (map[string]any, error) {
	detail := map[string]any{}
	if in.Name != nil {
		name := strings.TrimSpace(*in.Name)
		if name == "" || utf8.RuneCountInString(name) > 64 {
			return nil, errInvalid("套餐名不能为空且不超过 64 个字")
		}
		set.add("name", name)
		detail["name"] = name
	}
	if in.Description != nil {
		d := strings.TrimSpace(*in.Description)
		if utf8.RuneCountInString(d) > 1000 {
			return nil, errInvalid("描述不能超过 1000 个字")
		}
		set.add("description", d)
	}
	for _, m := range []struct {
		col string
		key string
		v   *int64
	}{
		{"price_micros", "priceMicros", in.PriceMicros},
		{"quota_micros", "quotaMicros", in.QuotaMicros},
		{"daily_limit_micros", "dailyLimitMicros", in.DailyLimitMicros},
		{"price_yearly_micros", "priceYearlyMicros", in.PriceYearlyMicros},
		{"forge_quota_micros", "forgeQuotaMicros", in.ForgeQuotaMicros},
	} {
		if m.v == nil {
			continue
		}
		if *m.v < 0 {
			return nil, errInvalid(m.key + " 不能为负")
		}
		set.add(m.col, *m.v)
		detail[m.key] = *m.v
	}
	if err := in.applyTier(set, detail); err != nil {
		return nil, err
	}
	if in.PeriodDays != nil {
		if *in.PeriodDays <= 0 || *in.PeriodDays > 3650 {
			return nil, errInvalid("periodDays 范围 1–3650")
		}
		set.add("period_days", *in.PeriodDays)
		detail["periodDays"] = *in.PeriodDays
	}
	if f.has("groupId") {
		if in.GroupID == nil || *in.GroupID <= 0 {
			set.add("group_id", nil)
			detail["groupId"] = nil
		} else {
			ok, err := groupExists(ctx, s.d.DB, *in.GroupID)
			if err != nil {
				return nil, err
			}
			if !ok {
				return nil, errGroupNotFound()
			}
			set.add("group_id", *in.GroupID)
			detail["groupId"] = *in.GroupID
		}
	}
	if in.Enabled != nil {
		set.add("enabled", *in.Enabled)
		detail["enabled"] = *in.Enabled
	}
	return detail, nil
}

func (s *Service) handleCreatePlan(w http.ResponseWriter, r *http.Request) {
	var in planInput
	f, err := decodePatch(r, &in)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	if in.Name == nil {
		httpx.WriteError(w, errInvalid("套餐名必填"))
		return
	}
	ctx := r.Context()
	set := newUpdateSet(nil)
	detail, err := in.apply(ctx, s, f, set)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	var p Plan
	err = pgx.BeginFunc(ctx, s.d.DB, func(tx pgx.Tx) error {
		var id int64
		if err := tx.QueryRow(ctx, `INSERT INTO plans (name) VALUES ($1) RETURNING id`, strings.TrimSpace(*in.Name)).Scan(&id); err != nil {
			return err
		}
		set.args[0] = id
		if _, err := tx.Exec(ctx, `UPDATE plans SET `+set.clause()+` WHERE id = $1`, set.args...); err != nil {
			return planWriteErr(err)
		}
		var err error
		p, err = loadPlan(ctx, tx, id)
		return err
	})
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	s.audit(r, "plan.create", target("plan", p.ID), detail)
	httpx.WriteJSON(w, http.StatusOK, p)
}

func (s *Service) handlePatchPlan(w http.ResponseWriter, r *http.Request) {
	id, err := httpx.PathInt64(r, "id")
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	var in planInput
	f, err := decodePatch(r, &in)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	ctx := r.Context()
	set := newUpdateSet(id)
	detail, err := in.apply(ctx, s, f, set)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	var p Plan
	err = pgx.BeginFunc(ctx, s.d.DB, func(tx pgx.Tx) error {
		if !set.empty() {
			set.raw("updated_at = now()")
			if _, err := tx.Exec(ctx, `UPDATE plans SET `+set.clause()+` WHERE id = $1`, set.args...); err != nil {
				return planWriteErr(err)
			}
		}
		var err error
		p, err = loadPlan(ctx, tx, id)
		return err
	})
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	s.audit(r, "plan.update", target("plan", id), detail)
	httpx.WriteJSON(w, http.StatusOK, p)
}

// handleDeletePlan：已有订阅引用的套餐不能删（改为停用）→ 409 PLAN_IN_USE。
func (s *Service) handleDeletePlan(w http.ResponseWriter, r *http.Request) {
	id, err := httpx.PathInt64(r, "id")
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	ctx := r.Context()
	var name string
	err = pgx.BeginFunc(ctx, s.d.DB, func(tx pgx.Tx) error {
		if err := tx.QueryRow(ctx, `SELECT name FROM plans WHERE id = $1 FOR UPDATE`, id).Scan(&name); err != nil {
			if isNoRows(err) {
				return core.NotFound("PLAN_NOT_FOUND", "套餐不存在")
			}
			return err
		}
		var inUse bool
		if err := tx.QueryRow(ctx, `SELECT EXISTS (SELECT 1 FROM subscriptions WHERE plan_id = $1)`, id).Scan(&inUse); err != nil {
			return err
		}
		if inUse {
			return core.Conflict("PLAN_IN_USE", "套餐已有订阅记录，请改为停用")
		}
		_, err := tx.Exec(ctx, `DELETE FROM plans WHERE id = $1`, id)
		if isFKViolation(err) {
			return core.Conflict("PLAN_IN_USE", "套餐仍被引用，请改为停用")
		}
		return err
	})
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	s.audit(r, "plan.delete", target("plan", id), map[string]any{"name": name})
	httpx.OK(w)
}
