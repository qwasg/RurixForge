package main

import (
	"errors"
	"unsafe"

	"golang.org/x/sys/windows"
)

func crypt(data []byte, decrypt bool) ([]byte, error) {
	if len(data) == 0 {
		return nil, errors.New("empty credential")
	}
	input := windows.DataBlob{Size: uint32(len(data)), Data: &data[0]}
	var output windows.DataBlob
	var err error
	if decrypt {
		err = windows.CryptUnprotectData(&input, nil, nil, 0, nil, windows.CRYPTPROTECT_UI_FORBIDDEN, &output)
	} else {
		err = windows.CryptProtectData(&input, nil, nil, 0, nil, windows.CRYPTPROTECT_UI_FORBIDDEN, &output)
	}
	if err != nil {
		return nil, err
	}
	defer windows.LocalFree(windows.Handle(unsafe.Pointer(output.Data)))
	return append([]byte(nil), unsafe.Slice(output.Data, int(output.Size))...), nil
}
func protect(data []byte) ([]byte, error)   { return crypt(data, false) }
func unprotect(data []byte) ([]byte, error) { return crypt(data, true) }
