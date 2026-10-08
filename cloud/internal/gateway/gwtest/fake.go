package gwtest

import (
	"context"

	"forge-cloud/internal/core"
)

// FakePrincipal 实现 core.PrincipalResolver（网关/目录测试用）。
type FakePrincipal struct {
	APIKey string
	P      *core.Principal
}

func (f *FakePrincipal) AuthenticateAPIKey(_ context.Context, rawKey string) (*core.Principal, error) {
	if f.APIKey != "" && rawKey != f.APIKey {
		return nil, core.Unauthorized(core.CodeInvalidAPIKey, "API Key 无效")
	}
	if f.P == nil {
		return nil, core.Unauthorized(core.CodeInvalidAPIKey, "API Key 无效")
	}
	cp := *f.P
	return &cp, nil
}

func (f *FakePrincipal) PrincipalForUser(_ context.Context, userID int64) (*core.Principal, error) {
	if f.P != nil && f.P.UserID == userID {
		cp := *f.P
		return &cp, nil
	}
	return nil, core.NotFound("USER_NOT_FOUND", "用户不存在")
}

// FakeBiller 实现 core.Biller。
type FakeBiller struct {
	PrecheckErr error
	SettleCost  int64
	Records     []*core.UsageRecord
}

func (f *FakeBiller) Precheck(_ context.Context, _ *core.Principal, _ *core.Model) error {
	return f.PrecheckErr
}

func (f *FakeBiller) Settle(_ context.Context, rec *core.UsageRecord) (int64, error) {
	cp := *rec
	f.Records = append(f.Records, &cp)
	return f.SettleCost, nil
}

// DefaultPrincipal 返回默认分组上的调用方（限流/并发默认不限）。
func DefaultPrincipal() *core.Principal {
	return &core.Principal{
		UserID: 1, Email: "user@example.com", Status: "active",
		APIKeyID: 1, APIKeyName: "test", APIKeyKind: "user",
		UserConcurrency: 0, KeyConcurrency: 0,
		Group: core.Group{ID: 1, Name: "default", RateMultiplier: 1},
	}
}
