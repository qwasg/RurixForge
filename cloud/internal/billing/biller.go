package billing

import (
	"context"
	"errors"
	"net/http"
	"time"

	"github.com/jackc/pgx/v5"

	"forge-cloud/internal/core"
	"forge-cloud/internal/usage"
)

func poolOf(m *core.Model) string {
	if m == nil {
		return core.PoolAPI
	}
	return core.NormalizePool(m.Pool)
}

// Precheck（§5、§11.1、§13）：用户 active；Key 有额度上限时 used < quota；零定价模型 → 放行；
// 收费模型所在用量池还有套餐内额度 → 放行；
// 否则看按量付费：关闭 → INCLUDED_USAGE_EXHAUSTED；余额 ≤ 0 → INSUFFICIENT_BALANCE；
// 设了每周期上限且已用满 → SPEND_LIMIT_REACHED（均 402）。
func (s *Service) Precheck(ctx context.Context, p *core.Principal, m *core.Model) error {
	if p == nil {
		return core.Unauthorized(core.CodeInvalidAPIKey, "未鉴权")
	}
	st, err := loadBillingState(ctx, s.db, p.UserID, time.Now(), false)
	if errors.Is(err, errBillingUserNotFound) {
		return core.Unauthorized(core.CodeInvalidAPIKey, "用户不存在")
	}
	if err != nil {
		return err
	}
	if st.status != "active" {
		return core.Forbidden(core.CodeUserDisabled, "账号已被禁用")
	}
	if p.KeyQuota > 0 && p.KeyUsed >= p.KeyQuota {
		return core.E(http.StatusPaymentRequired, core.CodeKeyQuotaExceeded, "该 API Key 的额度已用完")
	}
	// 所有 token 单价均为 0 时不会产生费用，不要求余额、套餐额度或按量付费预算。
	// 账号状态与 Key 额度仍先检查；缺失模型或任一非零单价继续走收费预检。
	if m != nil && m.Pricing == (core.Pricing{}) {
		return nil
	}
	if st.hasIncluded(poolOf(m)) {
		return nil
	}
	if !st.onDemand {
		return core.E(http.StatusPaymentRequired, core.CodeIncludedUsageExhausted,
			"本周期套餐内额度已用完；可在「套餐与用量」开启按量付费或升级套餐")
	}
	if st.balance <= 0 {
		return core.E(http.StatusPaymentRequired, core.CodeInsufficientBalance, "余额与套餐额度不足，请充值或兑换")
	}
	if st.onDemandLimit > 0 {
		spent, err := usage.OnDemandSpent(ctx, s.db, p.UserID, st.cycleStart, st.cycleEnd)
		if err != nil {
			return err
		}
		if spent >= st.onDemandLimit {
			return core.E(http.StatusPaymentRequired, core.CodeSpendLimitReached,
				"本周期按量付费已达上限；可在「套餐与用量」调高上限")
		}
	}
	return nil
}

// Settle 单事务结算：锁用户行与生效订阅 → 惰性过期 → 按到期先后扣模型所在用量池的套餐内额度
// （含 Hobby 免费额度，受每日上限约束）→ 余下按 §11.1 扣余额（允许扣成负数）→ 写 usage_logs →
// 余额部分写流水 → 累加 Key 用量。status=error 的记录费用为 0，只写 usage_logs。
func (s *Service) Settle(ctx context.Context, rec *core.UsageRecord) (int64, error) {
	if rec == nil || rec.Principal == nil {
		return 0, errors.New("billing: 结算记录缺少调用方")
	}
	p := rec.Principal
	status := "ok"
	if rec.Status == "error" {
		status = "error"
	}
	var modelID string
	var cost int64
	pool := poolOf(rec.Model)
	if rec.Model != nil {
		modelID = rec.Model.ID
		if status == "ok" {
			cost = core.ComputeCost(rec.Model.Pricing, rec.Usage, p.Group.RateMultiplier)
		}
	}
	endpoint := rec.Endpoint
	if endpoint == "" {
		endpoint = "unknown"
	}

	now := time.Now()
	var planPart, balancePart int64
	err := pgx.BeginFunc(ctx, s.db, func(tx pgx.Tx) error {
		if cost > 0 {
			st, err := loadBillingState(ctx, tx, p.UserID, now, true)
			if errors.Is(err, errBillingUserNotFound) {
				return core.NotFound("USER_NOT_FOUND", "用户不存在")
			}
			if err != nil {
				return err
			}
			if _, err := tx.Exec(ctx,
				`UPDATE subscriptions SET status = 'expired' WHERE user_id = $1 AND status = 'active' AND ends_at <= $2`,
				p.UserID, now); err != nil {
				return err
			}
			planPart, balancePart = st.consume(pool, cost)
			if err := st.save(ctx, tx, p.UserID); err != nil {
				return err
			}
		}
		if _, err := tx.Exec(ctx,
			`INSERT INTO usage_logs (request_id, user_id, api_key_id, account_id, model, upstream_model, endpoint, stream,
				input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, cost_micros,
				charged_balance_micros, charged_plan_micros, rate_multiplier, status, error_code, http_status,
				latency_ms, first_token_ms, session_key, ip, pool)
			 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23, $24)`,
			rec.RequestID, p.UserID, nullID(p.APIKeyID), nullID(rec.AccountID), modelID, rec.UpstreamModel, endpoint, rec.Stream,
			rec.Usage.InputTokens, rec.Usage.OutputTokens, rec.Usage.CacheReadTokens, rec.Usage.CacheWriteTokens, cost,
			balancePart, planPart, p.Group.RateMultiplier, status, rec.ErrorCode, rec.HTTPStatus,
			rec.LatencyMs, rec.FirstTokenMs, rec.SessionKey, rec.IP, pool); err != nil {
			return err
		}
		if balancePart > 0 {
			if _, err := AdjustBalance(ctx, tx, p.UserID, -balancePart, "usage", rec.RequestID, modelID); err != nil {
				return err
			}
		}
		if cost > 0 && p.APIKeyID > 0 {
			if _, err := tx.Exec(ctx, `UPDATE api_keys SET used_micros = used_micros + $2 WHERE id = $1`, p.APIKeyID, cost); err != nil {
				return err
			}
		}
		return nil
	})
	if err != nil {
		return 0, err
	}
	return cost, nil
}

func nullID(id int64) any {
	if id <= 0 {
		return nil
	}
	return id
}
