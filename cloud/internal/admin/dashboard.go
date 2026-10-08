package admin

import (
	"net/http"
	"time"

	"forge-cloud/internal/httpx"
	"forge-cloud/internal/usage"
)

type dashboardUsers struct {
	Total    int64 `json:"total"`
	Active7d int64 `json:"active7d"`
}

type dashboardAccounts struct {
	Total       int64 `json:"total"`
	Active      int64 `json:"active"`
	CoolingDown int64 `json:"coolingDown"`
	Error       int64 `json:"error"`
}

type dashboardToday struct {
	Requests     int64 `json:"requests"`
	InputTokens  int64 `json:"inputTokens"`
	OutputTokens int64 `json:"outputTokens"`
	CostMicros   int64 `json:"costMicros"`
	Errors       int64 `json:"errors"`
}

type topModel struct {
	Model      string `json:"model"`
	Requests   int64  `json:"requests"`
	CostMicros int64  `json:"costMicros"`
}

type dashboard struct {
	Users     dashboardUsers    `json:"users"`
	Accounts  dashboardAccounts `json:"accounts"`
	Today     dashboardToday    `json:"today"`
	Daily     []usage.Day       `json:"daily"`
	TopModels []topModel        `json:"topModels"`
	Currency  string            `json:"currency"`
}

// handleDashboard：active7d = 7 天内登录过或有用量；today/daily 按 UTC 日；topModels 取近 24 小时。
func (s *Service) handleDashboard(w http.ResponseWriter, r *http.Request) {
	ctx := r.Context()
	db := s.d.DB
	now := time.Now()
	var out dashboard
	if err := db.QueryRow(ctx,
		`SELECT count(*), count(*) FILTER (WHERE u.last_login_at > now() - interval '7 days'
		        OR EXISTS (SELECT 1 FROM usage_logs l WHERE l.user_id = u.id AND l.created_at > now() - interval '7 days'))
		 FROM users u`).Scan(&out.Users.Total, &out.Users.Active7d); err != nil {
		httpx.WriteError(w, err)
		return
	}
	if err := db.QueryRow(ctx,
		`SELECT count(*),
		        count(*) FILTER (WHERE status = 'active' AND (cooldown_until IS NULL OR cooldown_until <= now())),
		        count(*) FILTER (WHERE cooldown_until > now()),
		        count(*) FILTER (WHERE status = 'error')
		 FROM upstream_accounts`).Scan(&out.Accounts.Total, &out.Accounts.Active, &out.Accounts.CoolingDown, &out.Accounts.Error); err != nil {
		httpx.WriteError(w, err)
		return
	}
	if err := db.QueryRow(ctx,
		`SELECT count(*), COALESCE(sum(input_tokens), 0), COALESCE(sum(output_tokens), 0), COALESCE(sum(cost_micros), 0),
		        count(*) FILTER (WHERE status = 'error')
		 FROM usage_logs WHERE created_at >= $1`, usage.StartOfDayUTC(now)).
		Scan(&out.Today.Requests, &out.Today.InputTokens, &out.Today.OutputTokens, &out.Today.CostMicros, &out.Today.Errors); err != nil {
		httpx.WriteError(w, err)
		return
	}
	daily, err := usage.Daily(ctx, db, 0, 14, now)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	out.Daily = daily
	rows, err := db.Query(ctx,
		`SELECT model, count(*) AS n, COALESCE(sum(cost_micros), 0) FROM usage_logs
		 WHERE created_at > now() - interval '24 hours'
		 GROUP BY model ORDER BY n DESC, model LIMIT 10`)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	defer rows.Close()
	out.TopModels = []topModel{}
	for rows.Next() {
		var m topModel
		if err := rows.Scan(&m.Model, &m.Requests, &m.CostMicros); err != nil {
			httpx.WriteError(w, err)
			return
		}
		out.TopModels = append(out.TopModels, m)
	}
	if err := rows.Err(); err != nil {
		httpx.WriteError(w, err)
		return
	}
	st, err := s.d.Settings.Get(ctx)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	out.Currency = st.Currency
	httpx.WriteJSON(w, http.StatusOK, out)
}
