package main

import (
	"context"
	"encoding/json"
	"errors"
	"maps"
	"strings"

	"github.com/router-for-me/CLIProxyAPI/v8/sdk/cliproxy"
	core "github.com/router-for-me/CLIProxyAPI/v8/sdk/cliproxy/auth"
)

type catalogModel struct {
	ID    string `json:"upstreamId"`
	Label string `json:"label"`
}

func registerCatalog(manager *core.Manager, auth *core.Auth, catalog []catalogModel) error {
	if len(catalog) == 0 || len(catalog) > 256 {
		return errors.New("empty or oversized model catalog")
	}
	models := make([]*cliproxy.ModelInfo, 0, len(catalog))
	for _, model := range catalog {
		if model.ID == "" || len(model.ID) > 200 || strings.ContainsAny(model.ID, "\r\n\x00") {
			return errors.New("invalid model ID")
		}
		models = append(models, &cliproxy.ModelInfo{ID: model.ID, Object: "model", Type: "antigravity", OwnedBy: "google", DisplayName: model.Label})
	}
	cliproxy.GlobalModelRegistry().RegisterClient(auth.ID, "antigravity", models)
	manager.RefreshSchedulerEntry(auth.ID)
	return nil
}
func cacheCatalog(ctx context.Context, manager *core.Manager, auth *core.Auth, catalog []catalogModel) error {
	if err := registerCatalog(manager, auth, catalog); err != nil {
		return err
	}
	// Metadata maps in Auth.Clone are shared, so replace the map before changing it.
	auth.Metadata = maps.Clone(auth.Metadata)
	auth.Metadata["forge_models"] = catalog
	_, err := manager.Update(ctx, auth)
	return err
}
func restoreCatalogs(manager *core.Manager) {
	for _, auth := range manager.List() {
		if auth.Provider != "antigravity" || auth.Disabled {
			continue
		}
		encoded, err := json.Marshal(auth.Metadata["forge_models"])
		if err != nil {
			continue
		}
		var catalog []catalogModel
		if json.Unmarshal(encoded, &catalog) == nil {
			_ = registerCatalog(manager, auth, catalog)
		}
	}
}
