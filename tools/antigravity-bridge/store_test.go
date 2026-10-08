//go:build windows

package main

import (
	"bytes"
	"context"
	"os"
	"testing"

	core "github.com/router-for-me/CLIProxyAPI/v8/sdk/cliproxy/auth"
)

func TestCredentialEncryptedAndRefreshed(t *testing.T) {
	s := &encryptedStore{dir: t.TempDir()}
	s.beginLogin()
	ctx := context.Background()
	auth := &core.Auth{ID: "antigravity-person.json", Provider: "antigravity", Metadata: map[string]any{"access_token": "private-access-token", "refresh_token": "private-refresh-token", "email": "person@example.test"}}
	if path, err := s.Save(ctx, auth); err != nil || path != "" {
		t.Fatalf("save path=%q error=%v", path, err)
	}
	data, _ := os.ReadFile(s.path(auth.ID))
	if bytes.Contains(data, []byte("private-")) || bytes.Contains(data, []byte("example.test")) {
		t.Fatal("credential leaked into file")
	}
	auth.Metadata["access_token"] = "new-private-token"
	if _, err := s.Save(ctx, auth); err != nil {
		t.Fatal(err)
	}
	loaded, err := s.List(ctx)
	if err != nil || len(loaded) != 1 || loaded[0].Metadata["access_token"] != "new-private-token" {
		t.Fatalf("refresh not persisted: %v", err)
	}
	if err = s.Delete(ctx, auth.ID); err != nil {
		t.Fatal(err)
	}
	loaded, err = s.List(ctx)
	if err != nil || len(loaded) != 0 {
		t.Fatal("logout retained credentials")
	}
}
func TestCancelledLoginRollsBackNewCredentialsAndBlocksLateSave(t *testing.T) {
	s := &encryptedStore{dir: t.TempDir()}
	s.beginLogin()
	auth := &core.Auth{ID: "late.json", Provider: "antigravity", Metadata: map[string]any{"access_token": "late-token"}}
	ctx := core.WithAuthCreationIntent(context.Background())
	if _, err := s.Save(ctx, auth); err != nil {
		t.Fatal(err)
	}
	if err := s.finishLogin(true); err != nil {
		t.Fatal(err)
	}
	for _, context := range []context.Context{ctx, context.Background()} {
		if _, err := s.Save(context, auth); err == nil {
			t.Fatal("late callback recreated cancelled credential")
		}
	}
	records, err := s.List(context.Background())
	if err != nil || len(records) != 0 {
		t.Fatal("cancelled credential retained")
	}
}
func TestCredentialRejectsCorruptionAndOtherProviders(t *testing.T) {
	s := &encryptedStore{dir: t.TempDir()}
	if _, err := s.Save(context.Background(), &core.Auth{ID: "other", Provider: "codex"}); err == nil {
		t.Fatal("accepted unrelated credentials")
	}
	_ = os.WriteFile(s.path("broken"), []byte("invalid"), 0600)
	if _, err := s.List(context.Background()); err == nil {
		t.Fatal("ignored damaged encrypted credential")
	}
}
