// Forge's local Antigravity adapter. Google performs the browser OAuth;
// CLIProxyAPI's pinned SDK supplies token refresh and model/tool translation.
package main

import (
	"context"
	"crypto/subtle"
	"errors"
	"io"
	"os"
	"os/signal"
	"path/filepath"
	"strconv"
	"strings"
	"time"

	"github.com/gin-gonic/gin"
	sdkapi "github.com/router-for-me/CLIProxyAPI/v8/sdk/api"
	"github.com/router-for-me/CLIProxyAPI/v8/sdk/api/handlers"
	sdkauth "github.com/router-for-me/CLIProxyAPI/v8/sdk/auth"
	"github.com/router-for-me/CLIProxyAPI/v8/sdk/cliproxy"
	core "github.com/router-for-me/CLIProxyAPI/v8/sdk/cliproxy/auth"
	"github.com/router-for-me/CLIProxyAPI/v8/sdk/config"
	log "github.com/sirupsen/logrus"
	"golang.org/x/crypto/bcrypt"
)

func main() {
	if err := run(); err != nil {
		os.Exit(1)
	}
}
func run() error {
	key, management := os.Getenv("FORGE_ANTIGRAVITY_BRIDGE_KEY"), os.Getenv("FORGE_ANTIGRAVITY_MANAGEMENT_KEY")
	port, err := strconv.Atoi(os.Getenv("FORGE_ANTIGRAVITY_BRIDGE_PORT"))
	if err != nil || port < 1024 || key == "" || management == "" {
		return errors.New("missing local runtime configuration")
	}
	dir := os.Getenv("FORGE_ANTIGRAVITY_BRIDGE_HOME")
	if dir == "" {
		return errors.New("missing credential directory")
	}
	gin.SetMode(gin.ReleaseMode)
	gin.DefaultWriter, gin.DefaultErrorWriter = io.Discard, io.Discard
	log.SetOutput(io.Discard)
	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt)
	defer stop()
	// The parent closes stdin when it exits; the adapter exits with it.
	go func() { _, _ = io.Copy(io.Discard, os.Stdin); stop() }()
	store := &encryptedStore{dir: filepath.Join(dir, "credentials")}
	if _, err = store.List(ctx); err != nil {
		return err
	}
	sdkauth.RegisterTokenStore(store)
	cfg, err := config.ParseConfigBytes([]byte("host: 127.0.0.1\nport: " + strconv.Itoa(port) + "\ncommercial-mode: true\nrequest-log: false\nlogging-to-file: false\nrequest-retry: 0\nmax-retry-interval: 0\nremote-management:\n  disable-control-panel: true\n  disable-auto-update-panel: true\n"))
	if err != nil {
		return err
	}
	cfg.AuthDir = filepath.Join(dir, "oauth-callbacks")
	cfg.APIKeys = []string{key}
	// This SDK version checks for a configured hash before its localhost password.
	// Keep that hash in memory; never serialize it to the watched YAML file.
	hash, err := bcrypt.GenerateFromPassword([]byte(management), bcrypt.DefaultCost)
	if err != nil {
		return err
	}
	cfg.RemoteManagement.SecretKey = string(hash)
	manager := core.NewManager(store, nil, nil)
	service, err := cliproxy.NewBuilder().WithConfig(cfg).WithConfigPath(filepath.Join(dir, "runtime.yaml")).
		WithCoreAuthManager(manager).WithLocalManagementPassword(management).
		WithHooks(cliproxy.Hooks{OnAfterStart: func(*cliproxy.Service) { restoreCatalogs(manager) }}).
		WithServerOptions(sdkapi.WithRouterConfigurator(func(engine *gin.Engine, _ *handlers.BaseAPIHandler, _ *config.Config) {
			local := engine.Group("/forge", func(c *gin.Context) {
				if subtle.ConstantTimeCompare([]byte(strings.TrimPrefix(c.GetHeader("Authorization"), "Bearer ")), []byte(management)) != 1 {
					c.AbortWithStatus(401)
					return
				}
				c.Header("Cache-Control", "no-store")
			})
			local.GET("/account", func(c *gin.Context) {
				for _, auth := range manager.List() {
					if auth.Provider != "antigravity" || auth.Disabled {
						continue
					}
					c.JSON(200, gin.H{"configured": true, "email": auth.Metadata["email"], "authIndex": auth.EnsureIndex(), "projectId": auth.Metadata["project_id"], "userAgent": sdkauth.AntigravityUserAgent()})
					return
				}
				c.JSON(200, gin.H{"configured": false})
			})
			local.POST("/begin-login", func(c *gin.Context) { store.beginLogin(); c.JSON(200, gin.H{"ok": true}) })
			local.POST("/cancel-login", func(c *gin.Context) {
				if store.finishLogin(true) != nil {
					c.AbortWithStatus(500)
					return
				}
				c.JSON(200, gin.H{"ok": true})
			})
			local.POST("/complete-login", func(c *gin.Context) { _ = store.finishLogin(false); c.JSON(200, gin.H{"ok": true}) })
			local.POST("/logout", func(c *gin.Context) {
				for _, auth := range manager.List() {
					if auth.Provider != "antigravity" {
						continue
					}
					manager.Remove(c.Request.Context(), auth.ID)
					cliproxy.GlobalModelRegistry().UnregisterClient(auth.ID)
					if store.Delete(c.Request.Context(), auth.ID) != nil {
						c.JSON(500, gin.H{"error": "credential removal failed"})
						return
					}
				}
				c.JSON(200, gin.H{"ok": true})
			})
			local.POST("/models", func(c *gin.Context) {
				var request struct {
					AuthIndex string         `json:"authIndex"`
					Models    []catalogModel `json:"models"`
				}
				if c.ShouldBindJSON(&request) != nil {
					c.AbortWithStatus(400)
					return
				}
				for _, auth := range manager.List() {
					if auth.Provider != "antigravity" || auth.Disabled || auth.EnsureIndex() != request.AuthIndex {
						continue
					}
					if cacheCatalog(c.Request.Context(), manager, auth, request.Models) != nil {
						c.JSON(400, gin.H{"error": "model catalog registration failed"})
						return
					}
					c.JSON(200, gin.H{"ok": true})
					return
				}
				c.AbortWithStatus(401)
			})
		})).Build()
	if err != nil {
		return err
	}
	_ = os.MkdirAll(dir, 0700)
	// An empty, non-sensitive watch file avoids writing keys into config YAML.
	if err = os.WriteFile(filepath.Join(dir, "runtime.yaml"), []byte("# Runtime configuration is supplied in memory by Forge.\n"), 0600); err != nil {
		return err
	}
	go func() {
		<-ctx.Done()
		shutdown, cancel := context.WithTimeout(context.Background(), 3*time.Second)
		defer cancel()
		_ = service.Shutdown(shutdown)
	}()
	return service.Run(ctx)
}
