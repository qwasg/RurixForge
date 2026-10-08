// Package db 负责 PostgreSQL 连接池、内嵌迁移（goose）与 Redis 连接。
package db

import (
	"context"
	"embed"
	"fmt"
	"io/fs"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/jackc/pgx/v5/stdlib"
	"github.com/pressly/goose/v3"
	"github.com/redis/go-redis/v9"
)

//go:embed migrations/*.sql
var migrationsFS embed.FS

// Migrations 暴露迁移文件（测试与 CLI 共用）。
func Migrations() fs.FS { return migrationsFS }

// Connect 建立连接池并探活。
func Connect(ctx context.Context, url string) (*pgxpool.Pool, error) {
	cfg, err := pgxpool.ParseConfig(url)
	if err != nil {
		return nil, fmt.Errorf("解析数据库地址: %w", err)
	}
	if cfg.MaxConns < 10 {
		cfg.MaxConns = 20
	}
	cfg.MaxConnIdleTime = 5 * time.Minute
	pool, err := pgxpool.NewWithConfig(ctx, cfg)
	if err != nil {
		return nil, fmt.Errorf("连接数据库: %w", err)
	}
	pingCtx, cancel := context.WithTimeout(ctx, 5*time.Second)
	defer cancel()
	if err := pool.Ping(pingCtx); err != nil {
		pool.Close()
		return nil, fmt.Errorf("数据库不可达: %w", err)
	}
	return pool, nil
}

// Migrate 把内嵌迁移应用到最新版本。
func Migrate(ctx context.Context, pool *pgxpool.Pool) error {
	sqlDB := stdlib.OpenDBFromPool(pool)
	defer sqlDB.Close()
	provider, err := goose.NewProvider(goose.DialectPostgres, sqlDB, mustSub(migrationsFS, "migrations"))
	if err != nil {
		return fmt.Errorf("初始化迁移: %w", err)
	}
	if _, err := provider.Up(ctx); err != nil {
		return fmt.Errorf("执行迁移: %w", err)
	}
	return nil
}

func mustSub(f fs.FS, dir string) fs.FS {
	sub, err := fs.Sub(f, dir)
	if err != nil {
		panic(err)
	}
	return sub
}

// ConnectRedis 建立 Redis 客户端并探活。
func ConnectRedis(ctx context.Context, url string) (*redis.Client, error) {
	opt, err := redis.ParseURL(url)
	if err != nil {
		return nil, fmt.Errorf("解析 Redis 地址: %w", err)
	}
	client := redis.NewClient(opt)
	pingCtx, cancel := context.WithTimeout(ctx, 5*time.Second)
	defer cancel()
	if err := client.Ping(pingCtx).Err(); err != nil {
		_ = client.Close()
		return nil, fmt.Errorf("Redis 不可达: %w", err)
	}
	return client, nil
}
