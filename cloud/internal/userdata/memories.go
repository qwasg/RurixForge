package userdata

import (
	"context"
	"net/http"
	"strconv"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/jackc/pgx/v5"

	"forge-cloud/internal/core"
	"forge-cloud/internal/httpx"
)

const (
	maxMemoryBatch   = 200
	maxMemoryContent = 8 << 10
	maxActiveMemory  = 2000
	maxMemoryTags    = 32
	maxMemoryPage    = 500
)

// Memory 对应 §3.4 Memory。
type Memory struct {
	ID        string    `json:"id"`
	Scope     string    `json:"scope"`
	Kind      string    `json:"kind"`
	Content   string    `json:"content"`
	Tags      []string  `json:"tags"`
	UpdatedAt time.Time `json:"updatedAt"`
	Deleted   bool      `json:"deleted"`
	Version   int64     `json:"version"`
}

const memorySelect = `SELECT id, scope, kind, content, tags, updated_at, deleted, version, change_seq FROM user_memories`

func scanMemory(row pgx.Row) (Memory, int64, error) {
	var m Memory
	var seq int64
	err := row.Scan(&m.ID, &m.Scope, &m.Kind, &m.Content, &m.Tags, &m.UpdatedAt, &m.Deleted, &m.Version, &seq)
	m.UpdatedAt = m.UpdatedAt.UTC()
	if m.Tags == nil {
		m.Tags = []string{}
	}
	return m, seq, err
}

// parseCursor 解析 ?since= 与 ?limit=（limit 缺省与上限都是 max）。
func parseCursor(r *http.Request, max int) (since int64, limit int) {
	since = httpx.QueryInt64(r, "since")
	if since < 0 {
		since = 0
	}
	limit = max
	if v, err := strconv.Atoi(r.URL.Query().Get("limit")); err == nil && v > 0 && v < max {
		limit = v
	}
	return since, limit
}

func (s *Service) handleGetMemories(w http.ResponseWriter, r *http.Request) {
	c, err := claims(r)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	since, limit := parseCursor(r, maxMemoryPage)
	rows, err := s.db.Query(r.Context(),
		memorySelect+` WHERE user_id = $1 AND change_seq > $2 ORDER BY change_seq LIMIT $3`, c.UserID, since, limit+1)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	defer rows.Close()
	items := []Memory{}
	cursor := since
	hasMore := false
	for rows.Next() {
		m, seq, err := scanMemory(rows)
		if err != nil {
			httpx.WriteError(w, err)
			return
		}
		if len(items) == limit {
			hasMore = true
			break
		}
		items = append(items, m)
		cursor = seq
	}
	if err := rows.Err(); err != nil {
		httpx.WriteError(w, err)
		return
	}
	httpx.WriteJSON(w, http.StatusOK, map[string]any{"items": items, "cursor": cursor, "hasMore": hasMore})
}

type memoryInput struct {
	ID        string    `json:"id"`
	Scope     string    `json:"scope"`
	Kind      string    `json:"kind"`
	Content   string    `json:"content"`
	Tags      []string  `json:"tags"`
	UpdatedAt time.Time `json:"updatedAt"`
	Deleted   bool      `json:"deleted"`
}

func invalidMemory(i int, msg string) error {
	return core.BadRequest("INVALID_MEMORY", "第 "+strconv.Itoa(i+1)+" 条记忆"+msg)
}

func (m *memoryInput) normalize(i int) error {
	m.ID = strings.TrimSpace(m.ID)
	if m.ID == "" || len(m.ID) > 64 {
		return invalidMemory(i, "的 id 为空或过长")
	}
	switch {
	case m.Scope == "global":
	case strings.HasPrefix(m.Scope, "project:") && len(m.Scope) > len("project:") && len(m.Scope) <= 200:
	default:
		return invalidMemory(i, "的 scope 只能是 global 或 project:<key>")
	}
	switch m.Kind {
	case "preference", "fact", "convention":
	default:
		return invalidMemory(i, "的 kind 只能是 preference/fact/convention")
	}
	if m.UpdatedAt.IsZero() {
		return invalidMemory(i, "缺少 updatedAt")
	}
	m.UpdatedAt = normTime(m.UpdatedAt)
	if len(m.Content) > maxMemoryContent {
		return core.E(http.StatusRequestEntityTooLarge, "MEMORY_TOO_LARGE", "第 "+strconv.Itoa(i+1)+" 条记忆超过 8 KiB")
	}
	if len(m.Tags) > maxMemoryTags {
		return invalidMemory(i, "的标签过多")
	}
	tags := make([]string, 0, len(m.Tags))
	for _, t := range m.Tags {
		t = strings.TrimSpace(t)
		if t == "" {
			continue
		}
		if utf8.RuneCountInString(t) > 64 {
			return invalidMemory(i, "的标签过长")
		}
		tags = append(tags, t)
	}
	m.Tags = tags
	if m.Deleted {
		m.Content, m.Tags = "", []string{}
	}
	return nil
}

type storedMemory struct {
	updatedAt time.Time
	deleted   bool
}

// handlePutMemories：服务端无此 id 或 incoming.updatedAt > server.updatedAt 才写入（change_seq 取新值、
// version+1），否则把服务端副本放进 conflicts。返回的 cursor 是该用户写入后的最新 change_seq。
func (s *Service) handlePutMemories(w http.ResponseWriter, r *http.Request) {
	c, err := claims(r)
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	var req struct {
		Items []memoryInput `json:"items"`
	}
	if err := httpx.DecodeJSON(r, &req, 0); err != nil {
		httpx.WriteError(w, err)
		return
	}
	if len(req.Items) > maxMemoryBatch {
		httpx.WriteError(w, core.BadRequest("TOO_MANY_ITEMS", "单次最多提交 200 条记忆"))
		return
	}
	ids := make([]string, 0, len(req.Items))
	for i := range req.Items {
		if err := req.Items[i].normalize(i); err != nil {
			httpx.WriteError(w, err)
			return
		}
		ids = append(ids, req.Items[i].ID)
	}

	ctx := r.Context()
	applied := []string{}
	conflicts := []Memory{}
	var cursor int64
	err = pgx.BeginFunc(ctx, s.db, func(tx pgx.Tx) error {
		if err := lockUser(ctx, tx, c.UserID); err != nil {
			return err
		}
		stored, err := loadStoredMemories(ctx, tx, c.UserID, ids)
		if err != nil {
			return err
		}
		var active int
		if err := tx.QueryRow(ctx,
			`SELECT count(*) FROM user_memories WHERE user_id = $1 AND NOT deleted`, c.UserID).Scan(&active); err != nil {
			return err
		}
		conflictIdx := map[string]int{}
		for _, in := range req.Items {
			old, exists := stored[in.ID]
			if exists && !in.UpdatedAt.After(old.updatedAt) {
				m, _, err := scanMemory(tx.QueryRow(ctx, memorySelect+` WHERE user_id = $1 AND id = $2`, c.UserID, in.ID))
				if err != nil {
					return err
				}
				if i, ok := conflictIdx[in.ID]; ok {
					conflicts[i] = m
				} else {
					conflictIdx[in.ID] = len(conflicts)
					conflicts = append(conflicts, m)
				}
				continue
			}
			if !in.Deleted && (!exists || old.deleted) {
				active++
				if active > maxActiveMemory {
					return core.BadRequest("MEMORY_LIMIT", "有效记忆数量已达上限（2000 条）")
				}
			} else if in.Deleted && exists && !old.deleted {
				active--
			}
			if _, err := tx.Exec(ctx,
				`INSERT INTO user_memories (user_id, id, scope, kind, content, tags, version, updated_at, deleted, change_seq)
				 VALUES ($1, $2, $3, $4, $5, $6, 1, $7, $8, nextval('userdata_change_seq'))
				 ON CONFLICT (user_id, id) DO UPDATE
				 SET scope = EXCLUDED.scope, kind = EXCLUDED.kind, content = EXCLUDED.content, tags = EXCLUDED.tags,
				     version = user_memories.version + 1, updated_at = EXCLUDED.updated_at, deleted = EXCLUDED.deleted,
				     change_seq = EXCLUDED.change_seq`,
				c.UserID, in.ID, in.Scope, in.Kind, in.Content, in.Tags, in.UpdatedAt, in.Deleted); err != nil {
				return err
			}
			stored[in.ID] = storedMemory{updatedAt: in.UpdatedAt, deleted: in.Deleted}
			applied = append(applied, in.ID)
		}
		return tx.QueryRow(ctx,
			`SELECT COALESCE(max(change_seq), 0) FROM user_memories WHERE user_id = $1`, c.UserID).Scan(&cursor)
	})
	if err != nil {
		httpx.WriteError(w, err)
		return
	}
	httpx.WriteJSON(w, http.StatusOK, map[string]any{"applied": applied, "conflicts": conflicts, "cursor": cursor})
}

func loadStoredMemories(ctx context.Context, tx pgx.Tx, userID int64, ids []string) (map[string]storedMemory, error) {
	out := map[string]storedMemory{}
	if len(ids) == 0 {
		return out, nil
	}
	rows, err := tx.Query(ctx,
		`SELECT id, updated_at, deleted FROM user_memories WHERE user_id = $1 AND id = ANY($2) FOR UPDATE`, userID, ids)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	for rows.Next() {
		var id string
		var m storedMemory
		if err := rows.Scan(&id, &m.updatedAt, &m.deleted); err != nil {
			return nil, err
		}
		m.updatedAt = normTime(m.updatedAt)
		out[id] = m
	}
	return out, rows.Err()
}
