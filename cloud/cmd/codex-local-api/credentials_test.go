package main

import (
	"bufio"
	"context"
	"encoding/base64"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"
)

func credentialTestToken(t *testing.T, account string, expiration time.Time) string {
	t.Helper()
	payload, err := json.Marshal(map[string]any{
		"exp": expiration.Unix(), "https://api.openai.com/auth": map[string]any{"chatgpt_account_id": account},
	})
	if err != nil {
		t.Fatal(err)
	}
	return "test." + base64.RawURLEncoding.EncodeToString(payload) + ".signature"
}

func writeCredentialFixture(t *testing.T, home, token, account string) []byte {
	t.Helper()
	body, err := json.Marshal(map[string]any{"auth_mode": "chatgpt", "tokens": map[string]any{
		"access_token": token, "account_id": account, "refresh_token": "fixture-refresh-secret-must-stay-in-file",
	}})
	if err != nil {
		t.Fatal(err)
	}
	temporary := filepath.Join(home, "auth.json.test-update")
	if err := os.WriteFile(temporary, body, 0600); err != nil {
		t.Fatal(err)
	}
	if err := os.Rename(temporary, filepath.Join(home, "auth.json")); err != nil {
		t.Fatal(err)
	}
	return body
}

func TestCredentialsReadSameFileEachRequest(t *testing.T) {
	home := t.TempDir()
	config := gatewayConfig{CodexHome: home}
	first := credentialTestToken(t, "account-a", time.Now().Add(time.Hour))
	original := writeCredentialFixture(t, home, first, "account-a")
	noRefresh := func(context.Context, gatewayConfig) error { t.Fatal("fresh credential must not refresh"); return nil }
	creds, err := loadCodexCredentialsWithRefresh(context.Background(), config, false, noRefresh)
	if err != nil || creds.AccessToken != first || creds.AccountID != "account-a" {
		t.Fatal("could not load the existing credentials")
	}
	after, err := os.ReadFile(filepath.Join(home, "auth.json"))
	if err != nil || string(after) != string(original) {
		t.Fatal("reading credentials modified the source auth file")
	}
	second := credentialTestToken(t, "account-a", time.Now().Add(2*time.Hour))
	writeCredentialFixture(t, home, second, "account-a")
	creds, err = loadCodexCredentialsWithRefresh(context.Background(), config, false, noRefresh)
	if err != nil || creds.AccessToken != second {
		t.Fatal("did not observe Forge's updated credentials")
	}
}

func TestConcurrentCredentialsRefreshCoalesced(t *testing.T) {
	home := t.TempDir()
	config := gatewayConfig{CodexHome: home}
	writeCredentialFixture(t, home, credentialTestToken(t, "account-a", time.Now().Add(time.Minute)), "account-a")
	fresh := credentialTestToken(t, "account-a", time.Now().Add(time.Hour))
	var calls atomic.Int32
	refresh := func(ctx context.Context, cfg gatewayConfig) error {
		calls.Add(1)
		if _, ok := ctx.Deadline(); !ok || cfg.CodexHome != home {
			return errors.New("refresh must use a bounded context and the same home")
		}
		writeCredentialFixture(t, home, fresh, "account-a")
		return nil
	}
	var wg sync.WaitGroup
	for i := 0; i < 8; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			creds, err := loadCodexCredentialsWithRefresh(context.Background(), config, true, refresh)
			if err != nil || creds.AccessToken != fresh {
				t.Error("concurrent request did not receive refreshed credentials")
			}
		}()
	}
	wg.Wait()
	if calls.Load() != 1 {
		t.Fatalf("refresh calls = %d; expected one", calls.Load())
	}
}

func TestCredentialsRejectAccountChangeDuringRefresh(t *testing.T) {
	home := t.TempDir()
	writeCredentialFixture(t, home, credentialTestToken(t, "account-a", time.Now().Add(time.Hour)), "account-a")
	_, err := loadCodexCredentialsWithRefresh(context.Background(), gatewayConfig{CodexHome: home}, true,
		func(context.Context, gatewayConfig) error {
			writeCredentialFixture(t, home, credentialTestToken(t, "account-b", time.Now().Add(time.Hour)), "account-b")
			return nil
		})
	if err == nil || !strings.Contains(err.Error(), "account changed") {
		t.Fatal("account changes during refresh must fail the current request")
	}
}

func TestCredentialsRejectMismatchedAccountAndSecretSafeErrors(t *testing.T) {
	home := t.TempDir()
	token := credentialTestToken(t, "account-b", time.Now().Add(time.Hour))
	writeCredentialFixture(t, home, token, "account-a")
	_, err := readCodexCredentials(home)
	if err == nil || strings.Contains(err.Error(), token) || strings.Contains(err.Error(), "fixture-refresh-secret") {
		t.Fatal("mismatched auth must fail without exposing credentials")
	}
	if err := os.WriteFile(filepath.Join(home, "auth.json"), []byte(`{"secret":"fixture-refresh-secret"`), 0600); err != nil {
		t.Fatal(err)
	}
	_, err = readCodexCredentials(home)
	if err == nil || strings.Contains(err.Error(), "fixture-refresh-secret") {
		t.Fatal("malformed auth must fail without exposing source text")
	}
}

func TestCredentialsRefreshDeadlineAndCancellation(t *testing.T) {
	home := t.TempDir()
	writeCredentialFixture(t, home, credentialTestToken(t, "account-a", time.Now().Add(-time.Minute)), "account-a")
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	_, err := loadCodexCredentialsWithRefresh(ctx, gatewayConfig{CodexHome: home}, true,
		func(context.Context, gatewayConfig) error { t.Fatal("cancelled request started refresh"); return nil })
	if err == nil {
		t.Fatal("cancelled request must fail")
	}
	_, err = loadCodexCredentialsWithRefresh(context.Background(), gatewayConfig{CodexHome: home}, true,
		func(ctx context.Context, _ gatewayConfig) error {
			deadline, ok := ctx.Deadline()
			if !ok || time.Until(deadline) > 45*time.Second {
				t.Fatal("refresh must have a 45 second maximum deadline")
			}
			return nil
		})
	if err == nil {
		t.Fatal("refresh leaving an expired token must fail")
	}
}

func TestRenewalProtocolFiltersNotificationsAndErrors(t *testing.T) {
	scanner := bufio.NewScanner(strings.NewReader("startup banner\n{\"method\":\"notification\"}\n{\"id\":1,\"result\":{}}\n{\"id\":2,\"result\":{\"account\":{\"type\":\"chatgpt\"}}}\n"))
	result, err := readCodexRenewalResponse(scanner, 2)
	if err != nil || !strings.Contains(string(result), "chatgpt") {
		t.Fatal("failed to match the renewal response")
	}
	scanner = bufio.NewScanner(strings.NewReader(`{"id":2,"error":{"message":"secret-token-value"}}`))
	_, err = readCodexRenewalResponse(scanner, 2)
	if err == nil || strings.Contains(err.Error(), "secret-token-value") {
		t.Fatal("upstream errors must not leak their raw message")
	}
}

func TestCredentialRefreshThreshold(t *testing.T) {
	now := time.Unix(1800000000, 0)
	for _, test := range []struct {
		delta time.Duration
		want  bool
	}{{-time.Hour, true}, {2 * time.Minute, true}, {3 * time.Minute, false}} {
		token := credentialTestToken(t, "a", now.Add(test.delta))
		if got := codexTokenNeedsRefresh(token, now); got != test.want {
			t.Errorf("expiry delta %s: got %v", test.delta, got)
		}
	}
	if codexTokenNeedsRefresh("opaque-token", now) {
		t.Fatal("opaque tokens should be renewed on a real 401")
	}
}
