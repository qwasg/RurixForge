//go:build embedui

package webui

import (
	"embed"
	"io/fs"
)

//go:embed all:dist
var distFS embed.FS

func embedded() fs.FS {
	sub, err := fs.Sub(distFS, "dist")
	if err != nil {
		return nil
	}
	return sub
}
