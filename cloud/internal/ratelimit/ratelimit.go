// Package ratelimit：基于 Redis 的网关限流（15_CLOUD_SERVICE.md §4）。
//
//   - RPM：每用户固定 60 秒窗口（INCR + EXPIRE），键 `rpm:user:<id>`；
//   - TPM：每用户每分钟一个桶，结算后按 usage.Total() 累加，预检比较当前分钟与分组上限，
//     键 `tpm:user:<id>:<unix 分钟>`；
//   - 并发槽：有序集合，member = 请求 ID，score = 租约到期（毫秒），租约 15 分钟；
//     占槽由 Lua 原子完成（清理过期 → 判满 → 加入）。键 `slot:user:<id>`、`slot:key:<id>`、`slot:acct:<id>`。
package ratelimit

import (
	"context"
	"errors"
	"fmt"
	"strconv"
	"time"

	"github.com/redis/go-redis/v9"
)

// SlotLease 是并发槽租约；长流式请求需周期性 Extend。
const SlotLease = 15 * time.Minute

func UserSlotKey(userID int64) string       { return "slot:user:" + strconv.FormatInt(userID, 10) }
func KeySlotKey(apiKeyID int64) string      { return "slot:key:" + strconv.FormatInt(apiKeyID, 10) }
func AccountSlotKey(accountID int64) string { return "slot:acct:" + strconv.FormatInt(accountID, 10) }

func rpmKey(userID int64) string { return "rpm:user:" + strconv.FormatInt(userID, 10) }

func tpmKey(userID int64, minute int64) string {
	return fmt.Sprintf("tpm:user:%d:%d", userID, minute)
}

// acquireScript：KEYS[1]=槽集合；ARGV = now(ms)、limit、member、到期(ms)、键 TTL(ms)。
// limit<=0 表示不限（仍登记，供负载率与后台展示）；同一 member 重复占槽视为续租。
var acquireScript = redis.NewScript(`
local key = KEYS[1]
redis.call('ZREMRANGEBYSCORE', key, '-inf', ARGV[1])
local limit = tonumber(ARGV[2])
if limit > 0 and not redis.call('ZSCORE', key, ARGV[3]) then
  if redis.call('ZCARD', key) >= limit then
    return 0
  end
end
redis.call('ZADD', key, ARGV[4], ARGV[3])
redis.call('PEXPIRE', key, ARGV[5])
return 1
`)

// rpmScript：KEYS[1]=计数键；ARGV[1]=窗口秒数。返回 {当前计数, 剩余秒数}。
var rpmScript = redis.NewScript(`
local n = redis.call('INCR', KEYS[1])
local ttl = redis.call('TTL', KEYS[1])
if ttl < 0 then
  redis.call('EXPIRE', KEYS[1], ARGV[1])
  ttl = tonumber(ARGV[1])
end
return {n, ttl}
`)

type Limiter struct {
	rdb *redis.Client
	now func() time.Time
}

func New(rdb *redis.Client) *Limiter { return &Limiter{rdb: rdb, now: time.Now} }

// SetClock 替换时钟（测试用）。
func (l *Limiter) SetClock(now func() time.Time) { l.now = now }

// ---------- 并发槽 ----------

// Acquire 原子占槽：limit>0 且未过期成员数已达 limit 时返回 false。
func (l *Limiter) Acquire(ctx context.Context, key string, limit int, member string) (bool, error) {
	now := l.now()
	res, err := acquireScript.Run(ctx, l.rdb, []string{key},
		now.UnixMilli(), limit, member, now.Add(SlotLease).UnixMilli(), (SlotLease + time.Minute).Milliseconds(),
	).Int()
	if err != nil {
		return false, err
	}
	return res == 1, nil
}

// Release 释放槽（成员不存在时无副作用）。
func (l *Limiter) Release(ctx context.Context, key, member string) error {
	return l.rdb.ZRem(ctx, key, member).Err()
}

// Extend 为仍持有的槽续租。
func (l *Limiter) Extend(ctx context.Context, key, member string) error {
	now := l.now()
	pipe := l.rdb.Pipeline()
	pipe.ZAddXX(ctx, key, redis.Z{Score: float64(now.Add(SlotLease).UnixMilli()), Member: member})
	pipe.PExpire(ctx, key, SlotLease+time.Minute)
	_, err := pipe.Exec(ctx)
	return err
}

// Count 返回未过期的占槽数。
func (l *Limiter) Count(ctx context.Context, key string) (int, error) {
	counts, err := l.Counts(ctx, []string{key})
	if err != nil {
		return 0, err
	}
	return counts[0], nil
}

// Counts 批量返回未过期的占槽数（与 keys 一一对应）。
func (l *Limiter) Counts(ctx context.Context, keys []string) ([]int, error) {
	out := make([]int, len(keys))
	if len(keys) == 0 {
		return out, nil
	}
	min := "(" + strconv.FormatInt(l.now().UnixMilli(), 10)
	pipe := l.rdb.Pipeline()
	cmds := make([]*redis.IntCmd, len(keys))
	for i, k := range keys {
		cmds[i] = pipe.ZCount(ctx, k, min, "+inf")
	}
	if _, err := pipe.Exec(ctx); err != nil && !errors.Is(err, redis.Nil) {
		return nil, err
	}
	for i, c := range cmds {
		out[i] = int(c.Val())
	}
	return out, nil
}

// ---------- RPM / TPM ----------

// CheckRPM 计入一次请求；超过 limit 返回 ok=false 与窗口剩余秒数。limit<=0 不限。
func (l *Limiter) CheckRPM(ctx context.Context, userID int64, limit int) (ok bool, retryAfter int, err error) {
	if limit <= 0 {
		return true, 0, nil
	}
	vals, err := rpmScript.Run(ctx, l.rdb, []string{rpmKey(userID)}, 60).Int64Slice()
	if err != nil {
		return false, 0, err
	}
	if len(vals) != 2 {
		return false, 0, errors.New("ratelimit: rpm 脚本返回值异常")
	}
	if vals[0] > int64(limit) {
		return false, max(int(vals[1]), 1), nil
	}
	return true, 0, nil
}

// CheckTPM 比较当前分钟桶与 limit；已达上限返回 ok=false 与距下一分钟的秒数。limit<=0 不限。
func (l *Limiter) CheckTPM(ctx context.Context, userID int64, limit int) (ok bool, retryAfter int, err error) {
	if limit <= 0 {
		return true, 0, nil
	}
	now := l.now()
	used, err := l.rdb.Get(ctx, tpmKey(userID, now.Unix()/60)).Int64()
	if errors.Is(err, redis.Nil) {
		return true, 0, nil
	}
	if err != nil {
		return false, 0, err
	}
	if used >= int64(limit) {
		return false, max(60-int(now.Unix()%60), 1), nil
	}
	return true, 0, nil
}

// AddTokens 把一次请求的 token 数累加进当前分钟桶。
func (l *Limiter) AddTokens(ctx context.Context, userID int64, n int64) error {
	if n <= 0 {
		return nil
	}
	key := tpmKey(userID, l.now().Unix()/60)
	pipe := l.rdb.TxPipeline()
	pipe.IncrBy(ctx, key, n)
	pipe.Expire(ctx, key, 2*time.Minute)
	_, err := pipe.Exec(ctx)
	return err
}
