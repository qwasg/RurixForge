package accounts

import (
	"reflect"
	"testing"

	"forge-cloud/internal/core"
)

func TestSupportsModel(t *testing.T) {
	m := &core.Model{ID: "public-alias", UpstreamModel: "catalog-upstream"}
	for _, tt := range []struct {
		name string
		a    Account
		want bool
	}{
		{"legacy unrestricted", Account{}, true},
		{"catalog ID", Account{AllowedModels: []string{"public-alias"}}, true},
		{"catalog upstream", Account{AllowedModels: []string{"catalog-upstream"}}, true},
		{"account mapping", Account{AllowedModels: []string{"mapped"}, ModelMapping: map[string]string{"public-alias": "mapped"}}, true},
		{"mapping overrides catalog upstream", Account{AllowedModels: []string{"catalog-upstream"}, ModelMapping: map[string]string{"public-alias": "mapped"}}, false},
		{"other model", Account{AllowedModels: []string{"other"}}, false},
	} {
		t.Run(tt.name, func(t *testing.T) {
			if got := tt.a.SupportsModel(m); got != tt.want {
				t.Fatalf("SupportsModel = %v, want %v", got, tt.want)
			}
		})
	}
}

func TestNormalizeAllowedModels(t *testing.T) {
	got, err := normalizeAllowedModels([]string{" gpt-latest ", "claude-latest", "gpt-latest"})
	if err != nil || !reflect.DeepEqual(got, []string{"gpt-latest", "claude-latest"}) {
		t.Fatalf("normalize = %v, %v", got, err)
	}
	for _, ids := range [][]string{{""}, {"gpt invalid"}, {"gpt\u0000invalid"}, make([]string, 501)} {
		if _, err := normalizeAllowedModels(ids); err == nil {
			t.Fatalf("accepted invalid scope: %v", ids)
		}
	}
}
