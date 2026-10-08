package billing

import (
	"context"
	"errors"
	"net/http"
	"strconv"
	"strings"
	"time"

	"github.com/jackc/pgx/v5"

	"forge-cloud/internal/core"
	"forge-cloud/internal/httpx"
)

const (
	redeemLimit  = 10
	redeemWindow = time.Minute
)

type PlanRef struct {
	ID   int64  `json:"id"`
	Name string `json:"name"`
}

// RedeemResult 对应 POST /me/redeem 的响应。
type RedeemResult struct {
	Kind          string        `json:"kind"`
	ValueMicros   int64         `json:"valueMicros"`
	Plan          *PlanRef      `json:"plan"`
	Subscription  *Subscription `json:"subscription"`
	BalanceMicros int64         `json:"balanceMicros"`
}

func errCodeInvalid() error { return core.NotFound("REDEEM_CODE_INVALID", "兑换码无效") }
func errCodeUsed() error {
	return core.Conflict("REDEEM_CODE_USED", "兑换码已被用尽或你已兑换过")
}

// Redeem 兑换一个码：balance 加余额并写流水 redeem；plan 从现在起新建订阅。
func (s *Service) Redeem(ctx context.Context, userID int64, code string) (*RedeemResult, error) {
	code = strings.TrimSpace(code)
	if code == "" || len(code) > 128 {
		return nil, errCodeInvalid()
	}
	var res *RedeemResult
	err := pgx.BeginFunc(ctx, s.db, func(tx pgx.Tx) error {
		var (
			id        int64
			kind      string
			value     int64
			planID    *int64
			maxUses   int
			usedCount int
			status    string
			expiresAt *time.Time
		)
		err := tx.QueryRow(ctx,
			`SELECT id, kind, value_micros, plan_id, max_uses, used_count, status, expires_at
			 FROM redeem_codes WHERE code = $1 FOR UPDATE`, code).
			Scan(&id, &kind, &value, &planID, &maxUses, &usedCount, &status, &expiresAt)
		if errors.Is(err, pgx.ErrNoRows) {
			return errCodeInvalid()
		}
		if err != nil {
			return err
		}
		switch {
		case status == "revoked":
			return errCodeInvalid()
		case kind == "invite":
			return core.BadRequest("REDEEM_CODE_INVALID", "邀请码只能在注册时使用")
		case expiresAt != nil && !expiresAt.After(time.Now()):
			return core.E(http.StatusGone, "REDEEM_CODE_EXPIRED", "兑换码已过期")
		case usedCount >= maxUses:
			return errCodeUsed()
		}
		var redeemed bool
		if err := tx.QueryRow(ctx,
			`SELECT EXISTS (SELECT 1 FROM redeem_records WHERE code_id = $1 AND user_id = $2)`, id, userID).Scan(&redeemed); err != nil {
			return err
		}
		if redeemed {
			return errCodeUsed()
		}

		out := &RedeemResult{Kind: kind, ValueMicros: value}
		switch kind {
		case "balance":
			after, err := AdjustBalance(ctx, tx, userID, value, "redeem", "redeem:"+strconv.FormatInt(id, 10), "兑换码 "+code)
			if err != nil {
				return err
			}
			out.BalanceMicros = after
		case "plan":
			if planID == nil {
				return errCodeInvalid()
			}
			var name string
			var quota int64
			err := tx.QueryRow(ctx, `SELECT name, quota_micros FROM plans WHERE id = $1`, *planID).Scan(&name, &quota)
			if errors.Is(err, pgx.ErrNoRows) {
				return errCodeInvalid()
			}
			if err != nil {
				return err
			}
			sub, err := CreateSubscriptionFrom(ctx, tx, userID, *planID, 0, SourceRedeem)
			if err != nil {
				return err
			}
			if out.ValueMicros == 0 {
				out.ValueMicros = quota
			}
			out.Plan = &PlanRef{ID: *planID, Name: name}
			out.Subscription = &sub
			if err := tx.QueryRow(ctx, `SELECT balance_micros FROM users WHERE id = $1`, userID).Scan(&out.BalanceMicros); err != nil {
				return err
			}
		default:
			return errCodeInvalid()
		}
		if _, err := tx.Exec(ctx, `UPDATE redeem_codes SET used_count = used_count + 1 WHERE id = $1`, id); err != nil {
			return err
		}
		if _, err := tx.Exec(ctx, `INSERT INTO redeem_records (code_id, user_id) VALUES ($1, $2)`, id, userID); err != nil {
			return err
		}
		res = out
		return nil
	})
	if err != nil {
		return nil, err
	}
	return res, nil
}

// allowRedeem：同一用户每分钟最多 10 次兑换尝试（Redis 固定窗口）；Redis 故障时放行。
func (s *Service) allowRedeem(ctx context.Context, userID int64) error {
	key := "rl:redeem:" + strconv.FormatInt(userID, 10)
	n, err := s.rdb.Incr(ctx, key).Result()
	if err != nil {
		s.log.Warn("redeem rate limit unavailable", "err", err)
		return nil
	}
	if n == 1 {
		_ = s.rdb.Expire(ctx, key, redeemWindow).Err()
	}
	if n > redeemLimit {
		e := core.E(http.StatusTooManyRequests, "TOO_MANY_ATTEMPTS", "兑换尝试过于频繁，请稍后再试")
		if ttl, err := s.rdb.TTL(ctx, key).Result(); err == nil && ttl > 0 {
			e.RetryAfter = int((ttl + time.Second - 1) / time.Second)
		} else {
			e.RetryAfter = int(redeemWindow / time.Second)
		}
		return e
	}
	return nil
}

func (s *Service) handleRedeem(w http.ResponseWriter, r *http.Request) {
	c, err := claims(r)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	var req struct {
		Code string `json:"code"`
	}
	if err := httpx.DecodeJSON(r, &req, 0); err != nil {
		httpx.WriteError(w, err)
		return
	}
	if err := s.allowRedeem(r.Context(), c.UserID); err != nil {
		httpx.WriteError(w, err)
		return
	}
	res, err := s.Redeem(r.Context(), c.UserID, req.Code)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	httpx.WriteJSON(w, http.StatusOK, res)
}
