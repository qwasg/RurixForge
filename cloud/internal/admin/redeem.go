package admin

import (
	"encoding/csv"
	"fmt"
	"net/http"
	"regexp"
	"strconv"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/jackc/pgx/v5"

	"forge-cloud/internal/core"
	"forge-cloud/internal/httpx"
)

const (
	defaultRedeemPrefix = "RF-"
	redeemRandLen       = 16
	maxRedeemBatch      = 1000
	maxRedeemExport     = 100_000
)

var (
	redeemPrefixRe = regexp.MustCompile(`^[A-Za-z0-9_-]{0,16}$`)
	redeemBatchRe  = regexp.MustCompile(`^[A-Za-z0-9_.:-]{1,64}$`)
)

// RedeemCode 是管理端兑换码形状；status 为派生值（revoked > expired > exhausted > active）。
type RedeemCode struct {
	ID          int64      `json:"id"`
	Code        string     `json:"code"`
	Kind        string     `json:"kind"`
	ValueMicros int64      `json:"valueMicros"`
	PlanID      *int64     `json:"planId"`
	PlanName    string     `json:"planName"`
	Batch       string     `json:"batch"`
	MaxUses     int        `json:"maxUses"`
	UsedCount   int        `json:"usedCount"`
	Status      string     `json:"status"`
	ExpiresAt   *time.Time `json:"expiresAt"`
	Note        string     `json:"note"`
	CreatedAt   time.Time  `json:"createdAt"`
}

const redeemStatusExpr = `CASE WHEN rc.status = 'revoked' THEN 'revoked'
	WHEN rc.expires_at IS NOT NULL AND rc.expires_at <= now() THEN 'expired'
	WHEN rc.used_count >= rc.max_uses THEN 'exhausted'
	ELSE 'active' END`

const redeemSelect = `SELECT rc.id, rc.code, rc.kind, rc.value_micros, rc.plan_id, COALESCE(p.name, ''), rc.batch,
	rc.max_uses, rc.used_count, ` + redeemStatusExpr + `, rc.expires_at, rc.note, rc.created_at
	FROM redeem_codes rc LEFT JOIN plans p ON p.id = rc.plan_id`

func scanRedeem(row pgx.Row) (RedeemCode, error) {
	var c RedeemCode
	err := row.Scan(&c.ID, &c.Code, &c.Kind, &c.ValueMicros, &c.PlanID, &c.PlanName, &c.Batch, &c.MaxUses, &c.UsedCount,
		&c.Status, &c.ExpiresAt, &c.Note, &c.CreatedAt)
	c.CreatedAt = c.CreatedAt.UTC()
	if c.ExpiresAt != nil {
		t := c.ExpiresAt.UTC()
		c.ExpiresAt = &t
	}
	return c, err
}

// redeemWhere 解析 ?batch=&status=&kind=&q= 过滤条件。
func redeemWhere(r *http.Request) (string, []any) {
	qs := r.URL.Query()
	var conds []string
	var args []any
	add := func(cond string, v any) {
		args = append(args, v)
		conds = append(conds, fmt.Sprintf(cond, len(args)))
	}
	if v := strings.TrimSpace(qs.Get("batch")); v != "" {
		add("rc.batch = $%d", v)
	}
	if v := qs.Get("status"); v != "" {
		add("("+redeemStatusExpr+") = $%d", v)
	}
	if v := qs.Get("kind"); v != "" {
		add("rc.kind = $%d", v)
	}
	if v := strings.TrimSpace(qs.Get("q")); v != "" {
		args = append(args, likePattern(v))
		n := len(args)
		conds = append(conds, fmt.Sprintf("(rc.code ILIKE $%d OR rc.note ILIKE $%d OR rc.batch ILIKE $%d)", n, n, n))
	}
	if len(conds) == 0 {
		return "", nil
	}
	return " WHERE " + strings.Join(conds, " AND "), args
}

func (s *Service) handleListRedeemCodes(w http.ResponseWriter, r *http.Request) {
	limit, offset := httpx.Pagination(r)
	where, args := redeemWhere(r)
	ctx := r.Context()
	var total int64
	if err := s.d.DB.QueryRow(ctx, `SELECT count(*) FROM redeem_codes rc`+where, args...).Scan(&total); err != nil {
		httpx.WriteError(w, err)
		return
	}
	rows, err := s.d.DB.Query(ctx, fmt.Sprintf(`%s%s ORDER BY rc.id DESC LIMIT %d OFFSET %d`, redeemSelect, where, limit, offset), args...)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	defer rows.Close()
	items := []RedeemCode{}
	for rows.Next() {
		c, err := scanRedeem(rows)
		if err != nil {
			httpx.WriteError(w, err)
			return
		}
		items = append(items, c)
	}
	if err := rows.Err(); err != nil {
		httpx.WriteError(w, err)
		return
	}
	httpx.WriteJSON(w, http.StatusOK, map[string]any{"items": items, "total": total})
}

type createRedeemRequest struct {
	Kind        string     `json:"kind"`
	ValueMicros int64      `json:"valueMicros"`
	PlanID      *int64     `json:"planId"`
	Count       int        `json:"count"`
	MaxUses     int        `json:"maxUses"`
	ExpiresAt   *time.Time `json:"expiresAt"`
	Note        string     `json:"note"`
	Batch       string     `json:"batch"`
	Prefix      string     `json:"prefix"`
}

// handleCreateRedeemCodes 批量生成兑换码：code = prefix（缺省 RF-）+ 16 位 base62；batch 缺省 b+yyyyMMddHHmmss（UTC）。
func (s *Service) handleCreateRedeemCodes(w http.ResponseWriter, r *http.Request) {
	var req createRedeemRequest
	if err := httpx.DecodeJSON(r, &req, 0); err != nil {
		httpx.WriteError(w, err)
		return
	}
	if req.Count < 1 || req.Count > maxRedeemBatch {
		httpx.WriteError(w, errInvalid("count 范围 1–1000"))
		return
	}
	if req.MaxUses <= 0 {
		req.MaxUses = 1
	}
	if req.MaxUses > 1_000_000 {
		httpx.WriteError(w, errInvalid("maxUses 过大"))
		return
	}
	ctx := r.Context()
	var planID any
	switch req.Kind {
	case "balance":
		if req.ValueMicros <= 0 {
			httpx.WriteError(w, errInvalid("余额兑换码的 valueMicros 必须大于 0"))
			return
		}
	case "plan":
		if req.PlanID == nil || *req.PlanID <= 0 {
			httpx.WriteError(w, errInvalid("套餐兑换码必须指定 planId"))
			return
		}
		var ok bool
		if err := s.d.DB.QueryRow(ctx, `SELECT EXISTS (SELECT 1 FROM plans WHERE id = $1)`, *req.PlanID).Scan(&ok); err != nil {
			httpx.WriteError(w, err)
			return
		}
		if !ok {
			httpx.WriteError(w, core.BadRequest("PLAN_NOT_FOUND", "套餐不存在"))
			return
		}
		planID = *req.PlanID
		if req.ValueMicros < 0 {
			req.ValueMicros = 0
		}
	case "invite":
		req.ValueMicros = 0
	default:
		httpx.WriteError(w, errInvalid("kind 只能是 balance/plan/invite"))
		return
	}
	if req.ExpiresAt != nil && !req.ExpiresAt.After(time.Now()) {
		httpx.WriteError(w, errInvalid("过期时间必须晚于当前时间"))
		return
	}
	note := strings.TrimSpace(req.Note)
	if utf8.RuneCountInString(note) > 200 {
		httpx.WriteError(w, errInvalid("备注不能超过 200 个字"))
		return
	}
	prefix := strings.TrimSpace(req.Prefix)
	if prefix == "" {
		prefix = defaultRedeemPrefix
	}
	if !redeemPrefixRe.MatchString(prefix) {
		httpx.WriteError(w, errInvalid("prefix 只能含字母、数字、- 和 _，且不超过 16 个字符"))
		return
	}
	batch := strings.TrimSpace(req.Batch)
	if batch == "" {
		batch = "b" + time.Now().UTC().Format("20060102150405")
	}
	if !redeemBatchRe.MatchString(batch) {
		httpx.WriteError(w, errInvalid("batch 只能含字母、数字与 -_.:，且不超过 64 个字符"))
		return
	}
	codes := make([]string, req.Count)
	for i := range codes {
		codes[i] = prefix + core.RandomString(redeemRandLen)
	}
	rows, err := s.d.DB.Query(ctx,
		`WITH ins AS (
			INSERT INTO redeem_codes (code, kind, value_micros, plan_id, batch, max_uses, expires_at, note, created_by)
			SELECT c, $2::text, $3::bigint, $4::bigint, $5::text, $6::int, $7::timestamptz, $8::text, $9::bigint
			FROM unnest($1::text[]) AS c
			RETURNING *
		)
		SELECT rc.id, rc.code, rc.kind, rc.value_micros, rc.plan_id, COALESCE(p.name, ''), rc.batch,
		       rc.max_uses, rc.used_count, `+redeemStatusExpr+`, rc.expires_at, rc.note, rc.created_at
		FROM ins rc LEFT JOIN plans p ON p.id = rc.plan_id ORDER BY rc.id`,
		codes, req.Kind, req.ValueMicros, planID, batch, req.MaxUses, req.ExpiresAt, note, nullID(actorID(r)))
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	defer rows.Close()
	items := make([]RedeemCode, 0, req.Count)
	for rows.Next() {
		c, err := scanRedeem(rows)
		if err != nil {
			httpx.WriteError(w, err)
			return
		}
		items = append(items, c)
	}
	if err := rows.Err(); err != nil {
		httpx.WriteError(w, err)
		return
	}
	s.audit(r, "redeem.create", "batch:"+batch, map[string]any{
		"kind": req.Kind, "count": req.Count, "valueMicros": req.ValueMicros, "planId": planID, "maxUses": req.MaxUses,
	})
	httpx.WriteJSON(w, http.StatusOK, map[string]any{"items": items, "batch": batch})
}

func (s *Service) handleRevokeRedeemCode(w http.ResponseWriter, r *http.Request) {
	id, err := httpx.PathInt64(r, "id")
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	ctx := r.Context()
	tag, err := s.d.DB.Exec(ctx, `UPDATE redeem_codes SET status = 'revoked' WHERE id = $1`, id)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	if tag.RowsAffected() == 0 {
		httpx.WriteError(w, core.NotFound("REDEEM_CODE_NOT_FOUND", "兑换码不存在"))
		return
	}
	c, err := scanRedeem(s.d.DB.QueryRow(ctx, redeemSelect+` WHERE rc.id = $1`, id))
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	s.audit(r, "redeem.revoke", target("redeem", id), map[string]any{"batch": c.Batch})
	httpx.WriteJSON(w, http.StatusOK, c)
}

// csvSafe 防 CSV 公式注入：以 = + - @ 开头的单元格前加单引号。
func csvSafe(v string) string {
	if v != "" && strings.ContainsRune("=+-@\t\r", rune(v[0])) {
		return "'" + v
	}
	return v
}

func formatTime(t *time.Time) string {
	if t == nil {
		return ""
	}
	return t.UTC().Format(time.RFC3339)
}

// handleExportRedeemCodes 按与列表相同的过滤条件导出 CSV（UTF-8 BOM，便于 Excel 打开）。
func (s *Service) handleExportRedeemCodes(w http.ResponseWriter, r *http.Request) {
	where, args := redeemWhere(r)
	ctx := r.Context()
	rows, err := s.d.DB.Query(ctx, fmt.Sprintf(`%s%s ORDER BY rc.id LIMIT %d`, redeemSelect, where, maxRedeemExport), args...)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	defer rows.Close()
	var items []RedeemCode
	for rows.Next() {
		c, err := scanRedeem(rows)
		if err != nil {
			httpx.WriteError(w, err)
			return
		}
		items = append(items, c)
	}
	if err := rows.Err(); err != nil {
		httpx.WriteError(w, err)
		return
	}
	name := "redeem-codes"
	if b := strings.TrimSpace(r.URL.Query().Get("batch")); b != "" && redeemBatchRe.MatchString(b) {
		name += "-" + b
	}
	s.audit(r, "redeem.export", "batch:"+r.URL.Query().Get("batch"), map[string]any{"count": len(items)})
	w.Header().Set("Content-Type", "text/csv; charset=utf-8")
	w.Header().Set("Content-Disposition", fmt.Sprintf(`attachment; filename="%s.csv"`, name))
	w.WriteHeader(http.StatusOK)
	_, _ = w.Write([]byte("\xEF\xBB\xBF"))
	cw := csv.NewWriter(w)
	_ = cw.Write([]string{"code", "kind", "valueMicros", "planId", "planName", "batch", "maxUses", "usedCount", "status", "expiresAt", "note", "createdAt"})
	for _, c := range items {
		planID := ""
		if c.PlanID != nil {
			planID = strconv.FormatInt(*c.PlanID, 10)
		}
		_ = cw.Write([]string{
			csvSafe(c.Code), c.Kind, strconv.FormatInt(c.ValueMicros, 10), planID, csvSafe(c.PlanName), csvSafe(c.Batch),
			strconv.Itoa(c.MaxUses), strconv.Itoa(c.UsedCount), c.Status, formatTime(c.ExpiresAt), csvSafe(c.Note),
			c.CreatedAt.Format(time.RFC3339),
		})
	}
	cw.Flush()
}
