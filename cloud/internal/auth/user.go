package auth

import (
	"context"
	"errors"
	"time"

	"github.com/jackc/pgx/v5"

	"forge-cloud/internal/core"
)

// User 是用户的对外形状（§3.1）。
type User struct {
	ID            int64     `json:"id"`
	Email         string    `json:"email"`
	Nickname      string    `json:"nickname"`
	Role          string    `json:"role"`
	Status        string    `json:"status"`
	HasAvatar     bool      `json:"hasAvatar"`
	AvatarVersion int64     `json:"avatarVersion"`
	GroupID       *int64    `json:"groupId"`
	GroupName     string    `json:"groupName"`
	BalanceMicros int64     `json:"balanceMicros"`
	CreatedAt     time.Time `json:"createdAt"`
}

// AdminUser 是管理端的用户形状（User + lastLoginAt、concurrencyOverride）。
type AdminUser struct {
	User
	LastLoginAt         *time.Time `json:"lastLoginAt"`
	ConcurrencyOverride *int       `json:"concurrencyOverride"`
}

// UserSelect 是 AdminUser 的查询前缀（别名 u，配合 ScanAdminUser）。
const UserSelect = `SELECT u.id, u.email, u.nickname, u.role, u.status, u.avatar_updated_at, u.group_id,
	COALESCE(g.name, ''), u.balance_micros, u.created_at, u.last_login_at, u.concurrency_override
	FROM users u LEFT JOIN groups g ON g.id = u.group_id`

// ScanAdminUser 扫描一行 UserSelect 结果。
func ScanAdminUser(row pgx.Row) (AdminUser, error) {
	var u AdminUser
	var avatarAt *time.Time
	if err := row.Scan(&u.ID, &u.Email, &u.Nickname, &u.Role, &u.Status, &avatarAt, &u.GroupID, &u.GroupName,
		&u.BalanceMicros, &u.CreatedAt, &u.LastLoginAt, &u.ConcurrencyOverride); err != nil {
		return u, err
	}
	if avatarAt != nil {
		u.HasAvatar = true
		u.AvatarVersion = avatarAt.Unix()
	}
	u.CreatedAt = u.CreatedAt.UTC()
	if u.LastLoginAt != nil {
		t := u.LastLoginAt.UTC()
		u.LastLoginAt = &t
	}
	return u, nil
}

// LoadAdminUser 读取用户；不存在 → 404 USER_NOT_FOUND。
func LoadAdminUser(ctx context.Context, q Querier, id int64) (AdminUser, error) {
	u, err := ScanAdminUser(q.QueryRow(ctx, UserSelect+` WHERE u.id = $1`, id))
	if errors.Is(err, pgx.ErrNoRows) {
		return u, core.NotFound("USER_NOT_FOUND", "用户不存在")
	}
	return u, err
}

// LoadUser 读取用户的对外形状。
func LoadUser(ctx context.Context, q Querier, id int64) (User, error) {
	u, err := LoadAdminUser(ctx, q, id)
	return u.User, err
}

// NewUser 是新建用户的参数（GroupID=0 表示不设分组）。
type NewUser struct {
	Email         string
	PasswordHash  string
	Nickname      string
	Role          string
	GroupID       int64
	EmailVerified bool
}

// CreateUser 插入用户；邮箱已存在 → 409 EMAIL_TAKEN（不会中断调用方事务）。
func CreateUser(ctx context.Context, q Querier, nu NewUser) (int64, error) {
	role := nu.Role
	if role == "" {
		role = "user"
	}
	var group any
	if nu.GroupID > 0 {
		group = nu.GroupID
	}
	var id int64
	err := q.QueryRow(ctx,
		`INSERT INTO users (email, password_hash, nickname, role, group_id, email_verified)
		 VALUES ($1, $2, $3, $4, $5, $6)
		 ON CONFLICT (email) DO NOTHING RETURNING id`,
		nu.Email, nu.PasswordHash, nu.Nickname, role, group, nu.EmailVerified).Scan(&id)
	if errors.Is(err, pgx.ErrNoRows) {
		return 0, core.Conflict("EMAIL_TAKEN", "该邮箱已注册")
	}
	return id, err
}
