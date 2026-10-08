import { useCallback, useEffect, useRef, useState } from 'react';
import { Pencil, Plus, Search, Trash2 } from 'lucide-react';
import { cn } from '@/lib/cn';
import { useToastStore } from '@/lib/toastStore';
import {
  createMemory,
  deleteMemory,
  errorMessage,
  formatTime,
  listMemories,
  patchMemory,
  postAccountConfig,
  type LocalMemory,
  type MemoryKind,
  type MemoryListResponse,
  type MemoryScopeFilter,
} from '@/lib/accountApi';
import { useAccountStore } from '@/lib/accountStore';
import { SetCard, SetH1, SetRow, SetToggle, SmBtn } from './controls';

/**
 * 记忆页(D-041,15 §8.3):本机记忆库的检索 / 增删改;Agent 每轮取最相关的若干条注入上下文。
 * 「随账号同步」开关写 cloud-config 的 sync.memory,上次同步时间取自列表响应。
 */

export const MEMORY_SEARCH_DEBOUNCE_MS = 300;
const MEMORY_CONTENT_MAX_BYTES = 8 * 1024;

const KIND_OPTIONS: Array<{ id: MemoryKind; label: string }> = [
  { id: 'preference', label: '偏好' },
  { id: 'fact', label: '事实' },
  { id: 'convention', label: '约定' },
];

const SCOPE_FILTERS: Array<{ id: MemoryScopeFilter; label: string }> = [
  { id: 'all', label: '全部' },
  { id: 'global', label: '全局' },
  { id: 'project', label: '项目' },
];

interface Draft {
  id: string | null;
  content: string;
  kind: MemoryKind;
  scope: 'global' | 'project';
  tags: string;
}

function toast(kind: 'success' | 'error', msg: string): void {
  useToastStore.getState().push(kind, msg);
}

function kindLabel(kind: string): string {
  return KIND_OPTIONS.find((k) => k.id === kind)?.label ?? kind;
}

function scopeLabel(scope: string | undefined, projectKey?: string): string {
  if (!scope?.startsWith('project')) return '全局';
  const key = scope.slice('project:'.length);
  if (key === '' || key === projectKey) return '当前项目';
  return `项目 ${key}`;
}

/** 「a, b，c d」→ ['a','b','c','d'](去空去重)。 */
export function parseTags(raw: string): string[] {
  const out: string[] = [];
  for (const t of raw.split(/[,，\s]+/)) {
    const v = t.trim().replace(/^#/, '');
    if (v !== '' && !out.includes(v)) out.push(v);
  }
  return out;
}

function asKind(kind: string): MemoryKind {
  return kind === 'fact' || kind === 'convention' ? kind : 'preference';
}

function Badge({ children, testId, title }: { children: React.ReactNode; testId?: string; title?: string }) {
  return (
    <span
      data-testid={testId}
      title={title}
      className="flex h-[18px] items-center rounded-full bg-shell-active px-1.5 text-[10px] text-fg-3"
    >
      {children}
    </span>
  );
}

const fieldCls =
  'rounded-md border border-edge bg-shell-panel px-2 py-[5px] text-[12px] text-fg outline-none placeholder:text-fg-4 focus:border-edge-strong';

function MemoryDialog({
  draft,
  saving,
  error,
  onChange,
  onSave,
  onCancel,
}: {
  draft: Draft;
  saving: boolean;
  error: string | null;
  onChange: (next: Draft) => void;
  onSave: () => void;
  onCancel: () => void;
}) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onCancel();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onCancel]);

  const title = draft.id ? '编辑记忆' : '新建记忆';
  return (
    <div
      className="fixed inset-0 z-[60] flex items-center justify-center bg-black/30 p-4"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onCancel();
      }}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label={title}
        data-testid="memory-dialog"
        className="forge-pop-in flex w-[480px] max-w-full flex-col gap-3 rounded-xl border border-edge-strong bg-shell-float p-5 text-fg shadow-float"
      >
        <div className="text-[14px] font-medium">{title}</div>
        <label className="flex flex-col gap-1 text-[11.5px] text-fg-3">
          内容
          <textarea
            data-testid="memory-content"
            autoFocus
            rows={5}
            value={draft.content}
            onChange={(e) => onChange({ ...draft, content: e.target.value })}
            placeholder="例如:本项目的 UI 文案一律使用简体中文"
            className={cn(fieldCls, 'resize-y leading-[18px]')}
          />
        </label>
        <div className="flex gap-3">
          <label className="flex flex-1 flex-col gap-1 text-[11.5px] text-fg-3">
            类型
            <select
              data-testid="memory-kind"
              value={draft.kind}
              onChange={(e) => onChange({ ...draft, kind: asKind(e.target.value) })}
              className={fieldCls}
            >
              {KIND_OPTIONS.map((k) => (
                <option key={k.id} value={k.id}>
                  {k.label}
                </option>
              ))}
            </select>
          </label>
          <label className="flex flex-1 flex-col gap-1 text-[11.5px] text-fg-3">
            范围
            <select
              data-testid="memory-scope"
              value={draft.scope}
              disabled={draft.id !== null}
              title={draft.id !== null ? '已有记忆不能改范围' : undefined}
              onChange={(e) => onChange({ ...draft, scope: e.target.value === 'project' ? 'project' : 'global' })}
              className={cn(fieldCls, draft.id !== null && 'opacity-60')}
            >
              <option value="global">全局(所有项目)</option>
              <option value="project">当前项目</option>
            </select>
          </label>
        </div>
        <label className="flex flex-col gap-1 text-[11.5px] text-fg-3">
          标签
          <input
            data-testid="memory-tags"
            value={draft.tags}
            onChange={(e) => onChange({ ...draft, tags: e.target.value })}
            placeholder="用逗号或空格分隔,可留空"
            className={fieldCls}
          />
        </label>
        {error && (
          <div data-testid="memory-dialog-error" className="text-[11.5px] text-danger">
            {error}
          </div>
        )}
        <div className="flex justify-end gap-2">
          <SmBtn label="取消" testId="memory-cancel" onClick={onCancel} />
          <SmBtn accent label={saving ? '保存中…' : '保存'} testId="memory-save" disabled={saving} onClick={onSave} />
        </div>
      </div>
    </div>
  );
}

export default function MemoryPage() {
  const status = useAccountStore((st) => st.status);
  const refreshStatus = useAccountStore((st) => st.refreshStatus);
  const [query, setQuery] = useState('');
  const [appliedQuery, setAppliedQuery] = useState('');
  const [scope, setScope] = useState<MemoryScopeFilter>('all');
  const [data, setData] = useState<MemoryListResponse | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [saving, setSaving] = useState(false);
  const [draftError, setDraftError] = useState<string | null>(null);
  const [confirmId, setConfirmId] = useState<string | null>(null);
  const [syncBusy, setSyncBusy] = useState(false);
  const seq = useRef(0);

  useEffect(() => {
    if (useAccountStore.getState().status === null) void refreshStatus();
  }, [refreshStatus]);

  useEffect(() => {
    const t = setTimeout(() => setAppliedQuery(query.trim()), MEMORY_SEARCH_DEBOUNCE_MS);
    return () => clearTimeout(t);
  }, [query]);

  const load = useCallback(async () => {
    const my = ++seq.current;
    setLoading(true);
    try {
      const r = await listMemories({ q: appliedQuery, scope });
      if (my !== seq.current) return;
      setData({ ...r, items: Array.isArray(r?.items) ? r.items : [] });
      setError(null);
    } catch (err) {
      if (my === seq.current) setError(errorMessage(err));
    } finally {
      if (my === seq.current) setLoading(false);
    }
  }, [appliedQuery, scope]);

  useEffect(() => {
    void load();
  }, [load]);

  const openCreate = () => {
    setDraftError(null);
    setDraft({ id: null, content: '', kind: 'preference', scope: scope === 'project' ? 'project' : 'global', tags: '' });
  };

  const openEdit = (m: LocalMemory) => {
    setDraftError(null);
    setDraft({
      id: m.id,
      content: m.content,
      kind: asKind(m.kind),
      scope: m.scope?.startsWith('project') ? 'project' : 'global',
      tags: (m.tags ?? []).join(', '),
    });
  };

  const closeDraft = useCallback(() => setDraft(null), []);

  const save = async () => {
    if (!draft || saving) return;
    const content = draft.content.trim();
    if (content === '') return setDraftError('内容不能为空');
    if (new TextEncoder().encode(content).length > MEMORY_CONTENT_MAX_BYTES) {
      return setDraftError('内容不能超过 8 KiB');
    }
    const tags = parseTags(draft.tags);
    setSaving(true);
    setDraftError(null);
    try {
      if (draft.id) await patchMemory(draft.id, { content, kind: draft.kind, tags });
      else await createMemory({ content, kind: draft.kind, scope: draft.scope, tags });
      toast('success', draft.id ? '记忆已更新' : '已添加记忆');
      setDraft(null);
      await load();
    } catch (err) {
      setDraftError(errorMessage(err));
    } finally {
      setSaving(false);
    }
  };

  const remove = async (id: string) => {
    try {
      await deleteMemory(id);
      setConfirmId(null);
      toast('success', '已删除记忆');
      await load();
    } catch (err) {
      toast('error', `删除失败:${errorMessage(err)}`);
    }
  };

  const loggedIn = status?.loggedIn === true;
  const syncEnabled = data?.sync?.enabled ?? status?.sync.memory ?? false;
  const lastSyncAt = data?.sync?.lastSyncAt ?? null;

  const toggleSync = async (next: boolean) => {
    if (syncBusy) return;
    setSyncBusy(true);
    try {
      await postAccountConfig({ sync: { memory: next } });
      setData((d) => (d ? { ...d, sync: { enabled: next, lastSyncAt: d.sync?.lastSyncAt ?? null } } : d));
      toast('success', next ? '已开启记忆同步' : '已关闭记忆同步');
      await Promise.all([load(), refreshStatus()]);
    } catch (err) {
      toast('error', `切换失败:${errorMessage(err)}`);
    } finally {
      setSyncBusy(false);
    }
  };

  const items = data?.items ?? [];

  return (
    <div data-testid="settings-page-memory" className="flex flex-col">
      <SetH1>记忆</SetH1>
      <p className="-mt-2 mb-4 text-[12px] leading-[18px] text-fg-3">
        Agent 会记住你的偏好、项目事实与约定,每轮自动取最相关的若干条注入上下文。也可以在这里手动整理。
      </p>
      <SetCard testId="memory-sync-card">
        <SetRow
          title="随账号同步"
          desc={
            loggedIn ? (
              <span data-testid="memory-last-sync">
                {syncEnabled ? `上次同步 ${lastSyncAt ? formatTime(lastSyncAt) : '尚未同步'}` : '已关闭,记忆只保存在本机'}
              </span>
            ) : (
              '登录 RurixForge 云后可在多台设备间同步记忆'
            )
          }
          last
          control={
            <span className={cn(!loggedIn && 'pointer-events-none opacity-50')}>
              <SetToggle
                on={loggedIn && syncEnabled}
                onChange={(v) => void toggleSync(v)}
                testId="memory-sync-toggle"
                ariaLabel="随账号同步记忆"
              />
            </span>
          }
        />
      </SetCard>

      <div className="mb-2 mt-5 flex items-center gap-2">
        <div className="flex h-[28px] min-w-0 flex-1 items-center gap-1.5 rounded-md border border-edge bg-shell-panel px-2">
          <Search size={12} className="shrink-0 text-fg-4" />
          <input
            data-testid="memory-search"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') setAppliedQuery(query.trim());
            }}
            placeholder="搜索记忆…"
            className="h-full min-w-0 flex-1 bg-transparent text-[12px] text-fg outline-none placeholder:text-fg-4"
          />
        </div>
        <div className="flex h-[28px] items-center rounded-md border border-edge bg-shell-sunk p-0.5">
          {SCOPE_FILTERS.map((s) => (
            <button
              key={s.id}
              type="button"
              data-testid={`memory-scope-${s.id}`}
              aria-pressed={scope === s.id}
              onClick={() => setScope(s.id)}
              className={cn(
                'h-full rounded px-2 text-[11.5px] transition-colors',
                scope === s.id ? 'bg-shell-panel text-fg shadow-sh1' : 'text-fg-3 hover:text-fg-2',
              )}
            >
              {s.label}
            </button>
          ))}
        </div>
        <SmBtn accent label={<><Plus size={11} />新建</>} testId="memory-create" onClick={openCreate} />
      </div>

      <SetCard testId="memory-list">
        {error ? (
          <div data-testid="memory-error" className="flex items-center gap-2 px-4 py-3 text-[11.5px] text-warn">
            <span className="min-w-0 flex-1">记忆加载失败:{error}</span>
            <SmBtn label="重试" onClick={() => void load()} />
          </div>
        ) : data === null && loading ? (
          <div className="px-4 py-3 text-[11.5px] text-fg-4">加载中…</div>
        ) : items.length === 0 ? (
          <div data-testid="memory-empty" className="px-4 py-6 text-center text-[11.5px] text-fg-4">
            {appliedQuery !== '' || scope !== 'all' ? '没有匹配的记忆' : '还没有记忆。Agent 在对话中记下的内容会出现在这里。'}
          </div>
        ) : (
          items.map((m, i) => (
            <div key={m.id} data-testid={`memory-row-${m.id}`} className={cn('flex gap-3 px-4 py-3', i > 0 && 'border-t border-edge')}>
              <div className="flex min-w-0 flex-1 flex-col gap-1.5">
                <div data-testid={`memory-content-${m.id}`} className="line-clamp-4 whitespace-pre-wrap break-words text-[12.5px] leading-[19px] text-fg">
                  {m.content}
                </div>
                <div className="flex flex-wrap items-center gap-1.5 text-[10.5px] text-fg-4">
                  <Badge testId={`memory-kind-${m.id}`}>{kindLabel(m.kind)}</Badge>
                  <Badge testId={`memory-scope-badge-${m.id}`} title={m.scope}>
                    {scopeLabel(m.scope, data?.projectKey)}
                  </Badge>
                  {(m.tags ?? []).map((t) => (
                    <span key={t} className="font-code">
                      #{t}
                    </span>
                  ))}
                  <span>{m.source === 'agent' ? 'Agent 记录' : '手动添加'}</span>
                  <span>· 更新于 {formatTime(m.updatedAt)}</span>
                </div>
              </div>
              <div className="flex shrink-0 items-start gap-1">
                {confirmId === m.id ? (
                  <>
                    <SmBtn label="取消" onClick={() => setConfirmId(null)} />
                    <SmBtn accent label="确认删除" testId={`memory-delete-confirm-${m.id}`} onClick={() => void remove(m.id)} />
                  </>
                ) : (
                  <>
                    <button
                      type="button"
                      aria-label="编辑"
                      data-testid={`memory-edit-${m.id}`}
                      onClick={() => openEdit(m)}
                      className="flex h-[26px] w-[26px] items-center justify-center rounded-md text-fg-3 transition-colors hover:bg-shell-hover hover:text-fg"
                    >
                      <Pencil size={12} />
                    </button>
                    <button
                      type="button"
                      aria-label="删除"
                      data-testid={`memory-delete-${m.id}`}
                      onClick={() => setConfirmId(m.id)}
                      className="flex h-[26px] w-[26px] items-center justify-center rounded-md text-fg-3 transition-colors hover:bg-shell-hover hover:text-danger"
                    >
                      <Trash2 size={12} />
                    </button>
                  </>
                )}
              </div>
            </div>
          ))
        )}
      </SetCard>

      {draft && (
        <MemoryDialog
          draft={draft}
          saving={saving}
          error={draftError}
          onChange={setDraft}
          onSave={() => void save()}
          onCancel={closeDraft}
        />
      )}
    </div>
  );
}
