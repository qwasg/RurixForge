package apikeys

import (
	"context"
	"strings"
	"time"

	"forge-cloud/internal/core"
)

// principalUserCols 读取用户与「生效订阅的套餐分组」（多个取最晚到期）。
const principalUserCols = `u.id, u.email, u.role, u.status, u.balance_micros, u.concurrency_override, u.group_id,
	(SELECT s.group_id FROM subscriptions s
	  WHERE s.user_id = u.id AND s.status = 'active' AND s.group_id IS NOT NULL
	    AND s.starts_at <= now() AND s.ends_at > now()
	  ORDER BY s.ends_at DESC LIMIT 1)`

type principalUser struct {
	id                  int64
	email               string
	role                string
	status              string
	balance             int64
	concurrencyOverride *int
	groupID             *int64
	subGroupID          *int64
}

func (u *principalUser) dest() []any {
	return []any{&u.id, &u.email, &u.role, &u.status, &u.balance, &u.concurrencyOverride, &u.groupID, &u.subGroupID}
}

func errInvalidKey() error {
	return core.Unauthorized(core.CodeInvalidAPIKey, "API Key 无效、已吊销或已过期")
}

func errUserDisabled() error {
	return core.Forbidden(core.CodeUserDisabled, "账号已被禁用")
}

// AuthenticateAPIKey 校验平台 Key。设备 Key 还要求其登录会话仍有效（未吊销、未过期）。
func (s *Service) AuthenticateAPIKey(ctx context.Context, rawKey string) (*core.Principal, error) {
	rawKey = strings.TrimSpace(rawKey)
	if !strings.HasPrefix(rawKey, KeyPrefix) || len(rawKey) > 128 {
		return nil, errInvalidKey()
	}
	var (
		keyID     int64
		keyName   string
		kind      string
		status    string
		quota     int64
		used      int64
		keyConc   int
		expiresAt *time.Time
		sessionOK bool
		u         principalUser
	)
	dest := append([]any{&keyID, &keyName, &kind, &status, &quota, &used, &keyConc, &expiresAt, &sessionOK}, u.dest()...)
	err := s.db.QueryRow(ctx,
		`SELECT k.id, k.name, k.kind, k.status, k.quota_micros, k.used_micros, k.concurrency_limit, k.expires_at,
		        (k.kind = 'user' OR EXISTS (SELECT 1 FROM refresh_sessions rs
		           WHERE rs.id = k.session_id AND rs.revoked_at IS NULL AND rs.expires_at > now())),
		        `+principalUserCols+`
		 FROM api_keys k JOIN users u ON u.id = k.user_id
		 WHERE k.key_hash = $1`, core.SHA256Hex(rawKey)).Scan(dest...)
	if isNoRows(err) {
		return nil, errInvalidKey()
	}
	if err != nil {
		return nil, err
	}
	if status != "active" || !sessionOK || (expiresAt != nil && !expiresAt.After(time.Now())) {
		return nil, errInvalidKey()
	}
	if u.status != "active" {
		return nil, errUserDisabled()
	}
	p, err := s.buildPrincipal(ctx, &u)
	if err != nil {
		return nil, err
	}
	p.APIKeyID = keyID
	p.APIKeyName = keyName
	p.APIKeyKind = kind
	p.KeyQuota = quota
	p.KeyUsed = used
	p.KeyConcurrency = keyConc
	s.touch(ctx, keyID)
	return p, nil
}

// PrincipalForUser 为 JWT 用户构造 Principal（APIKeyID=0），用于模型目录。
func (s *Service) PrincipalForUser(ctx context.Context, userID int64) (*core.Principal, error) {
	var u principalUser
	err := s.db.QueryRow(ctx, `SELECT `+principalUserCols+` FROM users u WHERE u.id = $1`, userID).Scan(u.dest()...)
	if isNoRows(err) {
		return nil, core.Unauthorized("UNAUTHORIZED", "用户不存在")
	}
	if err != nil {
		return nil, err
	}
	if u.status != "active" {
		return nil, errUserDisabled()
	}
	return s.buildPrincipal(ctx, &u)
}

func (s *Service) buildPrincipal(ctx context.Context, u *principalUser) (*core.Principal, error) {
	g, err := s.effectiveGroup(ctx, u)
	if err != nil {
		return nil, err
	}
	conc := g.ConcurrencyLimit
	if u.concurrencyOverride != nil {
		conc = *u.concurrencyOverride
	}
	return &core.Principal{
		UserID:          u.id,
		Email:           u.email,
		Role:            u.role,
		Status:          u.status,
		UserConcurrency: conc,
		BalanceMicros:   u.balance,
		Group:           g,
	}, nil
}

// effectiveGroup：生效订阅的套餐分组 > 用户分组 > 默认分组（设置指定的，否则 is_default）。
func (s *Service) effectiveGroup(ctx context.Context, u *principalUser) (core.Group, error) {
	for _, id := range []*int64{u.subGroupID, u.groupID} {
		if id == nil {
			continue
		}
		g, err := loadGroup(ctx, s.db, `WHERE id = $1`, *id)
		if err == nil {
			return g, nil
		}
		if !isNoRows(err) {
			return core.Group{}, err
		}
	}
	def, err := s.settings.DefaultGroupID(ctx)
	if err != nil {
		return core.Group{}, err
	}
	if def > 0 {
		g, err := loadGroup(ctx, s.db, `WHERE id = $1`, def)
		if err == nil {
			return g, nil
		}
		if !isNoRows(err) {
			return core.Group{}, err
		}
	}
	g, err := loadGroup(ctx, s.db, `WHERE is_default LIMIT 1`)
	if isNoRows(err) {
		return core.Group{RateMultiplier: 1}, nil
	}
	return g, err
}

func loadGroup(ctx context.Context, q Querier, where string, args ...any) (core.Group, error) {
	var g core.Group
	err := q.QueryRow(ctx,
		`SELECT id, name, rate_multiplier, concurrency_limit, rpm_limit, tpm_limit, allowed_models, is_default
		 FROM groups `+where, args...).
		Scan(&g.ID, &g.Name, &g.RateMultiplier, &g.ConcurrencyLimit, &g.RPMLimit, &g.TPMLimit, &g.AllowedModels, &g.IsDefault)
	if g.AllowedModels == nil {
		g.AllowedModels = []string{}
	}
	return g, err
}
