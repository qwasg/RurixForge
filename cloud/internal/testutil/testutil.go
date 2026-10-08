// Package testutil 给测试提供隔离的 PostgreSQL 库与内存 Redis。
//
// PostgreSQL：每次 NewDB 都新建一个随机库并跑迁移，测试结束 DROP（多个测试/多人并行互不干扰）。
// 管理连接取 FORGE_CLOUD_TEST_PG_URL（默认连本机 dev compose 的 postgres 库）；连不上时 Skip，
// 设 FORGE_CLOUD_TEST_REQUIRE_DB=1 则改为 Fatal（CI 用）。
package testutil

import (
	"context"
	"fmt"
	"io"
	"log/slog"
	"net/url"
	"os"
	"strings"
	"testing"
	"time"

	"github.com/alicebob/miniredis/v2"
	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/redis/go-redis/v9"

	"forge-cloud/internal/config"
	"forge-cloud/internal/core"
	"forge-cloud/internal/db"
)

const defaultAdminURL = "postgres://forge:forge@127.0.0.1:5432/postgres?sslmode=disable"

func adminURL() string {
	if v := strings.TrimSpace(os.Getenv("FORGE_CLOUD_TEST_PG_URL")); v != "" {
		return v
	}
	return defaultAdminURL
}

// NewDB 返回一个已迁移的独立测试库连接池。
func NewDB(t testing.TB) *pgxpool.Pool {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()

	admin, err := pgx.Connect(ctx, adminURL())
	if err != nil {
		if os.Getenv("FORGE_CLOUD_TEST_REQUIRE_DB") == "1" {
			t.Fatalf("连接测试 PostgreSQL 失败: %v", err)
		}
		t.Skipf("跳过：测试 PostgreSQL 不可用（%v）。先 `docker compose -f cloud/deploy/docker-compose.dev.yml up -d`", err)
	}
	name := "ft_" + strings.ToLower(core.RandomString(12))
	if _, err := admin.Exec(ctx, "CREATE DATABASE "+name); err != nil {
		admin.Close(ctx)
		t.Fatalf("创建测试库失败: %v", err)
	}
	admin.Close(ctx)

	u, err := url.Parse(adminURL())
	if err != nil {
		t.Fatalf("解析 FORGE_CLOUD_TEST_PG_URL: %v", err)
	}
	u.Path = "/" + name
	pool, err := db.Connect(ctx, u.String())
	if err != nil {
		t.Fatalf("连接测试库失败: %v", err)
	}
	if err := db.Migrate(ctx, pool); err != nil {
		pool.Close()
		t.Fatalf("测试库迁移失败: %v", err)
	}
	t.Cleanup(func() {
		pool.Close()
		c, cancel := context.WithTimeout(context.Background(), 30*time.Second)
		defer cancel()
		if a, err := pgx.Connect(c, adminURL()); err == nil {
			_, _ = a.Exec(c, fmt.Sprintf("DROP DATABASE IF EXISTS %s WITH (FORCE)", name))
			a.Close(c)
		}
	})
	return pool
}

// NewRedis 返回基于 miniredis 的客户端（进程内，天然隔离）。
func NewRedis(t testing.TB) (*redis.Client, *miniredis.Miniredis) {
	t.Helper()
	mr := miniredis.RunT(t)
	client := redis.NewClient(&redis.Options{Addr: mr.Addr()})
	t.Cleanup(func() { _ = client.Close() })
	return client, mr
}

// Config 返回固定密钥的测试配置。
func Config() *config.Config { return config.ForTest() }

// Logger 返回丢弃输出的 logger（设 FORGE_CLOUD_TEST_LOG=1 时输出到 stderr）。
func Logger() *slog.Logger {
	if os.Getenv("FORGE_CLOUD_TEST_LOG") == "1" {
		return slog.New(slog.NewTextHandler(os.Stderr, &slog.HandlerOptions{Level: slog.LevelDebug}))
	}
	return slog.New(slog.NewTextHandler(io.Discard, nil))
}
