import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import {
  ArrowUp,
  BookOpen,
  Box,
  Check,
  ChevronDown,
  ChevronUp,
  ListTodo,
  Plus,
  Sparkles,
  Square,
  X,
} from 'lucide-react';
import { apiGet } from '@/lib/forgeApi';
import { useChatStore } from '@/lib/chatStore';
import { HOME_INPUT_MIN, type ChatVariant } from '@/lib/chatVariant';
import { useComposerPrefillStore } from '@/lib/composerStore';
import { useContextUsage } from '@/lib/contextUsage';
import { useSessionStore } from '@/lib/sessionStore';
import { useSettingsStore } from '@/lib/settingsStore';
import { useSpeechInput } from '@/lib/speechInput';
import { useToastStore } from '@/lib/toastStore';
import { COMPOSER_INPUT_MIN, composerInputHeight } from '@/lib/inputHeight';
import { cn } from '@/lib/cn';
import { COMPOSER_MODES, composerModeMeta } from './composerModes';
import AgentSwitcher from './AgentSwitcher';
import GoalBar from './GoalBar';
import { useGoalStore } from '@/lib/goalStore';
import { useWorkbenchStore } from '@/lib/workbenchStore';
import { ContextMeterButton, ContextMeterPanel } from './ContextMeter';
import ModelPicker from './ModelPicker';
import { VoiceInputButton } from './VoiceInput';

const AGENT_KINDS: Array<{ id: string; label: string; icon: typeof Box; hint: string }> = [
  { id: 'coding', label: '编码', icon: Box, hint: '全量 MCP + 文件编辑' },
  { id: 'general', label: '通用', icon: Sparkles, hint: '只读 + 对话' },
  { id: 'document', label: '文档', icon: BookOpen, hint: '只读 + 写文件' },
];

function modesForKind(kind: string, engine: 'local' | 'codex' = 'local') {
  let modes = COMPOSER_MODES;
  if (kind === 'general' || kind === 'document') {
    modes = COMPOSER_MODES.filter((m) => m.id === 'ask' || m.id === 'build');
  }
  return engine === 'codex'
    ? modes.filter((m) => m.id !== 'team' && m.id !== 'multitask')
    : modes;
}

/**
 * F7 wave.4 Composer 全量(参考 ui/composer.rs render_composer);2026-08-24 用户拍板改为
 * 「单行胶囊 + 上下附属行」三层竖排:
 * 上方(胶囊外)= TodoStrip(todos 非空:list-todo 图标+「TODO」+{done}/{total}+120×4 进度条
 * bg_active/accent 填充+折叠 chevron;展开最多 4 行 running→queued→done 排序,完成行 sage
 * check+划线)/ 技能 chips;
 * 胶囊(p-1,内容 26px → 36px 高,单行 rounded-full 紧贴圆钮,换行转 rounded-2xl 并底对齐)=
 * 模式钮(2026-09-03 用户拍板与模式 chip 融合:build = + 26px 圆钮;非 build = 同一胶囊拉宽为
 * [+ 模式图标 标签 ×],accent_bg 底,主体开 add menu,× 复位 Agent——不再在胶囊上方另出一行 chip)
 * / textarea(44 列估行与实测 scrollHeight 取大,clamp 26–200,13.5px;
 * Enter 发送 / Shift+Enter 换行 / isComposing 防中文误发)/ 发送区(running=26px danger 圆方块
 * cancelRun;可发送=accent 圆 arrow-up;空文本=禁用态);
 * 下方(胶囊外)= 技能钮(每次开菜单重拉 GET skills/list,只列启用项,最多 16 条双行,选中
 * accent_bg+check;发送时经 ask:execute 结构化 skills[] 下发,F11 起不再拼文本前缀,见 E-06-002)
 * / 模型规格钮(Cursor 式 Thinking·Context·Effort·Model 四行菜单,
 * 见 ModelPicker.tsx)/ 上下文计量环(灰环 + 占有率扇形 + 百分比,点开在胶囊上方
 * 展开占用明细表,见 ContextMeter.tsx)/ 语音输入钮(Web Speech 听写,识别文本实时接在草稿后,
 * 见 VoiceInput.tsx 与 lib/speechInput.ts 的能力边界留痕)/ 有文本无会话时右侧 warn 胶囊
 * 「先选择会话」。
 *
 * add menu:AgentKind 三节(PATCH agentKind)+ 按 kind 过滤模式。Plan/Todo 已接线。
 * 本组件的两个下拉统一 bottom-full 上弹,锚 rootRef(包住三层的 relative 壳);
 * 模型规格菜单自带锚与 outside-click,靠 onOpen 与这两个互斥。
 *
 * home 变体(全屏对话主页):去掉列内的顶部虚线与内边距(由主页壳给居中列宽),
 * 输入壳底高抬到 HOME_INPUT_MIN 直接以多行圆角盒起步;并且允许「没有会话直接发」——
 * 先建会话再发,首屏输入即开工(参考 Codex),不再把人堵在「先选择会话」。
 */

interface SkillItem {
  name: string;
  description?: string;
  enabled?: boolean;
}

export default function Composer({ variant = 'column' }: { variant?: ChatVariant }) {
  const activeRunId = useChatStore((st) => st.activeRunId);
  const sendMessage = useChatStore((st) => st.sendMessage);
  const cancelRun = useChatStore((st) => st.cancelRun);
  const hasSession = useSessionStore((st) => st.activeSessionId !== null);
  const activeSessionId = useSessionStore((st) => st.activeSessionId);
  const activeSession = useSessionStore((st) =>
    st.sessions.find((session) => session.id === st.activeSessionId),
  );
  const createSession = useSessionStore((st) => st.create);
  const draftAgentEngine = useSessionStore((st) => st.draftAgentEngine);
  const setDraftAgentEngine = useSessionStore((st) => st.setDraftAgentEngine);
  const setAgentEngine = useSessionStore((st) => st.setAgentEngine);
  const home = variant === 'home';
  const agentKind = useSessionStore((st) => {
    const s = st.sessions.find((x) => x.id === st.activeSessionId);
    return s?.agentKind ?? 'coding';
  });
  const setAgentKind = useSessionStore((st) => st.setAgentKind);
  const agentEngine = activeSession?.agentEngine ?? draftAgentEngine;
  const visibleModes = modesForKind(agentKind, agentEngine);

  const [text, setText] = useState('');
  const [mode, setMode] = useState('build');
  const [addMenuOpen, setAddMenuOpen] = useState(false);
  const [skillMenuOpen, setSkillMenuOpen] = useState(false);
  const [contextOpen, setContextOpen] = useState(false);
  const [skills, setSkills] = useState<SkillItem[] | null>(null);
  const [skillsLoading, setSkillsLoading] = useState(false);
  const [selectedSkills, setSelectedSkills] = useState<string[]>([]);
  const [submittingTargets, setSubmittingTargets] = useState<Set<string>>(() => new Set());
  const contextUsage = useContextUsage(selectedSkills, text);
  const rootRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const submitCtrlEnter = useSettingsStore((st) => st.submitCtrlEnter);
  const prefillToken = useComposerPrefillStore((st) => st.token);

  useEffect(() => {
    if (!modesForKind(agentKind, agentEngine).some((item) => item.id === mode)) setMode('build');
  }, [agentEngine, agentKind, mode]);

  // 语音输入:开听时以当前草稿为底稿,识别文本(含未定稿片段)实时接在其后写回 textarea
  const textRef = useRef(text);
  textRef.current = text;
  const voice = useSpeechInput({
    onStart: () => {
      inputRef.current?.focus();
      return textRef.current;
    },
    onText: setText,
    onError: (msg) => useToastStore.getState().push('error', msg),
  });

  // F7 wave.5:外部预填 seam(Plan tab「开始 Build」→ build 模式 + 预填文案 + 聚焦)
  useEffect(() => {
    if (prefillToken === 0) return;
    const { draft, mode: m, clear } = useComposerPrefillStore.getState();
    if (draft !== null) setText(draft);
    if (m !== null && COMPOSER_MODES.some((x) => x.id === m)) setMode(m);
    clear();
    inputRef.current?.focus();
  }, [prefillToken]);

  const draftTarget = '@draft';
  const currentTarget = activeSessionId ?? draftTarget;
  const submitting = submittingTargets.has(currentTarget);
  const running = activeRunId !== null || submitting;
  const hasText = text.trim() !== '';
  const canSend = hasText && !running && (hasSession || home);

  // 输入壳实测高:临时压平 textarea 读 scrollHeight(含自身 py-[3px]),再恢复 h-full。
  const [measuredHeight, setMeasuredHeight] = useState(0);
  useLayoutEffect(() => {
    const ta = inputRef.current;
    if (!ta) return;
    ta.style.height = '0px';
    const next = ta.scrollHeight;
    ta.style.height = '';
    setMeasuredHeight(next);
    // home 变体换了字号与盒宽,实测值会变,变体切换要重量一次
  }, [text, home]);

  // 点击外部关闭下拉(参考 close_composer_dropdowns)
  useEffect(() => {
    if (!addMenuOpen && !skillMenuOpen) return;
    const onDown = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) {
        setAddMenuOpen(false);
        setSkillMenuOpen(false);
      }
    };
    document.addEventListener('mousedown', onDown);
    return () => document.removeEventListener('mousedown', onDown);
  }, [addMenuOpen, skillMenuOpen]);

  const closeMenus = () => {
    setAddMenuOpen(false);
    setSkillMenuOpen(false);
  };

  const openSkillMenu = async () => {
    const next = !skillMenuOpen;
    setAddMenuOpen(false);
    setSkillMenuOpen(next);
    // 每次开菜单都重拉:Skill 管理 tab/设置页改了启停或增删,这里必须跟着变。
    if (next && !skillsLoading) {
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

  const runGoalCommand = async (raw: string) => {
    const command = raw.trim();
    const goal = useGoalStore.getState();
    if (command === '') {
      useWorkbenchStore.getState().openTab('goal');
      return;
    }
    if (command.toLowerCase() === 'pause') {
      await goal.pauseGoal();
      return;
    }
    if (command.toLowerCase() === 'resume') {
      await goal.resumeGoal();
      return;
    }
    if (command.toLowerCase() === 'clear') {
      await goal.clearGoal();
      return;
    }
    await goal.setGoal(command);
  };

  const markSubmitting = (target: string, on: boolean) => {
    setSubmittingTargets((current) => {
      const next = new Set(current);
      if (on) next.add(target);
      else next.delete(target);
      return next;
    });
  };

  const moveSubmitting = (from: string, to: string) => {
    setSubmittingTargets((current) => {
      const next = new Set(current);
      next.delete(from);
      next.add(to);
      return next;
    });
  };

  const send = () => {
    if (!canSend) return;
    voice.stop();
    // F11 wave.5:选中技能作结构化字段下发(ask:execute skills[]),不再拼文本前缀——
    // 前缀服务端零解析,SKILL.md 全文从未进过模型上下文(E-06-002)。
    const picked = [...selectedSkills].sort();
    if (picked.length > 0) setSelectedSkills([]);
    setText('');
    closeMenus();
    const body = text.trim();
    const goalCommand = /^\/goal(?:\s+(.*))?$/i.exec(body);
    const args: [string, string, string[]?] =
      picked.length > 0 ? [body, mode, picked] : [body, mode];
    if (hasSession) {
      if (goalCommand) {
        void runGoalCommand(goalCommand[1] ?? '');
        return;
      }
      const target = activeSessionId;
      if (!target) return;
      markSubmitting(target, true);
      void Promise.resolve(sendMessage(...args)).finally(() => markSubmitting(target, false));
      return;
    }
    // 全屏主页无会话直发:建会话(带上 Composer 已勾的模型规格,否则 selectSession
    // 回放默认档会把 thinking 等冲掉)→ 主动订阅(ChatColumn 副作用见 currentSessionId
    // 已对齐会跳过,否则它的 reset() 会把下面这条乐观回显抹掉)→ 再发。
    markSubmitting(draftTarget, true);
    void (async () => {
      let target = draftTarget;
      const chat = useChatStore.getState();
      try {
        const s = await createSession(undefined, {
          agentEngine,
          selectedModelId: chat.selectedModelId,
          thinkingEnabled: chat.thinkingEnabled,
          reasoningEffort: chat.reasoningEffort,
          contextOptionId: chat.contextOptionId,
        });
        if (!s) return;
        moveSubmitting(draftTarget, s.id);
        target = s.id;
        await useChatStore.getState().selectSession(s.id);
        if (goalCommand) {
          await runGoalCommand(goalCommand[1] ?? '');
          return;
        }
        await useChatStore.getState().sendMessage(...args);
      } finally {
        markSubmitting(target, false);
      }
    })();
  };

  const modeMeta = composerModeMeta(mode);

  // 胶囊态:输入未换行 → 胶囊只含 [+][输入][发送] 一行,26px 内容 + p-1 = 36px,全圆角紧贴按钮。
  // 高度取「44 列估行」与实测 scrollHeight 的较大者:估行对中西文混排偏乏,盒子收窄后会漏字;
  // jsdom 下 scrollHeight 恒 0,退化回估行。
  const inputHeight = Math.min(
    Math.max(composerInputHeight(text), measuredHeight, home ? HOME_INPUT_MIN : 0),
    200,
  );
  const capsule = inputHeight <= COMPOSER_INPUT_MIN;
  // 非 build 模式:[+] 圆钮拉宽为模式胶囊(图标 + 标签 + × 复位),模式态不再另占一行 chip
  const modeActive = mode !== 'build';
  const hasChips = selectedSkills.length > 0;

  return (
    <div className={cn('shrink-0', !home && 'border-t border-dashed border-edge px-4 pb-2.5 pt-3.5')}>
      <div
        ref={rootRef}
        data-testid="composer"
        data-variant={variant}
        data-capsule={capsule ? '1' : undefined}
        className="relative flex flex-col gap-1.5"
      >
        <GoalBar />
        <TodoStrip />
        {/* 技能 chip 行(胶囊外,上方;模式态已融进胶囊内的 [+] 钮) */}
        {hasChips && (
          <div data-testid="composer-chips" className="flex flex-wrap items-center gap-1.5 px-1">
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
          </div>
        )}
        {/* 上下文占用明细表(计量环点开;落在胶囊正上方,与 TodoStrip 同为胶囊外附属行) */}
        {contextOpen && (
          <ContextMeterPanel usage={contextUsage} onClose={() => setContextOpen(false)} />
        )}
        {/* 胶囊:[+] 输入 [发送] 单行;p-1 让边框紧贴 26px 圆钮,换行后转圆角矩形并底对齐 */}
        <div
          data-testid="composer-capsule"
          className={cn(
            'flex items-end gap-1.5 border border-edge bg-shell-panel shadow-sh1 transition-[border-radius,border-color,box-shadow] duration-150 focus-within:border-acc-ring focus-within:shadow-[0_0_0_3px_var(--accent-bg)]',
            home ? 'p-2' : 'p-1',
            capsule ? 'rounded-full' : 'rounded-2xl',
          )}
        >
          {/* 模式钮:build = 26px 圆 [+];非 build = 同一胶囊拉宽为 [+ 图标 标签 ×]。
              外壳承担边框/圆角/裁切,内部两个 button 不嵌套(主体开菜单,× 复位)。 */}
          <div
            data-testid="composer-mode-pill"
            data-mode={mode}
            className={cn(
              'flex h-[26px] shrink-0 items-stretch overflow-hidden rounded-full border transition-colors duration-150',
              modeActive ? 'bg-acc-bg text-acc' : 'text-fg-2',
              addMenuOpen ? 'border-acc-ring' : modeActive ? 'border-transparent' : 'border-edge',
            )}
          >
            <button
              type="button"
              aria-label="模式菜单"
              aria-expanded={addMenuOpen}
              data-testid="composer-add"
              onClick={() => {
                setAddMenuOpen((v) => !v);
                setSkillMenuOpen(false);
              }}
              className={cn(
                'flex items-center justify-center hover:bg-shell-hover',
                modeActive ? 'gap-1 pl-1.5 pr-1' : 'w-6',
              )}
            >
              <Plus size={14} />
              {modeActive && (
                <span
                  data-testid="composer-mode-chip"
                  className="flex items-center gap-1 whitespace-nowrap text-[11px]"
                >
                  <modeMeta.icon size={11} />
                  {modeMeta.label}
                </span>
              )}
            </button>
            {modeActive && (
              <button
                type="button"
                aria-label="复位为 Agent"
                data-testid="composer-mode-reset"
                onClick={() => {
                  setMode('build');
                  setAddMenuOpen(false);
                }}
                className="flex items-center pl-0.5 pr-1.5 hover:bg-shell-hover"
              >
                <X size={10} />
              </button>
            )}
          </div>
          <div className="min-w-0 flex-1" style={{ height: inputHeight }}>
            <textarea
              ref={inputRef}
              value={text}
              onChange={(e) => {
                setText(e.target.value);
                // 听写途中手打:以新值为底稿续听,已注入的识别文本不再重复
                voice.rebase(e.target.value);
              }}
              onKeyDown={(e) => {
                if (e.key === 'Escape' && voice.listening) {
                  e.preventDefault();
                  voice.stop();
                  return;
                }
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
              placeholder={home ? '描述你要做的事，Enter 发送…' : '描述任务…'}
              className={cn(
                'h-full w-full resize-none bg-transparent py-[3px] leading-[20px] text-fg outline-none placeholder:text-fg-4',
                home ? 'text-[14px]' : 'text-[13.5px]',
              )}
            />
          </div>
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
        {/* 胶囊下方工具行:技能 / 模型 */}
        <div data-testid="composer-tools" className="flex items-center gap-1.5 px-1">
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
          <AgentSwitcher
            engine={agentEngine}
            disabled={running}
            onPick={async (next) => {
              closeMenus();
              if (activeSessionId) {
                await setAgentEngine(activeSessionId, next);
                const sessionState = useSessionStore.getState();
                if (sessionState.activeSessionId !== activeSessionId) return;
                const saved = sessionState.sessions.find((session) => session.id === activeSessionId);
                if (saved?.agentEngine !== next) return;
              } else {
                setDraftAgentEngine(next);
              }
              let fresh = useChatStore.getState();
              let candidates = fresh.models.filter((model) =>
                next === 'codex' ? model.provider === 'codex' : model.provider !== 'codex',
              );
              if (candidates.length === 0) {
                await fresh.ensureModels(true, next === 'codex' ? 'codex' : undefined);
                fresh = useChatStore.getState();
                candidates = fresh.models.filter((model) =>
                  next === 'codex' ? model.provider === 'codex' : model.provider !== 'codex',
                );
              }
              if (!candidates.some((model) => model.id === fresh.selectedModelId)) {
                if (next === 'codex') {
                  // selectedModelId=null 才是「使用 Codex 设置/app-server 默认」。
                  // 不能把 model/list 第一项偷偷持久化，否则会覆盖设置页的默认模型。
                  if (fresh.selectedModelId !== null) await fresh.pickModel(null);
                  return;
                }
                const fallback =
                  candidates.find((model) => model.availability === 'available') ?? candidates[0];
                if (fallback) await fresh.pickModel(fallback.id);
              }
            }}
          />
          <ModelPicker
            onOpen={closeMenus}
            provider={agentEngine === 'codex' ? 'codex' : undefined}
            excludeProvider={agentEngine === 'local' ? 'codex' : undefined}
            disabled={running}
          />
          <ContextMeterButton
            usage={contextUsage}
            open={contextOpen}
            onToggle={() => {
              setContextOpen((v) => !v);
              closeMenus();
            }}
          />
          <VoiceInputButton
            supported={voice.supported}
            listening={voice.listening}
            seconds={voice.seconds}
            onToggle={() => {
              voice.toggle();
              closeMenus();
            }}
          />
          <span className="flex-1" />
          {hasText && !hasSession && !home && (
            <span
              data-testid="composer-warn-no-session"
              className="flex h-[22px] items-center rounded-full bg-warn-bg px-2 text-[10.5px] text-warn"
            >
              先选择会话
            </span>
          )}
          {hasText && !hasSession && home && (
            <span
              data-testid="composer-hint-new-session"
              className="flex h-[22px] items-center rounded-full bg-acc-bg px-2 text-[10.5px] text-acc"
            >
              发送即新建会话
            </span>
          )}
        </div>

        {/* add menu:AgentKind 三节 + 模式 */}
        {addMenuOpen && (
          <div
            role="menu"
            data-testid="composer-add-menu"
            className="absolute bottom-full left-0 z-40 mb-1.5 flex min-w-[196px] flex-col rounded-[10px] border border-edge bg-shell-float p-1 shadow-float"
          >
            <div className="px-2 py-1 text-[9.5px] text-fg-4">代理类型</div>
            {AGENT_KINDS.map((k) => (
              <button
                key={k.id}
                type="button"
                role="menuitem"
                data-testid={`kind-item-${k.id}`}
                title={k.hint}
                onClick={() => {
                  if (activeSessionId) void setAgentKind(activeSessionId, k.id);
                  const nextModes = modesForKind(k.id, agentEngine);
                  if (!nextModes.some((m) => m.id === mode)) setMode('build');
                  closeMenus();
                }}
                className={cn(
                  'flex h-[26px] items-center gap-2 rounded-md px-2 text-left text-[12px] hover:bg-shell-selection',
                  k.id === agentKind ? 'text-acc' : 'text-fg-2',
                )}
              >
                <k.icon size={12} />
                <span className="min-w-0 flex-1 truncate">{k.label}</span>
                {k.id === agentKind && <Check size={11} />}
              </button>
            ))}
            <div className="my-1 h-px bg-edge" />
            <div className="px-2 py-1 text-[9.5px] text-fg-4">模式</div>
            {visibleModes.map((m) => (
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
            className="absolute bottom-full left-0 z-40 mb-1.5 flex max-h-[240px] min-w-[196px] max-w-[240px] flex-col overflow-hidden rounded-lg border border-edge bg-shell-float p-[3px] shadow-float"
          >
            <div className="px-1.5 py-0.5 text-[9.5px] text-fg-4">选择技能</div>
            <div className="flex max-h-[176px] min-h-0 flex-col overflow-y-auto">
              {skillsLoading && <div className="p-2 text-[10.5px] text-fg-4">加载中…</div>}
              {!skillsLoading && skills !== null && skills.length === 0 && (
                <div className="p-2 text-[10.5px] text-fg-4">未发现技能</div>
              )}
              {(skills ?? [])
                .filter((s) => s.enabled !== false)
                .slice(0, 16)
                .map((s) => {
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

      </div>
    </div>
  );
}

/** TodoStrip(参考 render_todo_strip;todos 为空不渲染)。 */
function TodoStrip() {
  const todos = useChatStore((st) => st.todos);
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
      className="flex flex-col rounded-xl border border-edge bg-shell-panel px-3 py-2"
    >
      {/* 精简列表渲染在头部行上方:展开时面板朝消息区方向向上增长,不遮挡输入框 */}
      {open && (
        <div className="flex flex-col pb-1">
          {display.slice(0, 4).map((t) => {
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
      )}
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
        <span className="flex-1" />
        {/* 就地向上展开/收起精简面板(原「打开看板 ↗」跳 workbench tab 已移除) */}
        <button
          type="button"
          aria-label={open ? '收起待办' : '展开待办'}
          data-testid="todo-strip-toggle"
          onClick={() => setOpen((v) => !v)}
          className="flex h-5 items-center gap-0.5 rounded px-1 text-[10.5px] text-fg-3 hover:bg-shell-hover hover:text-fg-2"
        >
          {open ? '收起' : '展开'}
          {open ? <ChevronDown size={11} /> : <ChevronUp size={11} />}
        </button>
      </div>
    </div>
  );
}
