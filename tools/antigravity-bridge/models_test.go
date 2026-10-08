//go:build windows

package main

import (
	"context"
	"io"
	"net/http"
	"strings"
	"testing"

	sdkauth "github.com/router-for-me/CLIProxyAPI/v8/sdk/auth"
	"github.com/router-for-me/CLIProxyAPI/v8/sdk/cliproxy"
	core "github.com/router-for-me/CLIProxyAPI/v8/sdk/cliproxy/auth"
)

type fakeGoogleTransport struct{ paths []string }

func (transport *fakeGoogleTransport) RoundTrip(request *http.Request) (*http.Response, error) {
	transport.paths = append(transport.paths, request.URL.Path)
	body := ""
	switch request.URL.Path {
	case "/token":
		body = `{"access_token":"offline-access","refresh_token":"offline-refresh","expires_in":3600,"token_type":"Bearer"}`
	case "/oauth2/v2/userinfo":
		body = `{"email":"offline@example.test"}`
	case "/v1internal:loadCodeAssist":
		body = `{"cloudaicompanionProject":"offline-project","currentTier":{"id":"test-tier"}}`
	default:
		return &http.Response{StatusCode: 400, Body: io.NopCloser(strings.NewReader(`{}`)), Header: make(http.Header)}, nil
	}
	return &http.Response{StatusCode: 200, Body: io.NopCloser(strings.NewReader(body)), Header: make(http.Header)}, nil
}
func TestOAuthIdentityAndCatalogRestoreWithoutPaidInference(t *testing.T) {
	ctx := context.Background()
	transport := &fakeGoogleTransport{}
	auth, err := sdkauth.CompleteAntigravityOAuth(ctx, "offline-code", sdkauth.AntigravityDefaultCallbackURI(), &http.Client{Transport: transport})
	if err != nil {
		t.Fatal(err)
	}
	if auth.Metadata["email"] != "offline@example.test" || auth.Metadata["project_id"] != "offline-project" {
		t.Fatal("OAuth identity/project discovery failed")
	}
	store := &encryptedStore{dir: t.TempDir()}
	store.beginLogin()
	manager := core.NewManager(store, nil, nil)
	auth, err = manager.Register(ctx, auth)
	if err != nil {
		t.Fatal(err)
	}
	defer cliproxy.GlobalModelRegistry().UnregisterClient(auth.ID)
	if err = cacheCatalog(ctx, manager, auth, []catalogModel{{ID: "gemini-test-flash", Label: "Offline Gemini Flash"}}); err != nil {
		t.Fatal(err)
	}
	cliproxy.GlobalModelRegistry().UnregisterClient(auth.ID)
	restarted := core.NewManager(store, nil, nil)
	if err = restarted.Load(ctx); err != nil {
		t.Fatal(err)
	}
	restoreCatalogs(restarted)
	models := cliproxy.GlobalModelRegistry().GetModelsForClient(auth.ID)
	if len(models) != 1 || models[0].ID != "gemini-test-flash" {
		t.Fatal("model catalog was lost on restart")
	}
	if len(transport.paths) != 3 {
		t.Fatalf("expected OAuth exchange, identity and project requests; got %v", transport.paths)
	}
}
