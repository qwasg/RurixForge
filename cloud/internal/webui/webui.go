// Package webui 提供 /admin/ 管理后台静态资源：
// 发布构建（-tags embedui）内嵌 dist；开发时读 FORGE_CLOUD_WEB_DIR 或 ./web/dist；都没有则给占位页。
package webui

import (
	"io/fs"
	"log/slog"
	"net/http"
	"os"
	"path"
	"strings"

	"forge-cloud/internal/config"
)

const placeholder = `<!doctype html><html lang="zh-CN"><meta charset="utf-8"><title>forge-cloud</title>
<body style="font-family:system-ui;padding:40px;color:#333">
<h2>forge-cloud 管理后台未构建</h2>
<p>开发：<code>pnpm --filter @forge/cloud-admin dev</code>（Vite 代理到本服务）。</p>
<p>发布：<code>pnpm --filter @forge/cloud-admin build</code> 后设置 <code>FORGE_CLOUD_WEB_DIR=cloud/web/dist</code>，
或用 <code>-tags embedui</code> 构建内嵌版本（见 cloud/deploy/Dockerfile）。</p></body></html>`

// Handler 返回挂在 /admin/ 下的静态资源 handler（SPA：未知路径回落 index.html）。
func Handler(cfg *config.Config, log *slog.Logger) http.Handler {
	fsys := embedded()
	source := "embedded"
	if fsys == nil {
		dir := cfg.WebDir
		if dir == "" {
			dir = "web/dist"
		}
		if st, err := os.Stat(path.Join(dir, "index.html")); err == nil && !st.IsDir() {
			fsys = os.DirFS(dir)
			source = dir
		}
	}
	if fsys == nil {
		log.Info("admin ui not built; serving placeholder")
		return http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
			w.Header().Set("Content-Type", "text/html; charset=utf-8")
			_, _ = w.Write([]byte(placeholder))
		})
	}
	log.Info("admin ui", "source", source)
	fileServer := http.FileServer(http.FS(fsys))
	return http.StripPrefix("/admin", http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		p := strings.TrimPrefix(path.Clean("/"+r.URL.Path), "/")
		if p == "" {
			p = "index.html"
		}
		if _, err := fs.Stat(fsys, p); err != nil {
			// SPA 路由：回落 index.html，且不缓存。
			w.Header().Set("Cache-Control", "no-cache")
			r2 := r.Clone(r.Context())
			r2.URL.Path = "/"
			fileServer.ServeHTTP(w, r2)
			return
		}
		if strings.HasPrefix(p, "assets/") {
			w.Header().Set("Cache-Control", "public, max-age=31536000, immutable")
		}
		fileServer.ServeHTTP(w, r)
	}))
}
