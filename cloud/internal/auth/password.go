package auth

import (
	"crypto/rand"
	"crypto/subtle"
	"encoding/base64"
	"fmt"
	"net/mail"
	"strings"
	"sync"
	"unicode"
	"unicode/utf8"

	"golang.org/x/crypto/argon2"

	"forge-cloud/internal/core"
)

const (
	argonTime    = 3
	argonMemory  = 64 * 1024
	argonThreads = 2
	argonKeyLen  = 32
	argonSaltLen = 16

	minPasswordLen = 8
	maxPasswordLen = 128
	maxNicknameLen = 32
)

// HashPassword 生成 argon2id 的 PHC 串：$argon2id$v=19$m=65536,t=3,p=2$<salt>$<hash>。
func HashPassword(password string) (string, error) {
	salt := make([]byte, argonSaltLen)
	if _, err := rand.Read(salt); err != nil {
		return "", err
	}
	key := argon2.IDKey([]byte(password), salt, argonTime, argonMemory, argonThreads, argonKeyLen)
	return fmt.Sprintf("$argon2id$v=%d$m=%d,t=%d,p=%d$%s$%s", argon2.Version, argonMemory, argonTime, argonThreads,
		base64.RawStdEncoding.EncodeToString(salt), base64.RawStdEncoding.EncodeToString(key)), nil
}

// VerifyPassword 按 PHC 串里记录的参数校验（常量时间比较）。
func VerifyPassword(password, encoded string) bool {
	parts := strings.Split(encoded, "$")
	if len(parts) != 6 || parts[1] != "argon2id" {
		return false
	}
	var version int
	if _, err := fmt.Sscanf(parts[2], "v=%d", &version); err != nil || version != argon2.Version {
		return false
	}
	var mem, iters uint32
	var threads uint8
	if _, err := fmt.Sscanf(parts[3], "m=%d,t=%d,p=%d", &mem, &iters, &threads); err != nil || mem == 0 || iters == 0 || threads == 0 {
		return false
	}
	salt, err := base64.RawStdEncoding.DecodeString(parts[4])
	if err != nil {
		return false
	}
	want, err := base64.RawStdEncoding.DecodeString(parts[5])
	if err != nil || len(want) == 0 {
		return false
	}
	got := argon2.IDKey([]byte(password), salt, iters, mem, threads, uint32(len(want)))
	return subtle.ConstantTimeCompare(got, want) == 1
}

var (
	dummyOnce sync.Once
	dummyHash string
)

// burnPasswordCheck 在用户不存在时也做一次等价的哈希计算，避免按耗时枚举邮箱。
func burnPasswordCheck(password string) {
	dummyOnce.Do(func() { dummyHash, _ = HashPassword("forge-cloud-timing-equalizer") })
	VerifyPassword(password, dummyHash)
}

// NormalizeEmail 小写并做基本格式校验；非法 → 400 INVALID_EMAIL。
func NormalizeEmail(s string) (string, error) {
	e := strings.ToLower(strings.TrimSpace(s))
	bad := core.BadRequest("INVALID_EMAIL", "邮箱格式不正确")
	if len(e) < 3 || len(e) > 254 {
		return "", bad
	}
	addr, err := mail.ParseAddress(e)
	if err != nil || addr.Address != e {
		return "", bad
	}
	at := strings.LastIndexByte(e, '@')
	domain := e[at+1:]
	if at <= 0 || !strings.Contains(domain, ".") || strings.HasPrefix(domain, ".") || strings.HasSuffix(domain, ".") {
		return "", bad
	}
	return e, nil
}

// ValidatePassword：8–128 个字符；否则 400 WEAK_PASSWORD。
func ValidatePassword(p string) error {
	n := utf8.RuneCountInString(p)
	if n < minPasswordLen || n > maxPasswordLen {
		return core.BadRequest("WEAK_PASSWORD", fmt.Sprintf("密码长度需为 %d–%d 个字符", minPasswordLen, maxPasswordLen))
	}
	return nil
}

// ValidateNickname 去首尾空白，≤ 32 字且不含控制字符；否则 400 INVALID_NICKNAME。
func ValidateNickname(s string) (string, error) {
	n := strings.TrimSpace(s)
	if utf8.RuneCountInString(n) > maxNicknameLen {
		return "", core.BadRequest("INVALID_NICKNAME", fmt.Sprintf("昵称不能超过 %d 个字", maxNicknameLen))
	}
	for _, r := range n {
		if unicode.IsControl(r) {
			return "", core.BadRequest("INVALID_NICKNAME", "昵称含有非法字符")
		}
	}
	return n, nil
}
