// Package vault 用主密钥（AES-256-GCM）加密上游凭据。
// 密文格式：1 字节版本（0x01）+ 12 字节随机 nonce + GCM 密文。
package vault

import (
	"crypto/aes"
	"crypto/cipher"
	"crypto/rand"
	"encoding/json"
	"errors"
	"fmt"
)

const version1 = 0x01

type Vault struct {
	aead cipher.AEAD
}

// New 需要 32 字节主密钥。
func New(key []byte) (*Vault, error) {
	if len(key) != 32 {
		return nil, fmt.Errorf("主密钥必须 32 字节，当前 %d", len(key))
	}
	block, err := aes.NewCipher(key)
	if err != nil {
		return nil, err
	}
	aead, err := cipher.NewGCM(block)
	if err != nil {
		return nil, err
	}
	return &Vault{aead: aead}, nil
}

func (v *Vault) Encrypt(plain []byte) ([]byte, error) {
	nonce := make([]byte, v.aead.NonceSize())
	if _, err := rand.Read(nonce); err != nil {
		return nil, err
	}
	out := make([]byte, 0, 1+len(nonce)+len(plain)+v.aead.Overhead())
	out = append(out, version1)
	out = append(out, nonce...)
	return v.aead.Seal(out, nonce, plain, []byte{version1}), nil
}

func (v *Vault) Decrypt(blob []byte) ([]byte, error) {
	ns := v.aead.NonceSize()
	if len(blob) < 1+ns+v.aead.Overhead() || blob[0] != version1 {
		return nil, errors.New("密文格式无效")
	}
	plain, err := v.aead.Open(nil, blob[1:1+ns], blob[1+ns:], []byte{version1})
	if err != nil {
		return nil, errors.New("解密失败（主密钥不匹配或数据损坏）")
	}
	return plain, nil
}

func (v *Vault) EncryptJSON(value any) ([]byte, error) {
	b, err := json.Marshal(value)
	if err != nil {
		return nil, err
	}
	return v.Encrypt(b)
}

func (v *Vault) DecryptJSON(blob []byte, out any) error {
	b, err := v.Decrypt(blob)
	if err != nil {
		return err
	}
	return json.Unmarshal(b, out)
}
