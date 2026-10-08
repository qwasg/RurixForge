package auth

import (
	"errors"
	"net/http"
	"strconv"
	"strings"
	"time"

	"github.com/golang-jwt/jwt/v5"

	"forge-cloud/internal/core"
)

type accessClaims struct {
	SID   string `json:"sid"`
	Role  string `json:"role"`
	Email string `json:"email"`
	jwt.RegisteredClaims
}

// TokenPair 是登录/刷新返回的令牌对。
type TokenPair struct {
	AccessToken      string    `json:"accessToken"`
	AccessExpiresAt  time.Time `json:"accessExpiresAt"`
	RefreshToken     string    `json:"refreshToken"`
	RefreshExpiresAt time.Time `json:"refreshExpiresAt"`
}

func (s *Service) issueAccess(userID int64, sid, role, email string) (string, time.Time, error) {
	now := time.Now().UTC().Truncate(time.Second)
	exp := now.Add(accessTTL)
	c := accessClaims{
		SID: sid, Role: role, Email: email,
		RegisteredClaims: jwt.RegisteredClaims{
			Subject:   strconv.FormatInt(userID, 10),
			Issuer:    issuer,
			IssuedAt:  jwt.NewNumericDate(now),
			ExpiresAt: jwt.NewNumericDate(exp),
		},
	}
	tok, err := jwt.NewWithClaims(jwt.SigningMethodHS256, c).SignedString(s.cfg.JWTSecret)
	if err != nil {
		return "", time.Time{}, err
	}
	return tok, exp, nil
}

func (s *Service) parseAccess(raw string) (*accessClaims, int64, error) {
	var c accessClaims
	_, err := jwt.ParseWithClaims(raw, &c, func(*jwt.Token) (any, error) { return s.cfg.JWTSecret, nil },
		jwt.WithValidMethods([]string{jwt.SigningMethodHS256.Alg()}),
		jwt.WithIssuer(issuer),
		jwt.WithExpirationRequired(),
	)
	if err != nil {
		return nil, 0, err
	}
	uid, err := strconv.ParseInt(c.Subject, 10, 64)
	if err != nil || uid <= 0 || c.SID == "" {
		return nil, 0, errors.New("access token claims 不完整")
	}
	return &c, uid, nil
}

func newRefreshToken() string { return refreshPrefix + core.RandomString(48) }

func bearerToken(r *http.Request) (string, bool) {
	h := strings.TrimSpace(r.Header.Get("Authorization"))
	if len(h) < 8 || !strings.EqualFold(h[:7], "bearer ") {
		return "", false
	}
	t := strings.TrimSpace(h[7:])
	return t, t != ""
}
