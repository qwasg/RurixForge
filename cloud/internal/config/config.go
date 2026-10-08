// Package config 读取 forge-cloud 的环境变量配置（见 15_CLOUD_SERVICE.md §1.1）。
package config

import (
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"errors"
	"fmt"
	"os"
	"strconv"
	"strings"
)

// Version 由构建时 -ldflags "-X forge-cloud/internal/config.Version=..." 注入。
var Version = "0.1.0-dev"

const (
	devJWTSecret = "forge-cloud-dev-jwt-secret-change-me"
	devMasterKey = "forge-cloud-dev-master-key-change-me"
)

type SMTPConfig struct {
	Host     string
	Port     int
	User     string
	Password string
	From     string
}

func (s SMTPConfig) Enabled() bool { return s.Host != "" && s.From != "" }

type Config struct {
	Env            string
	Addr           string
	PublicURL      string
	DatabaseURL    string
	RedisURL       string
	JWTSecret      []byte
	MasterKey      []byte
	AdminEmail     string
	AdminPassword  string
	SMTP           SMTPConfig
	WebDir         string
	CodexBaseURL   string
	ChatGPTBaseURL string
	OpenAIAuthURL  string
	UpstreamProxy  string
	TrustProxy     bool
	LogLevel       string
	// Warnings 收集开发默认值等非致命提示，启动时打日志。
	Warnings []string
}

func (c *Config) IsProd() bool { return c.Env == "prod" }

func env(key, def string) string {
	if v := strings.TrimSpace(os.Getenv(key)); v != "" {
		return v
	}
	return def
}

// Load 读取环境变量；prod 环境下拒绝默认密钥。
func Load() (*Config, error) {
	c := &Config{
		Env:            env("FORGE_CLOUD_ENV", "dev"),
		Addr:           env("FORGE_CLOUD_ADDR", ":8110"),
		PublicURL:      strings.TrimRight(env("FORGE_CLOUD_PUBLIC_URL", "http://127.0.0.1:8110"), "/"),
		DatabaseURL:    env("FORGE_CLOUD_DATABASE_URL", "postgres://forge:forge@127.0.0.1:5432/forge_cloud?sslmode=disable"),
		RedisURL:       env("FORGE_CLOUD_REDIS_URL", "redis://127.0.0.1:6379/0"),
		AdminEmail:     strings.ToLower(env("FORGE_CLOUD_ADMIN_EMAIL", "")),
		AdminPassword:  os.Getenv("FORGE_CLOUD_ADMIN_PASSWORD"),
		WebDir:         env("FORGE_CLOUD_WEB_DIR", ""),
		CodexBaseURL:   strings.TrimRight(env("FORGE_CLOUD_CODEX_BASE_URL", "https://chatgpt.com/backend-api/codex"), "/"),
		ChatGPTBaseURL: strings.TrimRight(env("FORGE_CLOUD_CHATGPT_BASE_URL", "https://chatgpt.com/backend-api"), "/"),
		OpenAIAuthURL:  strings.TrimRight(env("FORGE_CLOUD_OPENAI_AUTH_URL", "https://auth.openai.com"), "/"),
		UpstreamProxy:  env("FORGE_CLOUD_UPSTREAM_PROXY", ""),
		TrustProxy:     env("FORGE_CLOUD_TRUST_PROXY", "0") == "1",
		LogLevel:       env("FORGE_CLOUD_LOG_LEVEL", "info"),
	}
	if c.Env != "dev" && c.Env != "prod" && c.Env != "test" {
		return nil, fmt.Errorf("FORGE_CLOUD_ENV 只能是 dev/test/prod，当前 %q", c.Env)
	}

	jwt := os.Getenv("FORGE_CLOUD_JWT_SECRET")
	if jwt == "" {
		if c.IsProd() {
			return nil, errors.New("prod 环境必须设置 FORGE_CLOUD_JWT_SECRET")
		}
		jwt = devJWTSecret
		c.Warnings = append(c.Warnings, "FORGE_CLOUD_JWT_SECRET 未设置，使用开发默认值（勿用于生产）")
	}
	if len(jwt) < 16 {
		return nil, errors.New("FORGE_CLOUD_JWT_SECRET 至少 16 个字符")
	}
	c.JWTSecret = []byte(jwt)

	mk := os.Getenv("FORGE_CLOUD_MASTER_KEY")
	if mk == "" {
		if c.IsProd() {
			return nil, errors.New("prod 环境必须设置 FORGE_CLOUD_MASTER_KEY（32 字节 base64 或 64 位 hex）")
		}
		sum := sha256.Sum256([]byte(devMasterKey))
		c.MasterKey = sum[:]
		c.Warnings = append(c.Warnings, "FORGE_CLOUD_MASTER_KEY 未设置，使用开发默认值（勿用于生产）")
	} else {
		key, err := ParseMasterKey(mk)
		if err != nil {
			return nil, err
		}
		c.MasterKey = key
	}

	c.SMTP = SMTPConfig{
		Host:     env("FORGE_CLOUD_SMTP_HOST", ""),
		User:     env("FORGE_CLOUD_SMTP_USER", ""),
		Password: os.Getenv("FORGE_CLOUD_SMTP_PASSWORD"),
		From:     env("FORGE_CLOUD_SMTP_FROM", ""),
	}
	if p := env("FORGE_CLOUD_SMTP_PORT", "587"); p != "" {
		n, err := strconv.Atoi(p)
		if err != nil {
			return nil, fmt.Errorf("FORGE_CLOUD_SMTP_PORT 非法: %w", err)
		}
		c.SMTP.Port = n
	}
	return c, nil
}

// ParseMasterKey 接受 32 字节的 base64（标准/URL、带或不带填充）或 64 位 hex。
func ParseMasterKey(s string) ([]byte, error) {
	s = strings.TrimSpace(s)
	if len(s) == 64 {
		if b, err := hex.DecodeString(s); err == nil {
			return b, nil
		}
	}
	for _, enc := range []*base64.Encoding{base64.StdEncoding, base64.RawStdEncoding, base64.URLEncoding, base64.RawURLEncoding} {
		if b, err := enc.DecodeString(s); err == nil && len(b) == 32 {
			return b, nil
		}
	}
	return nil, errors.New("FORGE_CLOUD_MASTER_KEY 必须是 32 字节（base64）或 64 位 hex")
}

// ForTest 返回固定密钥的测试配置。
func ForTest() *Config {
	sum := sha256.Sum256([]byte("forge-cloud-test-master-key"))
	return &Config{
		Env:            "test",
		Addr:           "127.0.0.1:0",
		PublicURL:      "http://127.0.0.1:8110",
		JWTSecret:      []byte("forge-cloud-test-jwt-secret"),
		MasterKey:      sum[:],
		CodexBaseURL:   "https://chatgpt.com/backend-api/codex",
		ChatGPTBaseURL: "https://chatgpt.com/backend-api",
		OpenAIAuthURL:  "https://auth.openai.com",
		LogLevel:       "error",
	}
}
