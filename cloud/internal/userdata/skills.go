package userdata

import (
	"context"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	"net/http"
	"regexp"
	"sort"
	"strings"
	"time"

	"github.com/go-chi/chi/v5"
	"github.com/jackc/pgx/v5"

	"forge-cloud/internal/core"
	"forge-cloud/internal/httpx"
)

const (
	maxSkillBytes = 2 << 20
	maxSkillFiles = 500
	maxSkillPath  = 256
	maxSkillPage  = 50
	// base64 膨胀约 4/3，再留出 JSON 包装的余量。
	maxSkillBody  = 4 << 20
	skillMainFile = "SKILL.md"
)

var skillNameRe = regexp.MustCompile(`^[a-z0-9][a-z0-9-]{0,63}$`)

// Skill 对应 §3.4 Skill；files 为「相对路径 → base64」，墓碑为 null。
type Skill struct {
	Name      string            `json:"name"`
	Files     map[string]string `json:"files"`
	SHA256    string            `json:"sha256"`
	SizeBytes int64             `json:"sizeBytes"`
	UpdatedAt time.Time         `json:"updatedAt"`
	Deleted   bool              `json:"deleted"`
	Version   int64             `json:"version"`
}

const skillSelect = `SELECT name, files, sha256, size_bytes, updated_at, deleted, version, change_seq FROM user_skills`

func scanSkill(row pgx.Row) (Skill, int64, error) {
	var (
		sk    Skill
		files []byte
		seq   int64
	)
	if err := row.Scan(&sk.Name, &files, &sk.SHA256, &sk.SizeBytes, &sk.UpdatedAt, &sk.Deleted, &sk.Version, &seq); err != nil {
		return sk, 0, err
	}
	sk.UpdatedAt = sk.UpdatedAt.UTC()
	if len(files) > 0 && !sk.Deleted {
		if err := json.Unmarshal(files, &sk.Files); err != nil {
			return sk, 0, err
		}
	}
	return sk, seq, nil
}

func errSkillInvalid(msg string) error { return core.BadRequest("SKILL_INVALID", msg) }

func errSkillTooLarge() error {
	return core.E(http.StatusRequestEntityTooLarge, "SKILL_TOO_LARGE", "技能包解码后不能超过 2 MiB")
}

// validSkillPath：相对的正斜杠路径，不含 ..、.、空段、反斜杠、盘符或控制字符。
func validSkillPath(p string) bool {
	if p == "" || len(p) > maxSkillPath || strings.HasPrefix(p, "/") || strings.ContainsAny(p, "\\:") {
		return false
	}
	for _, r := range p {
		if r < 0x20 || r == 0x7f {
			return false
		}
	}
	for _, seg := range strings.Split(p, "/") {
		if seg == "" || seg == "." || seg == ".." {
			return false
		}
	}
	return true
}

// packSkill 校验并规范化文件表：返回标准 base64 文件表、解码总字节数与内容摘要。
// 摘要 = SHA-256(按路径升序拼接 path + "\x00" + 内容 + "\x00")。
func packSkill(files map[string]string) (map[string]string, int64, string, error) {
	if len(files) == 0 {
		return nil, 0, "", errSkillInvalid("技能包不能为空")
	}
	if len(files) > maxSkillFiles {
		return nil, 0, "", errSkillInvalid("技能包文件过多")
	}
	if _, ok := files[skillMainFile]; !ok {
		return nil, 0, "", errSkillInvalid("技能包必须包含 SKILL.md")
	}
	paths := make([]string, 0, len(files))
	for p := range files {
		if !validSkillPath(p) {
			return nil, 0, "", errSkillInvalid("文件路径非法：" + p)
		}
		paths = append(paths, p)
	}
	sort.Strings(paths)
	out := make(map[string]string, len(files))
	h := sha256.New()
	var total int64
	for _, p := range paths {
		enc := strings.TrimSpace(files[p])
		data, err := base64.StdEncoding.DecodeString(enc)
		if err != nil {
			if data, err = base64.RawStdEncoding.DecodeString(enc); err != nil {
				return nil, 0, "", errSkillInvalid("文件不是合法 base64：" + p)
			}
		}
		total += int64(len(data))
		if total > maxSkillBytes {
			return nil, 0, "", errSkillTooLarge()
		}
		h.Write([]byte(p))
		h.Write([]byte{0})
		h.Write(data)
		h.Write([]byte{0})
		out[p] = base64.StdEncoding.EncodeToString(data)
	}
	return out, total, hex.EncodeToString(h.Sum(nil)), nil
}

func (s *Service) handleGetSkills(w http.ResponseWriter, r *http.Request) {
	c, err := claims(r)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	since, limit := parseCursor(r, maxSkillPage)
	rows, err := s.db.Query(r.Context(),
		skillSelect+` WHERE user_id = $1 AND change_seq > $2 ORDER BY change_seq LIMIT $3`, c.UserID, since, limit+1)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	defer rows.Close()
	items := []Skill{}
	cursor := since
	hasMore := false
	for rows.Next() {
		sk, seq, err := scanSkill(rows)
		if err != nil {
			httpx.WriteError(w, err)
			return
		}
		if len(items) == limit {
			hasMore = true
			break
		}
		items = append(items, sk)
		cursor = seq
	}
	if err := rows.Err(); err != nil {
		httpx.WriteError(w, err)
		return
	}
	httpx.WriteJSON(w, http.StatusOK, map[string]any{"items": items, "cursor": cursor, "hasMore": hasMore})
}

func skillName(r *http.Request) (string, error) {
	name := chi.URLParam(r, "name")
	if !skillNameRe.MatchString(name) {
		return "", errSkillInvalid("技能名需匹配 ^[a-z0-9][a-z0-9-]{0,63}$")
	}
	return name, nil
}

// lockedSkill 在事务内锁定并读取服务端副本（不存在返回 nil）。
func lockedSkill(ctx context.Context, tx pgx.Tx, userID int64, name string) (*Skill, error) {
	sk, _, err := scanSkill(tx.QueryRow(ctx, skillSelect+` WHERE user_id = $1 AND name = $2 FOR UPDATE`, userID, name))
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	return &sk, nil
}

func skillCursor(ctx context.Context, tx pgx.Tx, userID int64) (int64, error) {
	var cursor int64
	err := tx.QueryRow(ctx, `SELECT COALESCE(max(change_seq), 0) FROM user_skills WHERE user_id = $1`, userID).Scan(&cursor)
	return cursor, err
}

// handlePutSkill 整包写入（后写者胜）：服务端副本不早于 incoming.updatedAt 时 applied:false 并返回服务端副本。
func (s *Service) handlePutSkill(w http.ResponseWriter, r *http.Request) {
	c, err := claims(r)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	name, err := skillName(r)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	var req struct {
		Files     map[string]string `json:"files"`
		UpdatedAt time.Time         `json:"updatedAt"`
	}
	if err := httpx.DecodeJSON(r, &req, maxSkillBody); err != nil {
		if e := core.AsError(err); e != nil && e.Status == http.StatusRequestEntityTooLarge {
			err = errSkillTooLarge()
		}
		httpx.WriteError(w, err)
		return
	}
	if req.UpdatedAt.IsZero() {
		httpx.WriteError(w, errSkillInvalid("缺少 updatedAt"))
		return
	}
	updatedAt := normTime(req.UpdatedAt)
	files, size, sum, err := packSkill(req.Files)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	filesJSON, err := json.Marshal(files)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}

	ctx := r.Context()
	var (
		applied bool
		out     Skill
		cursor  int64
	)
	err = pgx.BeginFunc(ctx, s.db, func(tx pgx.Tx) error {
		if err := lockUser(ctx, tx, c.UserID); err != nil {
			return err
		}
		cur, err := lockedSkill(ctx, tx, c.UserID, name)
		if err != nil {
			return err
		}
		if cur != nil && !updatedAt.After(normTime(cur.UpdatedAt)) {
			out = *cur
		} else {
			out, _, err = scanSkill(tx.QueryRow(ctx,
				`INSERT INTO user_skills (user_id, name, files, sha256, size_bytes, version, updated_at, deleted, change_seq)
				 VALUES ($1, $2, $3, $4, $5, 1, $6, FALSE, nextval('userdata_change_seq'))
				 ON CONFLICT (user_id, name) DO UPDATE
				 SET files = EXCLUDED.files, sha256 = EXCLUDED.sha256, size_bytes = EXCLUDED.size_bytes,
				     version = user_skills.version + 1, updated_at = EXCLUDED.updated_at, deleted = FALSE,
				     change_seq = EXCLUDED.change_seq
				 RETURNING name, files, sha256, size_bytes, updated_at, deleted, version, change_seq`,
				c.UserID, name, filesJSON, sum, size, updatedAt))
			if err != nil {
				return err
			}
			applied = true
		}
		cursor, err = skillCursor(ctx, tx, c.UserID)
		return err
	})
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	httpx.WriteJSON(w, http.StatusOK, map[string]any{"applied": applied, "skill": out, "cursor": cursor})
}

// handleDeleteSkill 写墓碑（后写者胜；updatedAt 缺省取当前时间）。服务端副本更新时 applied:false 并附服务端副本。
func (s *Service) handleDeleteSkill(w http.ResponseWriter, r *http.Request) {
	c, err := claims(r)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	name, err := skillName(r)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	updatedAt := normTime(time.Now())
	if v := r.URL.Query().Get("updatedAt"); v != "" {
		t, err := time.Parse(time.RFC3339Nano, v)
		if err != nil {
			httpx.WriteError(w, errSkillInvalid("updatedAt 需为 RFC3339 时间"))
			return
		}
		updatedAt = normTime(t)
	}

	ctx := r.Context()
	var (
		applied bool
		cur     *Skill
		cursor  int64
	)
	err = pgx.BeginFunc(ctx, s.db, func(tx pgx.Tx) error {
		if err := lockUser(ctx, tx, c.UserID); err != nil {
			return err
		}
		var err error
		if cur, err = lockedSkill(ctx, tx, c.UserID, name); err != nil {
			return err
		}
		if cur == nil || updatedAt.After(normTime(cur.UpdatedAt)) {
			if _, err := tx.Exec(ctx,
				`INSERT INTO user_skills (user_id, name, files, sha256, size_bytes, version, updated_at, deleted, change_seq)
				 VALUES ($1, $2, NULL, '', 0, 1, $3, TRUE, nextval('userdata_change_seq'))
				 ON CONFLICT (user_id, name) DO UPDATE
				 SET files = NULL, sha256 = '', size_bytes = 0, version = user_skills.version + 1,
				     updated_at = EXCLUDED.updated_at, deleted = TRUE, change_seq = EXCLUDED.change_seq`,
				c.UserID, name, updatedAt); err != nil {
				return err
			}
			applied = true
		}
		cursor, err = skillCursor(ctx, tx, c.UserID)
		return err
	})
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	resp := map[string]any{"applied": applied, "cursor": cursor}
	if !applied {
		resp["skill"] = cur
	}
	httpx.WriteJSON(w, http.StatusOK, resp)
}
