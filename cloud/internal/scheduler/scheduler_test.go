package scheduler_test

import (
	"context"
	"testing"
	"time"

	"forge-cloud/internal/ratelimit"
	"forge-cloud/internal/scheduler"
	"forge-cloud/internal/testutil"
)

func TestPickPriorityAndLoad(t *testing.T) {
	rdb, _ := testutil.NewRedis(t)
	lim := ratelimit.New(rdb)
	sched := scheduler.New(rdb, lim)
	ctx := context.Background()

	cands := []scheduler.Candidate{
		{ID: 1, Priority: 20, Weight: 1, ConcurrencyLimit: 2},
		{ID: 2, Priority: 10, Weight: 1, ConcurrencyLimit: 2},
	}
	// 占满账号 2 的一个槽，负载率 50%。
	ok, err := lim.Acquire(ctx, ratelimit.AccountSlotKey(2), 2, "other")
	if err != nil || !ok {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = lim.Release(context.Background(), ratelimit.AccountSlotKey(2), "other") })

	sched.SetRand(func() float64 { return 0.5 })
	lease, err := sched.Pick(ctx, cands, scheduler.Request{UserID: 1, Member: "m1", StickyTTL: time.Hour})
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = sched.Release(context.Background(), lease) }()
	if lease.AccountID != 2 {
		t.Fatalf("want lower priority account 2, got %d", lease.AccountID)
	}
}

func TestStickyReuse(t *testing.T) {
	rdb, _ := testutil.NewRedis(t)
	lim := ratelimit.New(rdb)
	sched := scheduler.New(rdb, lim)
	ctx := context.Background()
	cands := []scheduler.Candidate{
		{ID: 10, Priority: 1, Weight: 1, ConcurrencyLimit: 1},
		{ID: 11, Priority: 1, Weight: 1, ConcurrencyLimit: 1},
	}
	req := scheduler.Request{UserID: 5, SessionKey: "sess-abc", Member: "r1", StickyTTL: time.Hour}
	lease1, err := sched.Pick(ctx, cands, req)
	if err != nil {
		t.Fatal(err)
	}
	first := lease1.AccountID
	_ = sched.Release(ctx, lease1)

	lease2, err := sched.Pick(ctx, cands, scheduler.Request{UserID: 5, SessionKey: "sess-abc", Member: "r2", StickyTTL: time.Hour})
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = sched.Release(context.Background(), lease2) }()
	if lease2.AccountID != first || !lease2.Sticky {
		t.Fatalf("sticky miss: first=%d second=%d sticky=%v", first, lease2.AccountID, lease2.Sticky)
	}
}

func TestExcludeOnFailover(t *testing.T) {
	rdb, _ := testutil.NewRedis(t)
	lim := ratelimit.New(rdb)
	sched := scheduler.New(rdb, lim)
	ctx := context.Background()
	cands := []scheduler.Candidate{
		{ID: 1, Priority: 1, Weight: 1, ConcurrencyLimit: 1},
		{ID: 2, Priority: 2, Weight: 1, ConcurrencyLimit: 1},
	}
	lease, err := sched.Pick(ctx, cands, scheduler.Request{UserID: 1, Member: "x", Exclude: map[int64]bool{1: true}})
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = sched.Release(context.Background(), lease) }()
	if lease.AccountID != 2 {
		t.Fatalf("want account 2, got %d", lease.AccountID)
	}
}
