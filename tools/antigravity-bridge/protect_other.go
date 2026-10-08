//go:build !windows

package main

import "errors"

// Do not silently persist subscription tokens unencrypted on unsupported hosts.
func protect([]byte) ([]byte, error) {
	return nil, errors.New("encrypted credentials require Windows DPAPI")
}
func unprotect([]byte) ([]byte, error) {
	return nil, errors.New("encrypted credentials require Windows DPAPI")
}
