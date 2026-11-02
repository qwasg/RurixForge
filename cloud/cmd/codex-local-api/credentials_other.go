//go:build !windows

package main

import "os/exec"

func hideCodexRefreshWindow(cmd *exec.Cmd) {}
