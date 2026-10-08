// Package usage 是 usage_logs 的共享查询（用户用量、管理端用量、日汇总），供 billing 与 admin 使用。
package usage

import (
	"context"
	"fmt"
	"strings"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
)

// Querier 是 *pgxpool.Pool 与 pgx.Tx 的公共子集。
type Querier interface {
	Exec(ctx context.Context, sql string, args ...any) (pgconn.CommandTag, error)
	Query(ctx context.Context, sql string, args ...any) (pgx.Rows, error)
	QueryRow(ctx context.Context, sql string, args ...any) pgx.Row
}

// Item 是用户可见的用量条目（不含上游账号等内部信息）。
type Item struct {
	ID               int64  `json:"id"`
	RequestID        string `json:"requestId"`
	Model            string `json:"model"`
	Endpoint         string `json:"endpoint"`
	Stream           bool   `json:"stream"`
	InputTokens      int64  `json:"inputTokens"`
	OutputTokens     int64  `json:"outputTokens"`
	CacheReadTokens  int64  `json:"cacheReadTokens"`
	CacheWriteTokens int64  `json:"cacheWriteTokens"`
	CostMicros       int64  `json:"costMicros"`
	// Pool 是计费用量池（api|forge）。
	Pool       string    `json:"pool"`
	Status     string    `json:"status"`
	ErrorCode  string    `json:"errorCode"`
	LatencyMs  int       `json:"latencyMs"`
	CreatedAt  time.Time `json:"createdAt"`
	APIKeyName string    `json:"apiKeyName"`
}

// AdminItem 是管理端用量条目。
type AdminItem struct {
	Item
	UserID        int64  `json:"userId"`
	UserEmail     string `json:"userEmail"`
	AccountID     *int64 `json:"accountId"`
	AccountName   string `json:"accountName"`
	UpstreamModel string `json:"upstreamModel"`
	HTTPStatus    int    `json:"httpStatus"`
	FirstTokenMs  int    `json:"firstTokenMs"`
	IP            string `json:"ip"`
}

type Summary struct {
	Requests         int64 `json:"requests"`
	InputTokens      int64 `json:"inputTokens"`
	OutputTokens     int64 `json:"outputTokens"`
	CacheReadTokens  int64 `json:"cacheReadTokens"`
	CacheWriteTokens int64 `json:"cacheWriteTokens"`
	CostMicros       int64 `json:"costMicros"`
}

// Day 是按 UTC 日期的汇总。
type Day struct {
	Date         string `json:"date"`
	Requests     int64  `json:"requests"`
	InputTokens  int64  `json:"inputTokens"`
	OutputTokens int64  `json:"outputTokens"`
	CostMicros   int64  `json:"costMicros"`
}

// Filter 为空字段表示不过滤；From 含、To 不含。
type Filter struct {
	UserID    int64
	AccountID int64
	Model     string
	Status    string
	From      time.Time
	To        time.Time
}

func (f Filter) where() (string, []any) {
	var conds []string
	var args []any
	add := func(cond string, v any) {
		args = append(args, v)
		conds = append(conds, fmt.Sprintf(cond, len(args)))
	}
	if f.UserID > 0 {
		add("l.user_id = $%d", f.UserID)
	}
	if f.AccountID > 0 {
		add("l.account_id = $%d", f.AccountID)
	}
	if f.Model != "" {
		add("l.model = $%d", f.Model)
	}
	if f.Status != "" {
		add("l.status = $%d", f.Status)
	}
	if !f.From.IsZero() {
		add("l.created_at >= $%d", f.From)
	}
	if !f.To.IsZero() {
		add("l.created_at < $%d", f.To)
	}
	if len(conds) == 0 {
		return "", nil
	}
	return " WHERE " + strings.Join(conds, " AND "), args
}

const itemCols = `l.id, l.request_id, l.model, l.endpoint, l.stream, l.input_tokens, l.output_tokens,
	l.cache_read_tokens, l.cache_write_tokens, l.cost_micros, l.pool, l.status, l.error_code, l.latency_ms, l.created_at,
	COALESCE(k.name, '')`

func (it *Item) dest() []any {
	return []any{&it.ID, &it.RequestID, &it.Model, &it.Endpoint, &it.Stream, &it.InputTokens, &it.OutputTokens,
		&it.CacheReadTokens, &it.CacheWriteTokens, &it.CostMicros, &it.Pool, &it.Status, &it.ErrorCode, &it.LatencyMs,
		&it.CreatedAt, &it.APIKeyName}
}

func count(ctx context.Context, q Querier, where string, args []any) (int64, error) {
	var n int64
	err := q.QueryRow(ctx, `SELECT count(*) FROM usage_logs l`+where, args...).Scan(&n)
	return n, err
}

// List 返回用户可见的用量条目（按时间倒序）与总数。
func List(ctx context.Context, q Querier, f Filter, limit, offset int) ([]Item, int64, error) {
	where, args := f.where()
	total, err := count(ctx, q, where, args)
	if err != nil {
		return nil, 0, err
	}
	sql := fmt.Sprintf(`SELECT %s FROM usage_logs l LEFT JOIN api_keys k ON k.id = l.api_key_id%s
		ORDER BY l.created_at DESC, l.id DESC LIMIT %d OFFSET %d`, itemCols, where, limit, offset)
	rows, err := q.Query(ctx, sql, args...)
	if err != nil {
		return nil, 0, err
	}
	defer rows.Close()
	out := []Item{}
	for rows.Next() {
		var it Item
		if err := rows.Scan(it.dest()...); err != nil {
			return nil, 0, err
		}
		it.CreatedAt = it.CreatedAt.UTC()
		out = append(out, it)
	}
	return out, total, rows.Err()
}

// ListAdmin 返回管理端用量条目（带用户邮箱与上游账号名）与总数。
func ListAdmin(ctx context.Context, q Querier, f Filter, limit, offset int) ([]AdminItem, int64, error) {
	where, args := f.where()
	total, err := count(ctx, q, where, args)
	if err != nil {
		return nil, 0, err
	}
	sql := fmt.Sprintf(`SELECT %s, l.user_id, COALESCE(u.email, ''), l.account_id, COALESCE(a.name, ''),
		l.upstream_model, l.http_status, l.first_token_ms, l.ip
		FROM usage_logs l
		LEFT JOIN api_keys k ON k.id = l.api_key_id
		LEFT JOIN users u ON u.id = l.user_id
		LEFT JOIN upstream_accounts a ON a.id = l.account_id%s
		ORDER BY l.created_at DESC, l.id DESC LIMIT %d OFFSET %d`, itemCols, where, limit, offset)
	rows, err := q.Query(ctx, sql, args...)
	if err != nil {
		return nil, 0, err
	}
	defer rows.Close()
	out := []AdminItem{}
	for rows.Next() {
		var it AdminItem
		dest := append(it.Item.dest(), &it.UserID, &it.UserEmail, &it.AccountID, &it.AccountName,
			&it.UpstreamModel, &it.HTTPStatus, &it.FirstTokenMs, &it.IP)
		if err := rows.Scan(dest...); err != nil {
			return nil, 0, err
		}
		it.CreatedAt = it.CreatedAt.UTC()
		out = append(out, it)
	}
	return out, total, rows.Err()
}

// Summarize 汇总满足条件的用量。
func Summarize(ctx context.Context, q Querier, f Filter) (Summary, error) {
	where, args := f.where()
	var s Summary
	err := q.QueryRow(ctx, `SELECT count(*), COALESCE(sum(l.input_tokens), 0), COALESCE(sum(l.output_tokens), 0),
		COALESCE(sum(l.cache_read_tokens), 0), COALESCE(sum(l.cache_write_tokens), 0), COALESCE(sum(l.cost_micros), 0)
		FROM usage_logs l`+where, args...).
		Scan(&s.Requests, &s.InputTokens, &s.OutputTokens, &s.CacheReadTokens, &s.CacheWriteTokens, &s.CostMicros)
	return s, err
}

// StartOfDayUTC 返回 t 所在 UTC 日的零点。
func StartOfDayUTC(t time.Time) time.Time {
	y, m, d := t.UTC().Date()
	return time.Date(y, m, d, 0, 0, 0, 0, time.UTC)
}

// ModelUsage 是按模型（与用量池）汇总的一行；IncludedMicros = 套餐内额度承担，OnDemandMicros = 余额承担。
type ModelUsage struct {
	Model            string `json:"model"`
	Pool             string `json:"pool"`
	Requests         int64  `json:"requests"`
	Errors           int64  `json:"errors"`
	InputTokens      int64  `json:"inputTokens"`
	OutputTokens     int64  `json:"outputTokens"`
	CacheReadTokens  int64  `json:"cacheReadTokens"`
	CacheWriteTokens int64  `json:"cacheWriteTokens"`
	CostMicros       int64  `json:"costMicros"`
	IncludedMicros   int64  `json:"includedMicros"`
	OnDemandMicros   int64  `json:"onDemandMicros"`
}

// ByModel 汇总用户在 [from, to) 内的用量（按费用倒序），并返回合计行（合计行 Model/Pool 为空）。
func ByModel(ctx context.Context, q Querier, userID int64, from, to time.Time) ([]ModelUsage, ModelUsage, error) {
	rows, err := q.Query(ctx, `SELECT l.model, l.pool, count(*), count(*) FILTER (WHERE l.status = 'error'),
		COALESCE(sum(l.input_tokens), 0), COALESCE(sum(l.output_tokens), 0),
		COALESCE(sum(l.cache_read_tokens), 0), COALESCE(sum(l.cache_write_tokens), 0),
		COALESCE(sum(l.cost_micros), 0), COALESCE(sum(l.charged_plan_micros), 0), COALESCE(sum(l.charged_balance_micros), 0)
		FROM usage_logs l WHERE l.user_id = $1 AND l.created_at >= $2 AND l.created_at < $3
		GROUP BY l.model, l.pool ORDER BY sum(l.cost_micros) DESC, l.model`, userID, from, to)
	if err != nil {
		return nil, ModelUsage{}, err
	}
	defer rows.Close()
	out := []ModelUsage{}
	var total ModelUsage
	for rows.Next() {
		var m ModelUsage
		if err := rows.Scan(&m.Model, &m.Pool, &m.Requests, &m.Errors, &m.InputTokens, &m.OutputTokens,
			&m.CacheReadTokens, &m.CacheWriteTokens, &m.CostMicros, &m.IncludedMicros, &m.OnDemandMicros); err != nil {
			return nil, ModelUsage{}, err
		}
		total.Requests += m.Requests
		total.Errors += m.Errors
		total.InputTokens += m.InputTokens
		total.OutputTokens += m.OutputTokens
		total.CacheReadTokens += m.CacheReadTokens
		total.CacheWriteTokens += m.CacheWriteTokens
		total.CostMicros += m.CostMicros
		total.IncludedMicros += m.IncludedMicros
		total.OnDemandMicros += m.OnDemandMicros
		out = append(out, m)
	}
	return out, total, rows.Err()
}

// OnDemandSpent 返回用户在 [from, to) 内由余额承担的费用（按量付费）。
func OnDemandSpent(ctx context.Context, q Querier, userID int64, from, to time.Time) (int64, error) {
	var v int64
	err := q.QueryRow(ctx, `SELECT COALESCE(sum(charged_balance_micros), 0) FROM usage_logs
		WHERE user_id = $1 AND charged_balance_micros > 0 AND created_at >= $2 AND created_at < $3`, userID, from, to).Scan(&v)
	return v, err
}

// Daily 返回截至今天（UTC）的最近 days 天汇总，缺失日期补 0；userID=0 表示全站。
func Daily(ctx context.Context, q Querier, userID int64, days int, now time.Time) ([]Day, error) {
	if days < 1 {
		days = 1
	}
	start := StartOfDayUTC(now).AddDate(0, 0, -(days - 1))
	f := Filter{UserID: userID, From: start}
	where, args := f.where()
	rows, err := q.Query(ctx, `SELECT to_char((l.created_at AT TIME ZONE 'UTC')::date, 'YYYY-MM-DD') AS d,
		count(*), COALESCE(sum(l.input_tokens), 0), COALESCE(sum(l.output_tokens), 0), COALESCE(sum(l.cost_micros), 0)
		FROM usage_logs l`+where+` GROUP BY d`, args...)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	byDate := map[string]Day{}
	for rows.Next() {
		var d Day
		if err := rows.Scan(&d.Date, &d.Requests, &d.InputTokens, &d.OutputTokens, &d.CostMicros); err != nil {
			return nil, err
		}
		byDate[d.Date] = d
	}
	if err := rows.Err(); err != nil {
		return nil, err
	}
	out := make([]Day, 0, days)
	for i := 0; i < days; i++ {
		date := start.AddDate(0, 0, i).Format("2006-01-02")
		d, ok := byDate[date]
		if !ok {
			d = Day{Date: date}
		}
		out = append(out, d)
	}
	return out, nil
}
