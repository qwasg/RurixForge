// forge-cloud：RurixForge 云服务（账号、模型网关、计费、资料同步、管理后台）。
//
// 子命令：
//
//	forge-cloud serve                                   启动服务（默认）
//	forge-cloud migrate                                 只执行数据库迁移
//	forge-cloud admin create --email E --password P     创建/提升管理员
//	forge-cloud accounts import-codex [--group ID]... FILE...   批量导入 Codex auth.json
//	forge-cloud dev fake-upstream [--addr :8199]        启动假上游（开发/端到端测试）
//	forge-cloud version
package main

import (
	"context"
	"errors"
	"flag"
	"fmt"
	"log/slog"
	"net/http"
	"os"
	"os/signal"
	"strconv"
	"strings"
	"syscall"
	"time"

	"forge-cloud/internal/config"
	"forge-cloud/internal/db"
	"forge-cloud/internal/devupstream"
	"forge-cloud/internal/server"
)

func main() {
	args := os.Args[1:]
	cmd := "serve"
	if len(args) > 0 && !strings.HasPrefix(args[0], "-") {
		cmd, args = args[0], args[1:]
	}
	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()

	var err error
	switch cmd {
	case "serve":
		err = runServe(ctx)
	case "migrate":
		err = runMigrate(ctx)
	case "admin":
		err = runAdmin(ctx, args)
	case "accounts":
		err = runAccounts(ctx, args)
	case "dev":
		err = runDev(ctx, args)
	case "version":
		fmt.Println(config.Version)
	case "help", "-h", "--help":
		usage()
	default:
		usage()
		err = fmt.Errorf("未知子命令 %q", cmd)
	}
	if err != nil {
		fmt.Fprintln(os.Stderr, "forge-cloud:", err)
		os.Exit(1)
	}
}

func usage() {
	fmt.Fprintln(os.Stderr, `用法: forge-cloud <serve|migrate|admin create|accounts import-codex|dev fake-upstream|version>`)
}

func newLogger(level string) *slog.Logger {
	var lv slog.Level
	switch strings.ToLower(level) {
	case "debug":
		lv = slog.LevelDebug
	case "warn":
		lv = slog.LevelWarn
	case "error":
		lv = slog.LevelError
	default:
		lv = slog.LevelInfo
	}
	return slog.New(slog.NewJSONHandler(os.Stdout, &slog.HandlerOptions{Level: lv}))
}

// bootstrap 读取配置、连库、迁移、连 Redis。
func bootstrap(ctx context.Context) (*server.Server, func(), error) {
	cfg, err := config.Load()
	if err != nil {
		return nil, nil, err
	}
	log := newLogger(cfg.LogLevel)
	slog.SetDefault(log)
	for _, w := range cfg.Warnings {
		log.Warn(w)
	}
	pool, err := db.Connect(ctx, cfg.DatabaseURL)
	if err != nil {
		return nil, nil, err
	}
	if err := db.Migrate(ctx, pool); err != nil {
		pool.Close()
		return nil, nil, err
	}
	rdb, err := db.ConnectRedis(ctx, cfg.RedisURL)
	if err != nil {
		pool.Close()
		return nil, nil, err
	}
	srv, err := server.New(cfg, log, pool, rdb)
	if err != nil {
		pool.Close()
		_ = rdb.Close()
		return nil, nil, err
	}
	cleanup := func() {
		_ = rdb.Close()
		pool.Close()
	}
	return srv, cleanup, nil
}

func runServe(ctx context.Context) error {
	srv, cleanup, err := bootstrap(ctx)
	if err != nil {
		return err
	}
	defer cleanup()
	if err := srv.Auth.EnsureBootstrapAdmin(ctx); err != nil {
		srv.Log.Error("bootstrap admin failed", "err", err)
	}
	srv.StartWorkers(ctx)

	httpSrv := &http.Server{
		Addr:              srv.Config.Addr,
		Handler:           srv.Handler(),
		ReadHeaderTimeout: 15 * time.Second,
		IdleTimeout:       120 * time.Second,
	}
	errCh := make(chan error, 1)
	go func() {
		srv.Log.Info("forge-cloud listening", "addr", srv.Config.Addr, "version", config.Version, "env", srv.Config.Env)
		errCh <- httpSrv.ListenAndServe()
	}()
	select {
	case <-ctx.Done():
	case err := <-errCh:
		if !errors.Is(err, http.ErrServerClosed) {
			return err
		}
	}
	shutdownCtx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
	defer cancel()
	return httpSrv.Shutdown(shutdownCtx)
}

func runMigrate(ctx context.Context) error {
	_, cleanup, err := bootstrap(ctx)
	if err != nil {
		return err
	}
	cleanup()
	fmt.Println("迁移完成")
	return nil
}

func runAdmin(ctx context.Context, args []string) error {
	if len(args) == 0 || args[0] != "create" {
		return errors.New("用法: forge-cloud admin create --email E --password P")
	}
	fs := flag.NewFlagSet("admin create", flag.ContinueOnError)
	email := fs.String("email", "", "管理员邮箱")
	password := fs.String("password", "", "管理员密码（≥8 位）")
	if err := fs.Parse(args[1:]); err != nil {
		return err
	}
	if *email == "" || *password == "" {
		return errors.New("--email 与 --password 必填")
	}
	srv, cleanup, err := bootstrap(ctx)
	if err != nil {
		return err
	}
	defer cleanup()
	if err := srv.Auth.CreateAdmin(ctx, strings.ToLower(strings.TrimSpace(*email)), *password); err != nil {
		return err
	}
	fmt.Println("管理员已就绪:", *email)
	return nil
}

type int64List []int64

func (l *int64List) String() string { return fmt.Sprint(*l) }
func (l *int64List) Set(v string) error {
	n, err := strconv.ParseInt(v, 10, 64)
	if err != nil {
		return err
	}
	*l = append(*l, n)
	return nil
}

func runAccounts(ctx context.Context, args []string) error {
	if len(args) == 0 || args[0] != "import-codex" {
		return errors.New("用法: forge-cloud accounts import-codex [--group ID]... FILE...")
	}
	fs := flag.NewFlagSet("accounts import-codex", flag.ContinueOnError)
	var groups int64List
	fs.Var(&groups, "group", "加入的分组 ID（可重复）")
	if err := fs.Parse(args[1:]); err != nil {
		return err
	}
	files := fs.Args()
	if len(files) == 0 {
		return errors.New("至少指定一个 auth.json 文件")
	}
	srv, cleanup, err := bootstrap(ctx)
	if err != nil {
		return err
	}
	defer cleanup()
	res, err := srv.Accounts.ImportCodexFiles(ctx, files, groups)
	if err != nil {
		return err
	}
	fmt.Printf("导入完成：新建 %d，更新 %d，失败 %d\n", res.Created, res.Updated, len(res.Errors))
	for _, e := range res.Errors {
		fmt.Println("  -", e)
	}
	if len(res.Errors) > 0 && res.Created+res.Updated == 0 {
		return errors.New("全部导入失败")
	}
	return nil
}

func runDev(ctx context.Context, args []string) error {
	if len(args) == 0 || args[0] != "fake-upstream" {
		return errors.New("用法: forge-cloud dev fake-upstream [--addr :8199]")
	}
	fs := flag.NewFlagSet("dev fake-upstream", flag.ContinueOnError)
	addr := fs.String("addr", "127.0.0.1:8199", "监听地址")
	if err := fs.Parse(args[1:]); err != nil {
		return err
	}
	return devupstream.Run(ctx, *addr, newLogger("info"))
}
