import { useEffect, useRef, useState } from 'react';
import {
  ArrowUp,
  BookOpen,
  Check,
  ChevronDown,
  ChevronUp,
  Globe,
  ListTodo,
  Plus,
  Sparkles,
  Square,
  X,
} from 'lucide-react';
import { apiGet } from '@/lib/forgeApi';
import { useChatStore } from '@/lib/chatStore';
import { useComposerPrefillStore } from '@/lib/composerStore';
import { useSessionStore } from '@/lib/sessionStore';
import { useSettingsStore } from '@/lib/settingsStore';
import { useToastStore } from '@/lib/toastStore';
import { useWorkbenchStore } from '@/lib/workbenchStore';
import { composerInputHeight } from '@/lib/inputHeight';
import { cn } from '@/lib/cn';
import { COMPOSER_MODES, composerModeMeta } from './composerModes';

/**
 * F7 wave.4 Composer 全量(参考 ui/composer.rs render_composer):
 * TodoStrip(todos 非空:list-todo 图标+「TODO」+{done}/{total}+120×4 进度条 bg_active/
 * accent 填充+折叠 chevron;展开最多 4 行 running→queued→done 排序,完成行 sage check+划线);
 * 输入壳(textarea 自适应 44 列估行 32+(n-1)×20 clamp 68–200,13.5px;Enter 发送 /
 * Shift+Enter 换行 / isComposing 防中文误发);
 * 模式条:+ 26px 圆钮开 add menu(五模式)/非 build 模式 chip(accent_bg 胶囊 + x 复位)/
 * 技能 chips(选中可 x 移除)/技能钮(GET skills/list,菜单最多 16 条双行,选中 accent_bg+check;
 * 发送时文本前缀 `Use skills: a, b.\n\n`)/模型 chip(snapshot models,needs-key 禁用
 * 「未配置 Key」,选中 PATCH selectedModelId)/发送区(running=26px danger 圆方块 cancelRun;
 * 可发送=accent 圆 arrow-up;有文本无会话=灰禁用+warn 胶囊「先选择会话」;空文本=禁用态)。
 *
 * F8 wave.1 占位清零裁决(D-007 / D-F8-D):
 * ① AgentKind:参考 add menu 有代理类型三节——D-007 裁决本仓后端 agentKind 恒 coding,
 *   不新增 kind,UI 不落任何 kind 选择面(单态呈现:模式即全部可选语义),无「看起来能选
 *   其实没接」;②联网开关(globe):本仓无 web 搜索后端(sessionStore.webSearchEnabled
 *   恒 true 无消费面)→ 诚实禁用态(globe 钮 disabled + tooltip「联网搜索后端未接入」),
 *   不可开关;③执行计划/打开看板:F7 wave.5 已真实接线(PlanTab「开始 Build」→
 *   composerStore 预填 seam;TodoStrip「打开看板 ↗」→ workbench openTab('todo')),
 *   本波核验保留(composerW5.test.tsx 双断言)。参考「添加上下文/Image/Models/MCP」
 *   占位项:无后端语义,不落。
 */

interface SkillItem {
  name: string;
  description?: string;
  enabled?: boolean;
}

export default function Composer() {
  const activeRunId = useChatStore((st) => st.activeRunId);
  const todos = useChatStore((st) => st.todos);
  const models = useChatStore((st) => st.models);
  const defaultModelId = useChatStore((st) => st.defaultModelId);
  const selectedModelId = useChatStore((st) => st.selectedModelId);
  const sendMessage = useChatStore((st) => st.sendMessage);
  const cancelRun = useChatStore((st) => st.cancelRun);
  const pickModel = useChatStore((st) => st.pickModel);
  const hasSession = useSessionStore((st) => st.activeSessionId !== null);

  const [text, setText] = useState('');
  const [mode, setMode] = useState('build');
  const [addMenuOpen, setAddMenuOpen] = useState(false);
  const [skillMenuOpen, setSkillMenuOpen] = useState(false);
  const [modelMenuOpen, setModelMenuOpen] = useState(false);
  const [skills, setSkills] = useState<SkillItem[] | null>(null);
  const [skillsLoading, setSkillsLoading] = useState(false);
  const [selectedSkills, setSelectedSkills] = useState<string[]>([]);
  const rootRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const submitCtrlEnter = useSettingsStore((st) => st.submitCtrlEnter);
  const prefillToken = useComposerPrefillStore((st) => st.token);

  // F7 wave.5:外部预填 seam(Plan tab「开始 Build」→ build 模式 + 预填文案 + 聚焦)
  useEffect(() => {
    if (prefillToken === 0) return;
    const { draft, mode: m, clear } = useComposerPrefillStore.getState();
    if (draft !== null) setText(draft);
    if (m !== null && COMPOSER_MODES.some((x) => x.id === m)) setMode(m);
    clear();
    inputRef.current?.focus();
  }, [prefillToken]);

  const running = activeRunId !== null;
  const hasText = text.trim() !== '';
  const canSend = hasText && hasSession && !running;

  // 点击外部关闭下拉(参考 close_composer_dropdowns)
  useEffect(() => {
    if (!addMenuOpen && !skillMenuOpen && !modelMenuOpen) return;
    const onDown = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) {
        setAddMenuOpen(false);
        setSkillMenuOpen(false);
        setModelMenuOpen(false);
      }
    };
    document.addEventListener('mousedown', onDown);
    return () => document.removeEventListener('mousedown', onDown);
  }, [addMenuOpen, skillMenuOpen, modelMenuOpen]);

  const closeMenus = () => {
    setAddMenuOpen(false);
    setSkillMenuOpen(false);
    setModelMenuOpen(false);
  };

  const openSkillMenu = async () => {
    const next = !skillMenuOpen;
    setAddMenuOpen(false);
    setModelMenuOpen(false);
    setSkillMenuOpen(next);
    if (next && skills === null && !skillsLoading) {
      setSkillsLoading(true);
      try {
        const r = await apiGet<{ skills?: SkillItem[] }>('/api/forge/skills/list');
        setSkills(r.skills ?? []);
      } catch {
        setSkills([]);
        useToastStore.getState().push('error', '技能列表加载失败');
      } finally {
        setSkillsLoading(false);
      }
    }
  };

  const toggleSkill = (name: string) => {
    setSelectedSkills((prev) =>
      prev.includes(name) ? prev.filter((n) => n !== name) : [...prev, name],
    );
  };

  const send = () => {
    if (!canSend) return;
    let out = text;
    if (selectedSkills.length > 0) {
      const names = [...selectedSkills].sort();
      out = `Use skills: ${names.join(', ')}.\n\n${text.trim()}`;
      setSelectedSkills([]);
    }
    setText('');
    closeMenus();
    void sendMessage(out, mode);
  };

  const modeMeta = composerModeMeta(mode);
  const effectiveModelId = selectedModelId ?? defaultModelId;
  const effectiveModel = models.find((m) => m.id === effectiveModelId);
  const modelLabel = effectiveModel?.label || effectiveModelId || 'default';

  return (
    <div className="shrink-0 border-t border-dashed border-edge px-4 pb-2.5 pt-3.5">
      <div
        ref={rootRef}
        data-testid="composer"
        className="relative flex flex-col rounded-2xl border border-edge bg-shell-panel shadow-sh1"
      >
        <TodoStrip />
        {/* 输入壳 */}
        <div className="px-2.5 pb-0.5 pt-1" style={{ height: composerInputHeight(text), minHeight: 32 }}>
          <textarea
            ref={inputRef}
            value={text}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={(e) => {
              if (e.key !== 'Enter' || e.nativeEvent.isComposing) return;
              // F7 wave.5:Ctrl+Enter 发送设置消费(设置·Agent 页)——
              // 开启:Ctrl+Enter 发送 / Enter 换行;关闭(默认):Enter 发送 / Shift+Enter 换行。
              if (submitCtrlEnter) {
                if (e.ctrlKey || e.metaKey) {
                  e.preventDefault();
                  send();
                }
                return;
              }
              if (!e.shiftKey) {
                e.preventDefault();
                send();
              }
            }}
            data-testid="composer-input"
            placeholder="描述任务…"
            className="h-full w-full resize-none bg-transparent text-[13.5px] text-fg outline-none placeholder:text-fg-4"
          />
        </div>
        {/* 模式条 */}
        <div className="flex items-center gap-1.5 px-2 pb-1.5 pt-0.5">
          <button
            type="button"
            aria-label="模式菜单"
            data-testid="composer-add"
            onClick={() => {
              setAddMenuOpen((v) => !v);
              setSkillMenuOpen(false);
              setModelMenuOpen(false);
            }}
            className={cn(
              'flex h-[26px] w-[26px] shrink-0 items-center justify-center rounded-full border text-fg-2 hover:bg-shell-hover',
              addMenuOpen ? 'border-acc-ring' : 'border-edge',
            )}
          >
            <Plus size={14} />
          </button>
          {mode !== 'build' && (
            <span
              data-testid="composer-mode-chip"
              className="flex h-[22px] items-center gap-1 rounded-full bg-acc-bg px-2 text-[11px] text-acc"
            >
              <modeMeta.icon size={11} />
              {modeMeta.label}
              <button
                type="button"
                aria-label="复位为 Agent"
                data-testid="composer-mode-reset"
                onClick={() => setMode('build')}
                className="flex items-center"
              >
                <X size={10} />
              </button>
            </span>
          )}
          {[...selectedSkills].sort().map((name) => (
            <span
              key={name}
              data-testid={`skill-chip-${name}`}
              className="flex h-[22px] items-center gap-1 rounded-full bg-acc-bg px-2 text-[11px] text-acc"
            >
              <BookOpen size={10} />
              {name}
              <button
                type="button"
                aria-label={`移除技能 ${name}`}
                onClick={() => toggleSkill(name)}
                className="flex items-center"
              >
                <X size={10} />
              </button>
            </span>
          ))}
          <span className="flex-1" />
          {hasText && !hasSession && (
            <span
              data-testid="composer-warn-no-session"
              className="flex h-[22px] items-center rounded-full bg-warn-bg px-2 text-[10.5px] text-warn"
            >
              先选择会话
            </span>
          )}
          <button
            type="button"
            aria-label="选择技能"
            data-testid="composer-skills"
            onClick={() => void openSkillMenu()}
            className={cn(
              'flex h-[22px] items-center rounded-md border px-1.5 hover:bg-shell-hover',
              skillMenuOpen
                ? 'border-acc-ring'
                : selectedSkills.length > 0
                  ? 'border-acc-soft'
                  : 'border-transparent',
            )}
          >
            <BookOpen size={11} className={selectedSkills.length > 0 ? 'text-acc' : 'text-fg-3'} />
          </button>
          {/* F8 wave.1:联网搜索诚实禁用态(本仓无 web 搜索后端,不可开关) */}
          <button
            type="button"
            aria-label="联网搜索(未接入)"
            data-testid="composer-websearch"
            disabled
            title="联网搜索后端未接入"
            className="flex h-[22px] cursor-not-allowed items-center rounded-md border border-transparent px-1.5 opacity-40"
          >
            <Globe size={11} className="text-fg-3" />
          </button>
          <button
            type="button"
            aria-label="选择模型"
            data-testid="composer-model"
            onClick={() => {
              setModelMenuOpen((v) => !v);
              setAddMenuOpen(false);
              setSkillMenuOpen(false);
            }}
            className="flex h-[22px] items-center gap-1 rounded-md px-1.5 text-[11px] text-fg-2 hover:bg-shell-hover"
          >
            <Sparkles size={11} />
            <span className="max-w-[140px] truncate">{modelLabel}</span>
            <ChevronDown size={10} className="text-fg-3" />
          </button>
          {running ? (
            <button
              type="button"
              aria-label="中止运行"
              data-testid="composer-abort"
              onClick={() => void cancelRun()}
              className="flex h-[26px] w-[26px] shrink-0 items-center justify-center rounded-full bg-danger text-fg-inv"
            >
              <Square size={11} />
            </button>
          ) : (
            <button
              type="button"
              aria-label="发送"
              data-testid="composer-send"
              disabled={!canSend}
              onClick={send}
              className={cn(
                'flex h-[26px] w-[26px] shrink-0 items-center justify-center rounded-full',
                canSend
                  ? 'bg-acc text-fg-inv hover:bg-acc-soft'
                  : 'border border-edge bg-shell-active text-fg-4',
              )}
            >
              <ArrowUp size={13} />
            </button>
          )}
        </div>

        {/* add menu:五模式 */}
        {addMenuOpen && (
          <div
            role="menu"
            data-testid="composer-add-menu"
            className="absolute bottom-10 left-2 z-40 flex min-w-[196px] flex-col rounded-[10px] border border-edge bg-shell-float p-1 shadow-float"
          >
            {COMPOSER_MODES.map((m) => (
              <button
                key={m.id}
                type="button"
                role="menuitem"
                data-testid={`mode-item-${m.id}`}
                onClick={() => {
                  setMode(m.id);
                  closeMenus();
                }}
                className={cn(
                  'flex h-[26px] items-center gap-2 rounded-md px-2 text-left text-[12px] hover:bg-shell-selection',
                  m.id === mode ? 'text-acc' : 'text-fg-2',
                )}
              >
                <m.icon size={12} />
                <span className="min-w-0 flex-1 truncate">{m.label}</span>
                {m.id === mode && <Check size={11} />}
              </button>
            ))}
          </div>
        )}

        {/* 技能菜单 */}
        {skillMenuOpen && (
          <div
            role="menu"
            data-testid="composer-skill-menu"
            className="absolute bottom-10 left-10 z-40 flex max-h-[240px] min-w-[196px] max-w-[240px] flex-col overflow-hidden rounded-lg border border-edge bg-shell-float p-[3px] shadow-float"
          >
            <div className="px-1.5 py-0.5 text-[9.5px] text-fg-4">选择技能</div>
            <div className="flex max-h-[176px] min-h-0 flex-col overflow-y-auto">
              {skillsLoading && <div className="p-2 text-[10.5px] text-fg-4">加载中…</div>}
              {!skillsLoading && skills !== null && skills.length === 0 && (
                <div className="p-2 text-[10.5px] text-fg-4">未发现技能</div>
              )}
              {(skills ?? []).slice(0, 16).map((s) => {
                const active = selectedSkills.includes(s.name);
                return (
                  <button
                    key={s.name}
                    type="button"
                    role="menuitem"
                    data-testid={`skill-item-${s.name}`}
                    onClick={() => toggleSkill(s.name)}
                    className={cn(
                      'flex items-center gap-1.5 rounded-[5px] px-1.5 py-[3px] text-left hover:bg-shell-selection',
                      active && 'bg-acc-bg',
                    )}
                  >
                    <BookOpen size={9} className="shrink-0 text-fg-3" />
                    <span className="flex min-w-0 flex-1 flex-col gap-px">
                      <span className="truncate text-[11px] text-fg-2">{s.name}</span>
                      <span className="truncate text-[9.5px] text-fg-4">
                        {s.description?.trim() ? s.description : '（无描述）'}
                      </span>
                    </span>
                    {active && <Check size={9} className="shrink-0 text-acc" />}
                  </button>
                );
              })}
            </div>
          </div>
        )}

        {/* 模型菜单 */}
        {modelMenuOpen && (
          <div
            role="menu"
            data-testid="composer-model-menu"
            className="absolute bottom-10 right-2 z-40 flex max-h-[320px] min-w-[214px] flex-col overflow-hidden rounded-[10px] border border-edge bg-shell-float p-1 shadow-float"
          >
            {models.length === 0 && <div className="p-2.5 text-[11.5px] text-fg-4">暂无可用模型</div>}
            {models.slice(0, 12).map((m) => {
              const disabled = m.availability === 'needs-key';
              const active = m.id === effectiveModelId;
              return (
                <button
                  key={m.id}
                  type="button"
                  role="menuitem"
                  data-testid={`model-item-${m.id}`}
                  disabled={disabled}
                  title={disabled ? '未配置 Key' : undefined}
                  onClick={() => {
                    closeMenus();
                    void pickModel(m.id);
                  }}
                  className={cn(
                    'flex items-center gap-2 rounded-md px-2 py-[5px] text-left',
                    disabled ? 'cursor-not-allowed opacity-50' : 'hover:bg-shell-selection',
                    active && 'bg-acc-bg',
                  )}
                >
                  <Sparkles size={11} className="shrink-0 text-fg-3" />
                  <span className="flex min-w-0 flex-1 flex-col">
                    <span className="truncate text-[12px] text-fg-2">{m.label || m.id}</span>
                    {m.provider && (
                      <span className="truncate text-[10.5px] text-fg-4">{m.provider}</span>
                    )}
                  </span>
                  {active && <Check size={11} className="shrink-0 text-acc" />}
                </button>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}

/** TodoStrip(参考 render_todo_strip;todos 为空不渲染)。 */
function TodoStrip() {
  const todos = useChatStore((st) => st.todos);
  const openTab = useWorkbenchStore((st) => st.openTab);
  const [open, setOpen] = useState(false);
  if (todos.length === 0) return null;
  const isDone = (s: string) => s === 'completed' || s === 'done';
  const isRunning = (s: string) => s === 'running' || s === 'in_progress';
  const done = todos.filter((t) => isDone(t.status)).length;
  const total = todos.length;
  const progress = total === 0 ? 0 : done / total;
  const display = [...todos].sort((a, b) => {
    const rank = (s: string) => (isRunning(s) ? 0 : isDone(s) ? 2 : 1);
    return rank(a.status) - rank(b.status);
  });

  return (
    <div
      data-testid="todo-strip"
      className={cn('flex flex-col border-b border-edge px-3 pt-2', open ? 'pb-1' : 'pb-2')}
    >
      <div className="flex items-center gap-2 text-[11px] text-fg-3">
        <ListTodo size={12} />
        <span className="font-semibold">TODO</span>
        <span className="font-code text-[10.5px]">
          {done}/{total}
        </span>
        <span className="h-1 w-[120px] rounded-full bg-shell-active">
          <span
            data-testid="todo-progress"
            className="block h-full rounded-full bg-acc"
            style={{ width: `${Math.round(progress * 100)}%` }}
          />
        </span>
        {/* F7 wave.5:打开看板 ↗(开 todo tab,真实接线) */}
        <button
          type="button"
          aria-label="打开看板"
          data-testid="todo-open-board"
          onClick={() => openTab('todo')}
          className="flex h-5 items-center gap-0.5 rounded px-1 text-[10.5px] text-fg-3 hover:bg-shell-hover hover:text-fg-2"
        >
          打开看板 ↗
        </button>
        <span className="flex-1" />
        <button
          type="button"
          aria-label="折叠待办"
          data-testid="todo-strip-toggle"
          onClick={() => setOpen((v) => !v)}
          className="flex h-5 w-5 items-center justify-center rounded text-fg-3 hover:bg-shell-hover"
        >
          {open ? <ChevronDown size={11} /> : <ChevronUp size={11} />}
        </button>
      </div>
      {open &&
        display.slice(0, 4).map((t) => {
          const doneRow = isDone(t.status);
          return (
            <div key={t.id} className="flex items-center gap-2 py-[3px] text-[11.5px]" data-testid={`todo-row-${t.id}`}>
              {doneRow ? (
                <Check size={11} className="shrink-0 text-sage" />
              ) : isRunning(t.status) ? (
                <span className="h-[6px] w-[6px] shrink-0 animate-pulse rounded-full bg-dot-running" />
              ) : (
                <span className="h-2.5 w-2.5 shrink-0 rounded-[3px] border border-edge-strong" />
              )}
              <span
                className={cn(
                  'min-w-0 flex-1 truncate',
                  doneRow ? 'text-fg-4 line-through' : 'text-fg-2',
                )}
              >
                {t.title === '' ? t.id : t.title}
              </span>
            </div>
          );
        })}
    </div>
  );
}
