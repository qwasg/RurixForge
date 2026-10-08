package auth

import (
	"context"
	"crypto/hmac"
	"crypto/rand"
	"crypto/sha256"
	"crypto/subtle"
	"crypto/tls"
	"encoding/base64"
	"encoding/hex"
	"errors"
	"fmt"
	"math/big"
	"mime"
	"net"
	"net/http"
	"net/mail"
	"net/smtp"
	"strconv"
	"strings"
	"time"

	"github.com/jackc/pgx/v5"

	"forge-cloud/internal/core"
	"forge-cloud/internal/httpx"
)

const (
	purposeRegister = "register"
	purposeReset    = "reset"

	emailCodeTTL         = 10 * time.Minute
	emailCodeMaxAttempts = 5
	emailCodeInterval    = 60 * time.Second
)

func errSMTPNotConfigured() error {
	return core.E(http.StatusNotImplemented, "SMTP_NOT_CONFIGURED", "服务器未配置邮件发送")
}

func errEmailCodeInvalid() error {
	return core.BadRequest("EMAIL_CODE_INVALID", "验证码错误或已过期")
}

// codeHash 用 JWT 密钥做 HMAC：6 位数字空间太小，裸 SHA-256 可被离线穷举。
func (s *Service) codeHash(email, purpose, code string) string {
	m := hmac.New(sha256.New, s.cfg.JWTSecret)
	m.Write([]byte(email + "|" + purpose + "|" + strings.TrimSpace(code)))
	return hex.EncodeToString(m.Sum(nil))
}

func randomDigits(n int) string {
	var b strings.Builder
	for range n {
		v, err := rand.Int(rand.Reader, big.NewInt(10))
		if err != nil {
			panic(err)
		}
		b.WriteByte(byte('0' + v.Int64()))
	}
	return b.String()
}

// checkEmailCode 校验验证码（不消费）；错误时累加尝试次数，超过 5 次作废。
func (s *Service) checkEmailCode(ctx context.Context, email, purpose, code string) error {
	var (
		hash      string
		attempts  int
		expiresAt time.Time
	)
	err := s.db.QueryRow(ctx,
		`SELECT code_hash, attempts, expires_at FROM email_codes WHERE email = $1 AND purpose = $2`, email, purpose).
		Scan(&hash, &attempts, &expiresAt)
	if errors.Is(err, pgx.ErrNoRows) {
		return errEmailCodeInvalid()
	}
	if err != nil {
		return err
	}
	if attempts >= emailCodeMaxAttempts || !expiresAt.After(time.Now()) {
		return errEmailCodeInvalid()
	}
	if subtle.ConstantTimeCompare([]byte(hash), []byte(s.codeHash(email, purpose, code))) != 1 {
		if _, err := s.db.Exec(ctx,
			`UPDATE email_codes SET attempts = attempts + 1 WHERE email = $1 AND purpose = $2`, email, purpose); err != nil {
			return err
		}
		return errEmailCodeInvalid()
	}
	return nil
}

func (s *Service) handleEmailCode(w http.ResponseWriter, r *http.Request) {
	if !s.cfg.SMTP.Enabled() {
		httpx.WriteError(w, errSMTPNotConfigured())
		return
	}
	var req struct {
		Email   string `json:"email"`
		Purpose string `json:"purpose"`
	}
	if err := httpx.DecodeJSON(r, &req, 0); err != nil {
		httpx.WriteError(w, err)
		return
	}
	email, err := NormalizeEmail(req.Email)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	if req.Purpose != purposeRegister && req.Purpose != purposeReset {
		httpx.WriteError(w, core.BadRequest("INVALID_PURPOSE", "purpose 只能是 register 或 reset"))
		return
	}
	ctx := r.Context()
	rlKey := "rl:email:" + email
	if ok, err := s.rdb.SetNX(ctx, rlKey, 1, emailCodeInterval).Result(); err != nil {
		s.log.Warn("email code rate limit unavailable", "err", err)
	} else if !ok {
		ttl, _ := s.rdb.TTL(ctx, rlKey).Result()
		if ttl <= 0 {
			ttl = emailCodeInterval
		}
		httpx.WriteError(w, errTooManyAttempts(ttl))
		return
	}

	var exists bool
	if err := s.db.QueryRow(ctx, `SELECT EXISTS (SELECT 1 FROM users WHERE email = $1)`, email).Scan(&exists); err != nil {
		httpx.WriteError(w, err)
		return
	}
	if req.Purpose == purposeRegister && exists {
		httpx.WriteError(w, core.Conflict("EMAIL_TAKEN", "该邮箱已注册"))
		return
	}
	if req.Purpose == purposeReset && !exists {
		// 不暴露账号是否存在。
		httpx.OK(w)
		return
	}

	code := randomDigits(6)
	if _, err := s.db.Exec(ctx,
		`INSERT INTO email_codes (email, purpose, code_hash, attempts, expires_at, created_at)
		 VALUES ($1, $2, $3, 0, $4, now())
		 ON CONFLICT (email, purpose) DO UPDATE
		 SET code_hash = EXCLUDED.code_hash, attempts = 0, expires_at = EXCLUDED.expires_at, created_at = now()`,
		email, req.Purpose, s.codeHash(email, req.Purpose, code), time.Now().Add(emailCodeTTL)); err != nil {
		httpx.WriteError(w, err)
		return
	}
	st, _ := s.settings.Get(ctx)
	subject := st.SiteName + " 验证码"
	action := "注册账号"
	if req.Purpose == purposeReset {
		action = "重置密码"
	}
	body := fmt.Sprintf("你正在%s，验证码：%s\n\n验证码 %d 分钟内有效。如非本人操作，请忽略本邮件。\n", action, code, int(emailCodeTTL/time.Minute))
	if err := s.mailer(ctx, email, subject, body); err != nil {
		s.log.Warn("send email code failed", "err", err)
		_ = s.rdb.Del(ctx, rlKey).Err()
		httpx.WriteError(w, core.E(http.StatusBadGateway, "EMAIL_SEND_FAILED", "验证码邮件发送失败，请稍后再试"))
		return
	}
	httpx.OK(w)
}

func (s *Service) handlePasswordReset(w http.ResponseWriter, r *http.Request) {
	if !s.cfg.SMTP.Enabled() {
		httpx.WriteError(w, errSMTPNotConfigured())
		return
	}
	var req struct {
		Email       string `json:"email"`
		Code        string `json:"code"`
		NewPassword string `json:"newPassword"`
	}
	if err := httpx.DecodeJSON(r, &req, 0); err != nil {
		httpx.WriteError(w, err)
		return
	}
	email, err := NormalizeEmail(req.Email)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	if err := ValidatePassword(req.NewPassword); err != nil {
		httpx.WriteError(w, err)
		return
	}
	ctx := r.Context()
	if err := s.checkEmailCode(ctx, email, purposeReset, req.Code); err != nil {
		httpx.WriteError(w, err)
		return
	}
	hash, err := HashPassword(req.NewPassword)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	err = pgx.BeginFunc(ctx, s.db, func(tx pgx.Tx) error {
		var id int64
		err := tx.QueryRow(ctx,
			`UPDATE users SET password_hash = $2, updated_at = now() WHERE email = $1 RETURNING id`, email, hash).Scan(&id)
		if errors.Is(err, pgx.ErrNoRows) {
			return errEmailCodeInvalid()
		}
		if err != nil {
			return err
		}
		if _, err := tx.Exec(ctx, `DELETE FROM email_codes WHERE email = $1 AND purpose = $2`, email, purposeReset); err != nil {
			return err
		}
		return RevokeUserSessions(ctx, tx, id, "")
	})
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	httpx.OK(w)
}

// sendSMTP 通过 net/smtp 发信：465 端口走隐式 TLS，其它端口在服务器支持时 STARTTLS。
func (s *Service) sendSMTP(ctx context.Context, to, subject, body string) error {
	c := s.cfg.SMTP
	from, err := mail.ParseAddress(c.From)
	if err != nil {
		return fmt.Errorf("FORGE_CLOUD_SMTP_FROM 非法: %w", err)
	}
	addr := net.JoinHostPort(c.Host, strconv.Itoa(c.Port))
	tlsCfg := &tls.Config{ServerName: c.Host, MinVersion: tls.VersionTLS12}
	dialer := &net.Dialer{Timeout: 10 * time.Second}
	var conn net.Conn
	if c.Port == 465 {
		conn, err = (&tls.Dialer{NetDialer: dialer, Config: tlsCfg}).DialContext(ctx, "tcp", addr)
	} else {
		conn, err = dialer.DialContext(ctx, "tcp", addr)
	}
	if err != nil {
		return err
	}
	_ = conn.SetDeadline(time.Now().Add(30 * time.Second))
	client, err := smtp.NewClient(conn, c.Host)
	if err != nil {
		_ = conn.Close()
		return err
	}
	defer client.Close()
	if c.Port != 465 {
		if ok, _ := client.Extension("STARTTLS"); ok {
			if err := client.StartTLS(tlsCfg); err != nil {
				return err
			}
		}
	}
	if c.User != "" {
		if err := client.Auth(smtp.PlainAuth("", c.User, c.Password, c.Host)); err != nil {
			return err
		}
	}
	if err := client.Mail(from.Address); err != nil {
		return err
	}
	if err := client.Rcpt(to); err != nil {
		return err
	}
	wc, err := client.Data()
	if err != nil {
		return err
	}
	if _, err := wc.Write(buildMessage(c.From, to, subject, body)); err != nil {
		_ = wc.Close()
		return err
	}
	if err := wc.Close(); err != nil {
		return err
	}
	return client.Quit()
}

func buildMessage(from, to, subject, body string) []byte {
	var b strings.Builder
	b.WriteString("From: " + from + "\r\n")
	b.WriteString("To: " + to + "\r\n")
	b.WriteString("Subject: " + mime.BEncoding.Encode("UTF-8", subject) + "\r\n")
	b.WriteString("Date: " + time.Now().Format(time.RFC1123Z) + "\r\n")
	b.WriteString("MIME-Version: 1.0\r\nContent-Type: text/plain; charset=UTF-8\r\nContent-Transfer-Encoding: base64\r\n\r\n")
	enc := base64.StdEncoding.EncodeToString([]byte(body))
	for len(enc) > 76 {
		b.WriteString(enc[:76] + "\r\n")
		enc = enc[76:]
	}
	b.WriteString(enc + "\r\n")
	return []byte(b.String())
}
