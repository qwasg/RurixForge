//go:build !embedui

package webui

import "io/fs"

func embedded() fs.FS { return nil }
