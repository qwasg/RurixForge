package main

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"sync"

	core "github.com/router-for-me/CLIProxyAPI/v8/sdk/cliproxy/auth"
)

// OAuth credentials never enter the SDK's plaintext auth directory or file watcher.
// Returning no file path lets its post-persist hook register the in-memory record.
type encryptedStore struct {
	dir           string
	mu            sync.Mutex
	allowCreation bool
	beforeLogin   map[string][]byte
}

func (s *encryptedStore) beginLogin() {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.allowCreation = true
	s.beforeLogin = make(map[string][]byte)
}
func (s *encryptedStore) finishLogin(cancel bool) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.allowCreation = false
	if cancel {
		for id, previous := range s.beforeLogin {
			var err error
			if previous == nil {
				err = os.Remove(s.path(id))
			} else {
				err = os.WriteFile(s.path(id), previous, 0600)
			}
			if err != nil && !os.IsNotExist(err) {
				return err
			}
		}
	}
	s.beforeLogin = nil
	return nil
}

func (s *encryptedStore) path(id string) string {
	digest := sha256.Sum256([]byte(id))
	return filepath.Join(s.dir, hex.EncodeToString(digest[:])+".credential")
}
func (s *encryptedStore) Save(ctx context.Context, auth *core.Auth) (string, error) {
	if err := ctx.Err(); err != nil {
		return "", err
	}
	if auth == nil || auth.ID == "" || auth.Provider != "antigravity" {
		return "", errors.New("invalid credential")
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	previous, readErr := os.ReadFile(s.path(auth.ID))
	if core.HasAuthCreationIntent(ctx) {
		if !s.allowCreation {
			return "", errors.New("authorization cancelled")
		}
		if _, tracked := s.beforeLogin[auth.ID]; !tracked {
			s.beforeLogin[auth.ID] = previous
		}
	} else if os.IsNotExist(readErr) && !s.allowCreation {
		// An SDK post-persist/refresh task must not recreate a cancelled credential.
		return "", errors.New("credential removed")
	}
	plain, err := json.Marshal(auth)
	if err != nil {
		return "", err
	}
	cipher, err := protect(plain)
	if err != nil {
		return "", err
	}
	if err = os.MkdirAll(s.dir, 0700); err != nil {
		return "", err
	}
	tmp, err := os.CreateTemp(s.dir, ".credential-*")
	if err != nil {
		return "", err
	}
	name := tmp.Name()
	defer os.Remove(name)
	_ = tmp.Chmod(0600)
	_, err = tmp.Write(cipher)
	if err == nil {
		err = tmp.Sync()
	}
	closeErr := tmp.Close()
	if err == nil {
		err = closeErr
	}
	if err == nil {
		err = os.Rename(name, s.path(auth.ID))
	}
	return "", err
}
func (s *encryptedStore) List(ctx context.Context) ([]*core.Auth, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	entries, err := os.ReadDir(s.dir)
	if os.IsNotExist(err) {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	var records []*core.Auth
	for _, entry := range entries {
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		if entry.IsDir() || filepath.Ext(entry.Name()) != ".credential" {
			continue
		}
		cipher, err := os.ReadFile(filepath.Join(s.dir, entry.Name()))
		if err != nil {
			return nil, err
		}
		plain, err := unprotect(cipher)
		if err != nil {
			return nil, errors.New("credential decryption failed")
		}
		var auth core.Auth
		if json.Unmarshal(plain, &auth) != nil || auth.ID == "" || auth.Provider != "antigravity" {
			return nil, errors.New("invalid stored credential")
		}
		auth.FileName = auth.ID
		records = append(records, &auth)
	}
	return records, nil
}
func (s *encryptedStore) Delete(ctx context.Context, id string) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	err := os.Remove(s.path(id))
	if os.IsNotExist(err) {
		return nil
	}
	return err
}
