package ratelimit_test

import (
	"context"
	"testing"
	"time"

	"forge-cloud/internal/ratelimit"
	"forge-cloud/internal/testutil"
)

func TestConcurrencySlotAcquireRelease(t *testing.T) {
	rdb, _ := testutil.NewRedis(t)
	lim := ratelimit.New(rdb)
	ctx := context.Background()
	key := ratelimit.UserSlotKey(42)

	ok, err := lim.Acquire(ctx, key, 1, "req-a")
	if err != nil || !ok {
		t.Fatalf("acquire a: ok=%v err=%v", ok, err)
	}
	ok, err = lim.Acquire(ctx, key, 1, "req-b")
	if err != nil || ok {
		t.Fatalf("second acquire should block: ok=%v err=%v", ok, err)
	}
	if err := lim.Release(ctx, key, "req-a"); err != nil {
		t.Fatal(err)
	}
	ok, err = lim.Acquire(ctx, key, 1, "req-b")
	if err != nil || !ok {
		t.Fatalf("acquire b after release: ok=%v err=%v", ok, err)
	}
}

func TestRPMWindow(t *testing.T) {
	rdb, mr := testutil.NewRedis(t)
	lim := ratelimit.New(rdb)
	now := time.Unix(1_700_000_000, 0)
	lim.SetClock(func() time.Time { return now })
	ctx := context.Background()
	uid := int64(7)

	for i := 0; i < 3; i++ {
		ok, retry, err := lim.CheckRPM(ctx, uid, 3)
		if err != nil || !ok || retry != 0 {
			t.Fatalf("rpm %d: ok=%v retry=%d err=%v", i, ok, retry, err)
		}
	}
	ok, retry, err := lim.CheckRPM(ctx, uid, 3)
	if err != nil || ok || retry < 1 {
		t.Fatalf("rpm exceeded: ok=%v retry=%d err=%v", ok, retry, err)
	}
	mr.FastForward(61 * time.Second)
	ok, _, err = lim.CheckRPM(ctx, uid, 3)
	if err != nil || !ok {
		t.Fatalf("rpm after window: ok=%v err=%v", ok, err)
	}
}

func TestTPMAccumulate(t *testing.T) {
	rdb, _ := testutil.NewRedis(t)
	lim := ratelimit.New(rdb)
	now := time.Unix(1_700_000_000, 0)
	lim.SetClock(func() time.Time { return now })
	ctx := context.Background()
	uid := int64(9)

	ok, _, err := lim.CheckTPM(ctx, uid, 100)
	if err != nil || !ok {
		t.Fatal(err)
	}
	if err := lim.AddTokens(ctx, uid, 90); err != nil {
		t.Fatal(err)
	}
	ok, retry, err := lim.CheckTPM(ctx, uid, 100)
	if err != nil || !ok {
		t.Fatalf("under limit: ok=%v err=%v", ok, err)
	}
	if err := lim.AddTokens(ctx, uid, 20); err != nil {
		t.Fatal(err)
	}
	ok, retry, err = lim.CheckTPM(ctx, uid, 100)
	if err != nil || ok || retry < 1 {
		t.Fatalf("tpm exceeded: ok=%v retry=%d err=%v", ok, retry, err)
	}
}
