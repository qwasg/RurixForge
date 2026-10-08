package auth

import (
	"context"
	"errors"
	"net/http"
	"strings"
	"time"

	"github.com/jackc/pgx/v5"

	"forge-cloud/internal/billing"
	"forge-cloud/internal/core"
	"forge-cloud/internal/httpx"
)

type registerRequest struct {
	Email          string     `json:"email"`
	Password       string     `json:"password"`
	Nickname       string     `json:"nickname"`
	InviteCode     string     `json:"inviteCode"`
	EmailCode      string     `json:"emailCode"`
	Device         DeviceInfo `json:"device"`
	IssueDeviceKey *bool      `json:"issueDeviceKey"`
}

func errInviteInvalid() error {
	return core.BadRequest("INVITE_CODE_INVALID", "邀请码无效或已用完")
}

// lockInviteCode 锁定并校验邀请码（kind=invite、未作废、未过期、未用尽），返回其 ID。
func lockInviteCode(ctx context.Context, tx pgx.Tx, code string) (int64, error) {
	var (
		id        int64
		kind      string
		status    string
		expiresAt *time.Time
		maxUses   int
		usedCount int
	)
	err := tx.QueryRow(ctx,
		`SELECT id, kind, status, expires_at, max_uses, used_count FROM redeem_codes WHERE code = $1 FOR UPDATE`, code).
		Scan(&id, &kind, &status, &expiresAt, &maxUses, &usedCount)
	if errors.Is(err, pgx.ErrNoRows) {
		return 0, errInviteInvalid()
	}
	if err != nil {
		return 0, err
	}
	if kind != "invite" || status != "active" || usedCount >= maxUses || (expiresAt != nil && !expiresAt.After(time.Now())) {
		return 0, errInviteInvalid()
	}
	return id, nil
}

func (s *Service) handleRegister(w http.ResponseWriter, r *http.Request) {
	var req registerRequest
	if err := httpx.DecodeJSON(r, &req, 0); err != nil {
		httpx.WriteError(w, err)
		return
	}
	ctx := r.Context()
	st, err := s.settings.Get(ctx)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	if st.RegistrationMode == "closed" {
		httpx.WriteError(w, core.Forbidden("REGISTRATION_CLOSED", "暂未开放注册"))
		return
	}
	email, err := NormalizeEmail(req.Email)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	if err := ValidatePassword(req.Password); err != nil {
		httpx.WriteError(w, err)
		return
	}
	nickname, err := ValidateNickname(req.Nickname)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	invite := strings.TrimSpace(req.InviteCode)
	needInvite := st.RegistrationMode == "invite"
	if needInvite && invite == "" {
		httpx.WriteError(w, core.BadRequest("INVITE_CODE_REQUIRED", "需要邀请码才能注册"))
		return
	}
	verifyEmail := st.RequireEmailVerify && s.cfg.SMTP.Enabled()
	if verifyEmail {
		if strings.TrimSpace(req.EmailCode) == "" {
			httpx.WriteError(w, core.BadRequest("EMAIL_CODE_REQUIRED", "请填写邮箱验证码"))
			return
		}
		if err := s.checkEmailCode(ctx, email, purposeRegister, req.EmailCode); err != nil {
			httpx.WriteError(w, err)
			return
		}
	}
	hash, err := HashPassword(req.Password)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	groupID, err := s.settings.DefaultGroupID(ctx)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	ip := httpx.ClientIP(r, s.cfg.TrustProxy)

	var (
		userID int64
		sess   *session
	)
	err = pgx.BeginFunc(ctx, s.db, func(tx pgx.Tx) error {
		var inviteID int64
		if needInvite {
			var err error
			if inviteID, err = lockInviteCode(ctx, tx, invite); err != nil {
				return err
			}
		}
		var err error
		userID, err = CreateUser(ctx, tx, NewUser{
			Email: email, PasswordHash: hash, Nickname: nickname, Role: "user", GroupID: groupID, EmailVerified: verifyEmail,
		})
		if err != nil {
			return err
		}
		if inviteID > 0 {
			if _, err := tx.Exec(ctx, `UPDATE redeem_codes SET used_count = used_count + 1 WHERE id = $1`, inviteID); err != nil {
				return err
			}
			if _, err := tx.Exec(ctx, `INSERT INTO redeem_records (code_id, user_id) VALUES ($1, $2)`, inviteID, userID); err != nil {
				return err
			}
		}
		if st.SignupBonusMicros > 0 {
			if _, err := billing.AdjustBalance(ctx, tx, userID, st.SignupBonusMicros, "signup_bonus", "", "注册赠送"); err != nil {
				return err
			}
		}
		if verifyEmail {
			if _, err := tx.Exec(ctx, `DELETE FROM email_codes WHERE email = $1 AND purpose = $2`, email, purposeRegister); err != nil {
				return err
			}
		}
		if sess, err = createSession(ctx, tx, userID, req.Device.normalized(), ip, issueKeyFlag(req.IssueDeviceKey)); err != nil {
			return err
		}
		_, err = tx.Exec(ctx, `UPDATE users SET last_login_at = now() WHERE id = $1`, userID)
		return err
	})
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	resp, err := s.buildLoginResponse(ctx, userID, sess)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	httpx.WriteJSON(w, http.StatusOK, resp)
}
