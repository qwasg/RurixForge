// Package scheduler 为一次网关请求从候选上游账号里选号（15_CLOUD_SERVICE.md §4）：
//
//  1. 粘性：Redis `sticky:<userId>:<sha256(sessionKey)[:16]>` → 账号 ID（TTL = stickyTtlSeconds，命中即续期）；
//     仅当该账号仍在候选里、未被排除且能占到账号并发槽时才采用；
//  2. 优先级数值小者优先；
//  3. 负载率（当前并发 / 上限，不限 = 0）低者优先；
//  4. 同优先级同负载率内按权重随机。
//
// 按上述顺序逐个尝试占账号槽，成功即绑定粘性。候选由调用方给出（已过滤状态、冷却、分组与端点兼容性），
// Exclude 用于换号重试时跳过已失败的账号。
package scheduler

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"math"
	"math/rand/v2"
	"sort"
	"strconv"
	"sync"
	"time"

	"github.com/redis/go-redis/v9"

	"forge-cloud/internal/ratelimit"
)

var (
	// ErrNoCandidates：排除后没有任何候选账号。
	ErrNoCandidates = errors.New("scheduler: 没有可用的候选账号")
	// ErrAllBusy：有候选但账号并发槽全满。
	ErrAllBusy = errors.New("scheduler: 候选账号并发已满")
)

// Candidate 是一个可调度的上游账号。
type Candidate struct {
	ID               int64
	Priority         int
	Weight           int
	ConcurrencyLimit int // 0 = 不限
}

// Request 描述一次选号。
type Request struct {
	UserID int64
	// SessionKey 为空表示不走粘性。
	SessionKey string
	StickyTTL  time.Duration
	// Member 是占槽成员（请求 ID）。
	Member  string
	Exclude map[int64]bool
}

// Lease 是选中的账号及其并发槽；用完必须 Release。
type Lease struct {
	AccountID int64
	// Sticky 表示经粘性映射选中。
	Sticky  bool
	slotKey string
	member  string
}

type Scheduler struct {
	rdb     *redis.Client
	limiter *ratelimit.Limiter

	mu   sync.Mutex
	rand func() float64
}

func New(rdb *redis.Client, limiter *ratelimit.Limiter) *Scheduler {
	return &Scheduler{rdb: rdb, limiter: limiter, rand: rand.Float64}
}

// SetRand 替换随机源（测试用），f 返回 [0,1)。
func (s *Scheduler) SetRand(f func() float64) {
	s.mu.Lock()
	s.rand = f
	s.mu.Unlock()
}

// StickyKey 返回粘性映射的 Redis 键。
func StickyKey(userID int64, sessionKey string) string {
	sum := sha256.Sum256([]byte(sessionKey))
	return "sticky:" + strconv.FormatInt(userID, 10) + ":" + hex.EncodeToString(sum[:])[:16]
}

// Pick 选号并占账号槽。
func (s *Scheduler) Pick(ctx context.Context, cands []Candidate, req Request) (*Lease, error) {
	pool := make([]Candidate, 0, len(cands))
	for _, c := range cands {
		if !req.Exclude[c.ID] {
			pool = append(pool, c)
		}
	}
	if len(pool) == 0 {
		return nil, ErrNoCandidates
	}

	var stickyKey string
	if req.SessionKey != "" {
		stickyKey = StickyKey(req.UserID, req.SessionKey)
		if lease, err := s.trySticky(ctx, pool, stickyKey, req); err != nil || lease != nil {
			return lease, err
		}
	}

	ordered, err := s.order(ctx, pool)
	if err != nil {
		return nil, err
	}
	for _, c := range ordered {
		key := ratelimit.AccountSlotKey(c.ID)
		ok, err := s.limiter.Acquire(ctx, key, c.ConcurrencyLimit, req.Member)
		if err != nil {
			return nil, err
		}
		if !ok {
			continue
		}
		if stickyKey != "" && req.StickyTTL > 0 {
			if err := s.rdb.Set(ctx, stickyKey, c.ID, req.StickyTTL).Err(); err != nil {
				_ = s.limiter.Release(ctx, key, req.Member)
				return nil, err
			}
		}
		return &Lease{AccountID: c.ID, slotKey: key, member: req.Member}, nil
	}
	return nil, ErrAllBusy
}

func (s *Scheduler) trySticky(ctx context.Context, pool []Candidate, stickyKey string, req Request) (*Lease, error) {
	// 读粘性失败（键不存在/值损坏/Redis 抖动）一律按无粘性处理，后续占槽仍会暴露真正的 Redis 故障。
	id, err := s.rdb.Get(ctx, stickyKey).Int64()
	if err != nil {
		return nil, nil
	}
	for _, c := range pool {
		if c.ID != id {
			continue
		}
		key := ratelimit.AccountSlotKey(c.ID)
		ok, err := s.limiter.Acquire(ctx, key, c.ConcurrencyLimit, req.Member)
		if err != nil || !ok {
			return nil, err
		}
		if req.StickyTTL > 0 {
			_ = s.rdb.Expire(ctx, stickyKey, req.StickyTTL).Err()
		}
		return &Lease{AccountID: c.ID, Sticky: true, slotKey: key, member: req.Member}, nil
	}
	return nil, nil
}

// order 按 优先级 → 负载率 → 权重随机 排出尝试顺序。
func (s *Scheduler) order(ctx context.Context, pool []Candidate) ([]Candidate, error) {
	keys := make([]string, len(pool))
	for i, c := range pool {
		keys[i] = ratelimit.AccountSlotKey(c.ID)
	}
	counts, err := s.limiter.Counts(ctx, keys)
	if err != nil {
		return nil, err
	}
	type scored struct {
		c    Candidate
		load float64
		rnd  float64
	}
	items := make([]scored, len(pool))
	s.mu.Lock()
	for i, c := range pool {
		load := 0.0
		if c.ConcurrencyLimit > 0 {
			load = float64(counts[i]) / float64(c.ConcurrencyLimit)
		}
		w := c.Weight
		if w < 1 {
			w = 1
		}
		// Efraimidis–Spirakis：key = u^(1/w)，按 key 降序即按权重无放回抽样。
		u := s.rand()
		if u <= 0 {
			u = math.SmallestNonzeroFloat64
		}
		items[i] = scored{c: c, load: load, rnd: math.Pow(u, 1/float64(w))}
	}
	s.mu.Unlock()
	sort.SliceStable(items, func(a, b int) bool {
		x, y := items[a], items[b]
		if x.c.Priority != y.c.Priority {
			return x.c.Priority < y.c.Priority
		}
		if x.load != y.load {
			return x.load < y.load
		}
		return x.rnd > y.rnd
	})
	out := make([]Candidate, len(items))
	for i, it := range items {
		out[i] = it.c
	}
	return out, nil
}

// Release 释放账号槽（可重复调用）。
func (s *Scheduler) Release(ctx context.Context, l *Lease) error {
	if l == nil || l.slotKey == "" {
		return nil
	}
	key := l.slotKey
	l.slotKey = ""
	return s.limiter.Release(ctx, key, l.member)
}

// Extend 为长请求续租账号槽。
func (s *Scheduler) Extend(ctx context.Context, l *Lease) error {
	if l == nil || l.slotKey == "" {
		return nil
	}
	return s.limiter.Extend(ctx, l.slotKey, l.member)
}

// SlotKey 返回租约占用的账号槽键（已释放为空）。
func (l *Lease) SlotKey() string { return l.slotKey }
