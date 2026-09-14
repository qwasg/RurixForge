import { useEffect, useState } from 'react';
import {
  BookOpen,
  Check,
  CircleAlert,
  FolderPlus,
  Plus,
  Search,
  Trash2,
  TriangleAlert,
  X,
} from 'lucide-react';
import { cn } from '@/lib/cn';
import {
  SKILL_NAME_RE,
  skillErrorLabel,
  useSkillStore,
  type SkillItem,
} from '@/lib/skillStore';
import { useWorkbenchStore } from '@/lib/workbenchStore';
import CodeEditor from './CodeEditor';

/**
 * F11 wave.5 Skill 管理 tab(07 §7.2;入口 = Sidebar「Skill 管理」按钮 → workbench tab)。
 * 左 260px 技能列表(搜索 / 新建内联输入行 / 启停 / 来源徽标),右详情:
 * 工具条(名 + 版本·许可证·标签 chips + 校验/保存/删除 + dirty 圆点)→ 校验结果 →
 * 删除确认条 → SKILL.md 编辑器(复用 F9 的 CodeEditor;path 以 .md 结尾即走 markdown 语言包)。
 * 未选中技能时右侧显示技能目录(extraDirs)配置卡。
 *
 * 非模态纪律:新建输入与删除确认一律内联条,不用 <dialog>/confirm——
 * 模态会永久阻塞无人值守冒烟(见 AssetsPanel 的 F1 坑留痕)。
 * 删除走两阶段:DELETE → 409 GOV_PROPOSAL_REQUIRED + proposalId → 批准 → 重发 DELETE。
 */

function Chip({ text, testId }: { text: string; testId?: string }) {
  return (
    <span
      data-testid={testId}
      className="flex h-[16px] shrink-0 items-center rounded-full border border-edge bg-shell-sunk px-1.5 font-code text-[10px] text-fg-3"
    >
      {text}
    </span>
  );
}

/** 启停开关(32×18 胶囊,与设置控件同款;行内不可嵌 button,故行本身不是 button)。 */
function Toggle({
  on,
  onChange,
  label,
  testId,
}: {
  on: boolean;
  onChange: (next: boolean) => void;
  label: string;
  testId: string;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={on}
      aria-label={label}
      data-testid={testId}
      onClick={(e) => {
        e.stopPropagation();
        onChange(!on);
      }}
      className={cn(
        'flex h-[16px] w-[28px] shrink-0 items-center rounded-full px-0.5 transition-colors',
        on ? 'justify-end bg-sage' : 'justify-start bg-shell-active',
      )}
    >
      <span className="h-3 w-3 rounded-full bg-white shadow-[0_1px_3px_rgba(0,0,0,0.18)]" />
    </button>
  );
}

function SkillRow({ s, active }: { s: SkillItem; active: boolean }) {
  const select = useSkillStore((st) => st.select);
  const toggleEnabled = useSkillStore((st) => st.toggleEnabled);
  return (
    <div
      data-testid={`skill-row-${s.name}`}
      className={cn(
        'flex items-center gap-1.5 rounded-md px-2 py-1.5 transition-colors',
        active ? 'bg-shell-active' : 'hover:bg-shell-hover',
      )}
    >
      <button
        type="button"
        aria-label={`选择技能 ${s.name}`}
        data-testid={`skill-select-${s.name}`}
        onClick={() => void select(s.name)}
        className="flex min-w-0 flex-1 flex-col items-start gap-px text-left"
      >
        <span className="flex w-full min-w-0 items-center gap-1">
          <span className={cn('min-w-0 truncate text-[12.4px]', s.enabled ? 'text-fg' : 'text-fg-3')}>
            {s.name}
          </span>
          {typeof s.version === 'string' && s.version !== '' && (
            <Chip text={`v${s.version}`} testId={`skill-version-${s.name}`} />
          )}
        </span>
        <span className="w-full truncate text-[10.5px] text-fg-4">
          {s.description.trim() === '' ? '（无描述）' : s.description}
        </span>
      </button>
      <span
        data-testid={`skill-origin-${s.name}`}
        title={s.dir === '' ? undefined : s.dir}
        className={cn(
          'flex h-[16px] shrink-0 items-center rounded-full px-1.5 text-[10px]',
          s.builtin ? 'bg-shell-sunk text-fg-3' : 'bg-acc-bg text-acc',
        )}
      >
        {s.builtin ? '内置' : '外部'}
      </span>
      <Toggle
        on={s.enabled}
        label={`${s.enabled ? '禁用' : '启用'}技能 ${s.name}`}
        testId={`skill-toggle-${s.name}`}
        onChange={(v) => void toggleEnabled(s.name, v)}
      />
    </div>
  );
}

/** 技能目录(extraDirs)配置;设置页与本 tab 共用同一 skillStore 事实源。 */
export function SkillDirsEditor() {
  const extraDirs = useSkillStore((st) => st.extraDirs);
  const setExtraDirs = useSkillStore((st) => st.setExtraDirs);
  const [draft, setDraft] = useState('');

  const add = () => {
    const v = draft.trim();
    if (v === '' || extraDirs.includes(v)) return;
    setDraft('');
    void setExtraDirs([...extraDirs, v]);
  };

  return (
    <div data-testid="skill-dirs" className="flex flex-col gap-1.5">
      {extraDirs.length === 0 && (
        <span className="text-[11px] text-fg-4" data-testid="skill-dirs-empty">
          未配置附加目录，仅扫描仓库 skills/。
        </span>
      )}
      {extraDirs.map((d) => (
        <div
          key={d}
          data-testid={`skill-dir-${d}`}
          className="flex items-center gap-2 rounded-md border border-edge bg-shell-panel px-2 py-1"
        >
          <span className="min-w-0 flex-1 truncate font-code text-[11px] text-fg-2">{d}</span>
          <button
            type="button"
            aria-label={`移除技能目录 ${d}`}
            data-testid={`skill-dir-remove-${d}`}
            onClick={() => void setExtraDirs(extraDirs.filter((x) => x !== d))}
            className="flex h-[18px] w-[18px] shrink-0 items-center justify-center rounded text-fg-3 hover:bg-shell-hover hover:text-fg"
          >
            <X size={11} />
          </button>
        </div>
      ))}
      <div className="flex items-center gap-1.5">
        <input
          value={draft}
          aria-label="新增技能目录"
          data-testid="skill-dir-input"
          placeholder="如 vendor/skills"
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') add();
          }}
          className="min-w-0 flex-1 rounded-md border border-edge bg-shell-panel px-2 py-[5px] font-code text-[11px] text-fg outline-none placeholder:text-fg-4 focus:border-edge-strong"
        />
        <button
          type="button"
          data-testid="skill-dir-add"
          onClick={add}
          className="flex h-[24px] shrink-0 items-center gap-1 rounded-md border border-edge bg-shell-panel px-2 text-[11px] text-fg-2 hover:bg-shell-hover"
        >
          <FolderPlus size={11} />
          添加
        </button>
      </div>
      <span className="text-[10.5px] text-fg-4">
        目录相对仓库根解析；扫描顺序为 skills/ 优先，同名技能取先扫到的一份。
      </span>
    </div>
  );
}

export default function SkillsTab() {
  const items = useSkillStore((st) => st.items);
  const loading = useSkillStore((st) => st.loading);
  const error = useSkillStore((st) => st.error);
  const selected = useSkillStore((st) => st.selected);
  const detail = useSkillStore((st) => st.detail);
  const detailLoading = useSkillStore((st) => st.detailLoading);
  const draft = useSkillStore((st) => st.draft);
  const dirty = useSkillStore((st) => st.dirty);
  const validation = useSkillStore((st) => st.validation);
  const pendingDelete = useSkillStore((st) => st.pendingDelete);
  const load = useSkillStore((st) => st.load);
  const setDraft = useSkillStore((st) => st.setDraft);
  const save = useSkillStore((st) => st.save);
  const validate = useSkillStore((st) => st.validate);
  const create = useSkillStore((st) => st.create);
  const requestDelete = useSkillStore((st) => st.requestDelete);
  const cancelDelete = useSkillStore((st) => st.cancelDelete);
  const confirmDelete = useSkillStore((st) => st.confirmDelete);
  const approveAndDelete = useSkillStore((st) => st.approveAndDelete);

  const [query, setQuery] = useState('');
  const [creating, setCreating] = useState(false);
  const [newName, setNewName] = useState('');
  const [createError, setCreateError] = useState<string | null>(null);

  useEffect(() => {
    void load();
  }, [load]);

  // Ctrl+S:编辑器外聚焦时也能保存(编辑器内由 CM keymap 消费)。
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && !e.shiftKey && !e.altKey && e.key.toLowerCase() === 's') {
        e.preventDefault();
        void save();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [save]);

  const q = query.trim().toLowerCase();
  const visible =
    q === ''
      ? items
      : items.filter(
          (s) => s.name.toLowerCase().includes(q) || s.description.toLowerCase().includes(q),
        );

  const nameOk = SKILL_NAME_RE.test(newName.trim());
  const submitCreate = () => {
    const name = newName.trim();
    if (!SKILL_NAME_RE.test(name)) {
      setCreateError('技能名只能用小写英文、数字与中划线');
      return;
    }
    setCreateError(null);
    void create(name)
      .then(() => {
        setCreating(false);
        setNewName('');
      })
      .catch((err: unknown) => setCreateError(skillErrorLabel(err, '新建')));
  };

  const front = detail?.front;
  const deleteBar = pendingDelete !== null && pendingDelete.name === selected ? pendingDelete : null;

  return (
    <div data-testid="skills-tab" className="flex h-full min-h-0 flex-col bg-shell-bg">
      {error !== null && (
        <div
          data-testid="skills-error"
          className="flex shrink-0 items-center gap-1.5 border-b border-edge bg-warn-bg px-3 py-1 text-[11px] text-warn"
        >
          <CircleAlert size={11} className="shrink-0" />
          <span className="min-w-0 flex-1">{error}</span>
          <button
            type="button"
            data-testid="skills-retry"
            onClick={() => void load()}
            className="shrink-0 rounded border border-edge-strong px-1.5 py-px hover:bg-shell-hover"
          >
            重试
          </button>
        </div>
      )}
      <div className="flex min-h-0 flex-1">
        {/* 左:技能列表 */}
        <div className="flex w-[260px] shrink-0 flex-col border-r border-edge bg-shell-panel">
          <div className="flex shrink-0 items-center gap-1.5 border-b border-edge px-2.5 py-2">
            <span className="flex min-w-0 flex-1 items-center gap-1 rounded-md border border-edge bg-shell-sunk px-1.5">
              <Search size={11} className="shrink-0 text-fg-4" />
              <input
                value={query}
                aria-label="搜索技能"
                data-testid="skill-search"
                placeholder="搜索技能…"
                onChange={(e) => setQuery(e.target.value)}
                className="min-w-0 flex-1 bg-transparent py-1 text-[11.5px] text-fg outline-none placeholder:text-fg-4"
              />
            </span>
            <button
              type="button"
              aria-label="新建技能"
              data-testid="skill-new"
              onClick={() => {
                setCreating((v) => !v);
                setNewName('');
                setCreateError(null);
              }}
              className="flex h-[22px] shrink-0 items-center gap-0.5 rounded-md border border-edge bg-shell-panel px-1.5 text-[11px] text-fg-2 hover:bg-shell-hover"
            >
              <Plus size={11} />
              新建
            </button>
          </div>

          {creating && (
            <div className="flex shrink-0 flex-col gap-1 border-b border-edge px-2.5 py-1.5">
              <div
                className={cn(
                  'flex h-7 items-center gap-1.5 rounded-md border bg-shell-panel px-2',
                  newName !== '' && !nameOk ? 'border-danger' : 'border-acc-ring',
                )}
              >
                <BookOpen size={11} className="shrink-0 text-fg-3" />
                <input
                  autoFocus
                  value={newName}
                  aria-label="新技能名"
                  data-testid="skill-new-name"
                  placeholder="技能名（小写英文/数字/中划线）"
                  onChange={(e) => {
                    setNewName(e.target.value);
                    setCreateError(null);
                  }}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter') submitCreate();
                    if (e.key === 'Escape') {
                      setCreating(false);
                      setNewName('');
                      setCreateError(null);
                    }
                  }}
                  className="min-w-0 flex-1 bg-transparent text-[12px] text-fg outline-none placeholder:text-fg-4"
                />
                <button
                  type="button"
                  data-testid="skill-new-submit"
                  onClick={submitCreate}
                  className="shrink-0 text-[11px] text-acc"
                >
                  创建
                </button>
              </div>
              {newName !== '' && !nameOk && (
                <span className="text-[10.5px] text-danger" data-testid="skill-new-name-hint">
                  技能名只能用小写英文、数字与中划线
                </span>
              )}
              {createError !== null && (
                <span className="text-[10.5px] text-danger" data-testid="skill-new-error">
                  {createError}
                </span>
              )}
            </div>
          )}

          <div className="flex min-h-0 flex-1 flex-col gap-px overflow-y-auto p-1.5">
            {loading && items.length === 0 && (
              <span className="px-1.5 py-1 text-[11px] text-fg-4">加载中…</span>
            )}
            {!loading && error === null && items.length === 0 && (
              <span className="px-1.5 py-1 text-[11px] text-fg-4" data-testid="skills-empty">
                未发现技能
              </span>
            )}
            {items.length > 0 && visible.length === 0 && (
              <span className="px-1.5 py-1 text-[11px] text-fg-4" data-testid="skills-no-match">
                无匹配技能
              </span>
            )}
            {visible.map((s) => (
              <SkillRow key={s.name} s={s} active={s.name === selected} />
            ))}
          </div>
        </div>

        {/* 右:详情 / 目录配置 */}
        <div className="flex min-h-0 min-w-0 flex-1 flex-col">
          {selected === null ? (
            <div className="flex min-h-0 flex-1 flex-col gap-2.5 overflow-y-auto p-5">
              <span className="font-serif text-[20px] font-bold text-fg">技能目录</span>
              <span className="text-[11px] text-fg-4">
                左侧选中技能可查看与编辑 SKILL.md；此处配置额外的扫描目录（06 §2 extraDirs）。
              </span>
              <div className="rounded-[10px] border border-edge bg-shell-sunk p-3">
                <SkillDirsEditor />
              </div>
            </div>
          ) : (
            <>
              <div className="flex h-[26px] shrink-0 items-center gap-1.5 border-b border-edge px-2.5">
                <BookOpen size={11} className="shrink-0 text-fg-3" />
                <span className="shrink-0 text-[12.4px] text-fg" data-testid="skill-detail-name">
                  {selected}
                </span>
                {dirty && (
                  <span
                    data-testid="skill-dirty"
                    title="有未保存的改动"
                    className="h-[6px] w-[6px] shrink-0 rounded-full bg-acc"
                  />
                )}
                {typeof front?.version === 'string' && front.version !== '' && (
                  <Chip text={`v${front.version}`} testId="skill-detail-version" />
                )}
                {typeof front?.license === 'string' && front.license !== '' && (
                  <Chip text={front.license} testId="skill-detail-license" />
                )}
                {(front?.tags ?? []).slice(0, 4).map((t) => (
                  <Chip key={t} text={t} testId={`skill-tag-${t}`} />
                ))}
                {detail !== null && (
                  <Chip text={detail.builtin ? '内置' : '外部'} testId="skill-detail-origin" />
                )}
                <span className="flex-1" />
                <button
                  type="button"
                  aria-label="校验 SKILL.md"
                  data-testid="skill-validate"
                  onClick={() => void validate()}
                  className="flex h-[18px] shrink-0 items-center rounded border border-edge bg-shell-panel px-1.5 text-[10.5px] text-fg-2 hover:bg-shell-hover"
                >
                  校验
                </button>
                <button
                  type="button"
                  aria-label="保存 SKILL.md"
                  data-testid="skill-save"
                  onClick={() => void save()}
                  className={cn(
                    'flex h-[18px] shrink-0 items-center rounded border px-1.5 text-[10.5px]',
                    dirty
                      ? 'border-acc bg-acc text-fg-inv hover:bg-acc-soft'
                      : 'border-edge bg-shell-panel text-fg-3 hover:bg-shell-hover',
                  )}
                  title="保存(Ctrl+S)"
                >
                  保存
                </button>
                <button
                  type="button"
                  aria-label="删除技能"
                  data-testid="skill-delete"
                  onClick={() => requestDelete(selected)}
                  className="flex h-[18px] shrink-0 items-center gap-0.5 rounded border border-edge bg-shell-panel px-1.5 text-[10.5px] text-fg-2 hover:bg-shell-hover hover:text-danger"
                >
                  <Trash2 size={10} />
                  删除
                </button>
              </div>

              {deleteBar !== null && (
                <DeleteBar
                  name={deleteBar.name}
                  proposalId={deleteBar.proposalId}
                  onContinue={() => void confirmDelete()}
                  onApprove={() => void approveAndDelete()}
                  onCancel={cancelDelete}
                />
              )}

              {validation !== null && (
                <div
                  data-testid="skill-validation"
                  className="flex shrink-0 flex-col gap-px border-b border-edge bg-shell-sunk px-2.5 py-1"
                >
                  {validation.errors.map((e, i) => (
                    <span
                      key={`e-${i}`}
                      data-testid="skill-validation-error"
                      className="flex items-center gap-1 text-[11px] text-danger"
                    >
                      <CircleAlert size={10} className="shrink-0" />
                      {e}
                    </span>
                  ))}
                  {validation.warnings.map((w, i) => (
                    <span
                      key={`w-${i}`}
                      data-testid="skill-validation-warning"
                      className="flex items-center gap-1 text-[11px] text-warn"
                    >
                      <TriangleAlert size={10} className="shrink-0" />
                      {w}
                    </span>
                  ))}
                  {validation.errors.length === 0 && validation.warnings.length === 0 && (
                    <span
                      data-testid="skill-validation-ok"
                      className="flex items-center gap-1 text-[11px] text-sage"
                    >
                      <Check size={10} className="shrink-0" />
                      校验通过
                    </span>
                  )}
                </div>
              )}

              <div className="min-h-0 flex-1">
                {detailLoading && (
                  <div className="px-3 py-2 text-[11px] text-fg-4" data-testid="skill-detail-loading">
                    加载中…
                  </div>
                )}
                {!detailLoading && detail === null && (
                  <div className="px-3 py-2 text-[11px] text-fg-4" data-testid="skill-detail-missing">
                    详情不可用，原因见上方错误条。
                  </div>
                )}
                {!detailLoading && detail !== null && (
                  <CodeEditor
                    key={selected}
                    path={detail.path.toLowerCase().endsWith('.md') ? detail.path : `${selected}/SKILL.md`}
                    initialDoc={draft}
                    onDocChanged={setDraft}
                    onSave={() => void save()}
                    data-testid="skill-editor"
                    className="h-full min-h-0 font-code text-[12.5px]"
                  />
                )}
              </div>
            </>
          )}
        </div>
      </div>
    </div>
  );
}

/** 删除确认条(内联,两阶段:确认 → 提案待批 → 批准重发)。 */
function DeleteBar({
  name,
  proposalId,
  onContinue,
  onApprove,
  onCancel,
}: {
  name: string;
  proposalId: string | null;
  onContinue: () => void;
  onApprove: () => void;
  onCancel: () => void;
}) {
  const openTab = useWorkbenchStore((st) => st.openTab);
  const btn =
    'shrink-0 rounded border border-edge-strong bg-shell-panel px-1.5 py-px hover:bg-shell-hover';
  return (
    <div
      data-testid="skill-delete-bar"
      className="flex shrink-0 items-center gap-2 border-b border-edge bg-warn-bg px-2.5 py-1 text-[11px] text-warn"
    >
      {proposalId === null ? (
        <>
          <span className="min-w-0 flex-1">删除技能 {name}？此操作需要提案确认。</span>
          <button type="button" data-testid="skill-delete-continue" onClick={onContinue} className={btn}>
            继续
          </button>
        </>
      ) : (
        <>
          <span className="min-w-0 flex-1" data-testid="skill-delete-proposal">
            已创建提案 {proposalId}，需批准后才能删除。
          </span>
          <button
            type="button"
            data-testid="skill-delete-goto-proposals"
            onClick={() => openTab('proposals')}
            className={btn}
          >
            去提案页批准
          </button>
          <button type="button" data-testid="skill-delete-approve" onClick={onApprove} className={btn}>
            在此批准
          </button>
        </>
      )}
      <button type="button" data-testid="skill-delete-cancel" onClick={onCancel} className={btn}>
        取消
      </button>
    </div>
  );
}
