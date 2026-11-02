package main

import (
	"bufio"
	"context"
	"crypto/sha256"
	"encoding/base64"
	"encoding/json"
	"errors"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"sync"
	"time"
)

// Only the credentials needed for an inference request leave this module.
// Refresh tokens stay in the existing Codex auth.json and are managed by Codex.
type codexCredentials struct {
	AccessToken string
	AccountID   string
}

type credentialRefreshState struct {
	gate       chan struct{}
	lastHash   [32]byte
	lastUpdate time.Time
}

var credentialRefreshStates sync.Map

func loadCodexCredentials(ctx context.Context, config gatewayConfig, force bool) (codexCredentials, error) {
	return loadCodexCredentialsWithRefresh(ctx, config, force, refreshCodexAccount)
}

func loadCodexCredentialsWithRefresh(ctx context.Context, config gatewayConfig, force bool, refresh func(context.Context, gatewayConfig) error) (codexCredentials, error) {
	var empty codexCredentials
	if err := ctx.Err(); err != nil {
		return empty, errors.New("Codex credential request cancelled")
	}
	before, err := readCodexCredentials(config.CodexHome)
	if err != nil {
		return empty, err
	}
	if !force && !codexTokenNeedsRefresh(before.AccessToken, time.Now()) {
		return before, nil
	}
	stateValue, _ := credentialRefreshStates.LoadOrStore(filepath.Clean(config.CodexHome), &credentialRefreshState{gate: make(chan struct{}, 1)})
	state := stateValue.(*credentialRefreshState)
	select {
	case state.gate <- struct{}{}:
		defer func() { <-state.gate }()
	case <-ctx.Done():
		return empty, errors.New("Codex credential request cancelled")
	}

	// Forge or another gateway request may have renewed the same auth.json while
	// this request was waiting. Never refresh an obsolete snapshot of the file.
	current, err := readCodexCredentials(config.CodexHome)
	if err != nil {
		return empty, err
	}
	if current.AccountID != before.AccountID {
		return empty, errors.New("Codex account changed while loading credentials; retry the request")
	}
	needsRefresh := codexTokenNeedsRefresh(current.AccessToken, time.Now())
	if current.AccessToken != before.AccessToken && !needsRefresh {
		return current, nil
	}
	if !force && !needsRefresh {
		return current, nil
	}
	// A burst of 401 responses can arrive just after one refresh finishes. A
	// short coalescing window avoids repeatedly rotating the same new token.
	if force && !needsRefresh && time.Since(state.lastUpdate) < 5*time.Second && state.lastHash == sha256.Sum256([]byte(current.AccessToken)) {
		return current, nil
	}
	refreshCtx, cancel := context.WithTimeout(ctx, 45*time.Second)
	defer cancel()
	if err := refresh(refreshCtx, config); err != nil {
		return empty, err
	}
	after, err := readCodexCredentials(config.CodexHome)
	if err != nil {
		return empty, err
	}
	if after.AccountID != current.AccountID {
		return empty, errors.New("Codex account changed during credential renewal; retry the request")
	}
	if codexTokenNeedsRefresh(after.AccessToken, time.Now()) {
		return empty, errors.New("Codex did not renew the expiring session; check the existing RurixForge Codex login")
	}
	state.lastHash = sha256.Sum256([]byte(after.AccessToken))
	state.lastUpdate = time.Now()
	return after, nil
}

func readCodexCredentials(home string) (codexCredentials, error) {
	var empty codexCredentials
	if !filepath.IsAbs(home) || validateCredentialPath(home, true) != nil {
		return empty, errors.New("Codex home must be an existing absolute directory without symlinks")
	}
	path := filepath.Join(home, "auth.json")
	if validateCredentialPath(path, false) != nil {
		return empty, errors.New("The existing Codex auth.json is unavailable or is not a regular file")
	}
	f, err := os.Open(path)
	if err != nil {
		return empty, errors.New("Cannot read the existing Codex auth.json")
	}
	defer f.Close()
	info, err := f.Stat()
	if err != nil || !info.Mode().IsRegular() || info.Size() > 1<<20 {
		return empty, errors.New("The existing Codex auth.json has an invalid file type or size")
	}
	currentInfo, err := os.Lstat(path)
	if err != nil || !os.SameFile(info, currentInfo) || currentInfo.Mode()&os.ModeSymlink != 0 {
		return empty, errors.New("The existing Codex auth.json changed while opening it")
	}
	var auth struct {
		AuthMode string `json:"auth_mode"`
		Tokens   struct {
			AccessToken string `json:"access_token"`
			AccountID   string `json:"account_id"`
			IDToken     string `json:"id_token"`
		} `json:"tokens"`
	}
	if err := json.NewDecoder(io.LimitReader(f, 1<<20)).Decode(&auth); err != nil {
		return empty, errors.New("The existing Codex auth.json is not valid JSON")
	}
	if auth.AuthMode != "" && auth.AuthMode != "chatgpt" {
		return empty, errors.New("The existing Codex session is not a ChatGPT login")
	}
	accountID := auth.Tokens.AccountID
	tokenAccount := codexJWTAccountID(auth.Tokens.AccessToken)
	if tokenAccount == "" {
		tokenAccount = codexJWTAccountID(auth.Tokens.IDToken)
	}
	if accountID == "" {
		accountID = tokenAccount
	} else if tokenAccount != "" && tokenAccount != accountID {
		return empty, errors.New("The existing Codex session has inconsistent account identifiers")
	}
	if !validCredentialHeader(auth.Tokens.AccessToken) || !validCredentialHeader(accountID) {
		return empty, errors.New("The existing Codex session is missing valid access credentials")
	}
	return codexCredentials{AccessToken: auth.Tokens.AccessToken, AccountID: accountID}, nil
}

func validCredentialHeader(s string) bool {
	return s != "" && len(s) <= 128<<10 && !strings.ContainsAny(s, "\r\n\t ")
}

// Check every path component so a junction/symlink cannot redirect credential
// access or executable selection away from the explicitly configured paths.
func validateCredentialPath(path string, directory bool) error {
	if !filepath.IsAbs(path) {
		return errors.New("absolute path required")
	}
	for p := filepath.Clean(path); ; p = filepath.Dir(p) {
		info, err := os.Lstat(p)
		if err != nil || info.Mode()&os.ModeSymlink != 0 {
			return errors.New("unavailable or redirected path")
		}
		if p == filepath.Clean(path) && ((directory && !info.IsDir()) || (!directory && !info.Mode().IsRegular())) {
			return errors.New("unexpected path type")
		}
		if parent := filepath.Dir(p); parent == p {
			return nil
		}
	}
}

func codexJWTClaims(token string) map[string]json.RawMessage {
	parts := strings.Split(token, ".")
	if len(parts) != 3 || len(parts[1]) > 128<<10 {
		return nil
	}
	payload, err := base64.RawURLEncoding.DecodeString(strings.TrimRight(parts[1], "="))
	if err != nil {
		return nil
	}
	var claims map[string]json.RawMessage
	if json.Unmarshal(payload, &claims) != nil {
		return nil
	}
	return claims
}

func codexJWTAccountID(token string) string {
	claims := codexJWTClaims(token)
	var auth struct {
		AccountID string `json:"chatgpt_account_id"`
	}
	if json.Unmarshal(claims["https://api.openai.com/auth"], &auth) != nil {
		return ""
	}
	return auth.AccountID
}

func codexTokenNeedsRefresh(token string, now time.Time) bool {
	claims := codexJWTClaims(token)
	var exp int64
	if json.Unmarshal(claims["exp"], &exp) != nil || exp <= 0 {
		// Opaque tokens cannot be preflighted; a real 401 still requests refresh.
		return false
	}
	return !time.Unix(exp, 0).After(now.Add(2 * time.Minute))
}

func codexRefreshEnvironment(home string) []string {
	out := make([]string, 0, len(os.Environ())+1)
	for _, value := range os.Environ() {
		name, _, _ := strings.Cut(value, "=")
		switch strings.ToUpper(name) {
		case "CODEX_HOME", "OPENAI_API_KEY", "OPENAI_BASE_URL", "CODEX_API_KEY", "FORGE_CLOUD_API_KEY", "RURIXFORGE_API_KEY":
			continue
		}
		out = append(out, value)
	}
	return append(out, "CODEX_HOME="+home)
}

func refreshCodexAccount(ctx context.Context, config gatewayConfig) error {
	if validateCredentialPath(config.CodexBin, false) != nil {
		return errors.New("Configured Codex executable is unavailable or is not a regular absolute path")
	}
	cmd := exec.CommandContext(ctx, config.CodexBin, "app-server", "--listen", "stdio://", "-c", `model_provider="openai"`)
	cmd.Env = codexRefreshEnvironment(config.CodexHome)
	cmd.Dir = config.CodexHome
	cmd.Stderr = io.Discard
	cmd.WaitDelay = 2 * time.Second
	hideCodexRefreshWindow(cmd)
	stdin, err := cmd.StdinPipe()
	if err != nil {
		return errors.New("Cannot open Codex renewal input")
	}
	stdout, err := cmd.StdoutPipe()
	if err != nil {
		stdin.Close()
		return errors.New("Cannot open Codex renewal output")
	}
	if err := cmd.Start(); err != nil {
		stdin.Close()
		stdout.Close()
		return errors.New("Cannot start the configured Codex executable for session renewal")
	}
	defer func() {
		stdin.Close()
		stdout.Close()
		_ = cmd.Process.Kill()
		_ = cmd.Wait()
	}()
	encoder := json.NewEncoder(stdin)
	scanner := bufio.NewScanner(stdout)
	scanner.Buffer(make([]byte, 4096), 1<<20)
	if err := encoder.Encode(map[string]any{
		"id": 1, "method": "initialize",
		"params": map[string]any{"clientInfo": map[string]any{"name": "rurixforge_local_api", "title": "RurixForge local API", "version": "1.0.0"}},
	}); err != nil {
		return errors.New("Cannot initialize Codex session renewal")
	}
	if _, err := readCodexRenewalResponse(scanner, 1); err != nil {
		return errors.New("Codex session renewal initialization failed")
	}
	if encoder.Encode(map[string]any{"method": "initialized"}) != nil || encoder.Encode(map[string]any{
		"id": 2, "method": "account/read", "params": map[string]any{"refreshToken": true},
	}) != nil {
		return errors.New("Cannot request existing Codex session renewal")
	}
	result, err := readCodexRenewalResponse(scanner, 2)
	if err != nil {
		if ctx.Err() != nil {
			return errors.New("Codex session renewal timed out or was cancelled")
		}
		return errors.New("Existing Codex session renewal failed; check the RurixForge Codex login")
	}
	var account struct {
		Account json.RawMessage `json:"account"`
	}
	if json.Unmarshal(result, &account) != nil || len(account.Account) == 0 || string(account.Account) == "null" {
		return errors.New("The existing RurixForge Codex session is not signed in")
	}
	return nil
}

func readCodexRenewalResponse(scanner *bufio.Scanner, id int) (json.RawMessage, error) {
	for scanner.Scan() {
		var message struct {
			ID     json.RawMessage `json:"id"`
			Result json.RawMessage `json:"result"`
			Error  json.RawMessage `json:"error"`
		}
		if json.Unmarshal(scanner.Bytes(), &message) != nil {
			continue
		}
		var receivedID int
		if json.Unmarshal(message.ID, &receivedID) != nil || receivedID != id {
			continue
		}
		if (len(message.Error) != 0 && string(message.Error) != "null") || len(message.Result) == 0 {
			return nil, errors.New("Codex renewal RPC failed")
		}
		return message.Result, nil
	}
	return nil, errors.New("Codex renewal RPC closed before responding")
}
