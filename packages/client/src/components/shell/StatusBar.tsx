import type { ReactNode } from 'react';
import { Bot, Boxes, Cpu, GitBranch, ListChecks } from 'lucide-react';
import { cn } from '@/lib/cn';
import { useChatStore } from '@/lib/chatStore';
import { useGitPolling, useGitStore } from '@/lib/gitStore';
import { useOverlayStore } from '@/lib/overlayStore';
import { useSessionStore } from '@/lib/sessionStore';
import { useSettingsStore, type SettingsPage } from '@/lib/settingsStore';
import {
  describeEngine,
  describeModel,
  humanDuration,
  useSystemPolling,
  useSystemStore,
} from '@/lib/systemStore';
import { useWorkbenchStore } from '@/lib/workbenchStore';
import { useWorkspaceStore } from '@/lib/workspaceStore';
import { StatusDot } from './primitives';

/**
 * StatusBar(26px,D-040 重排):数据全部来自 systemStore / gitStore(统一轮询,页面隐藏暂停)。
 * 左 = 连接状态 · 工作区与 2D/3D · git 分支(ahead/behind + 增删行)· 运行中;
 * 右 = 待办进度 · 引擎与模型 · Codex 额度。每段可点,直达能处理这件事的地方;
 * 会话标题段下线(对话列头已有)。首轮探测完成前显示「检测中」,不先亮写死的结论。
 */

function openSettings(page: SettingsPage): void {
  useSettingsStore.getState().setPage(page);
  useOverlayStore.getState().open('settings');
}

function Seg({
  testId,
  title,
  onClick,
  className,
  children,
}: {
  testId: string;
  title?: string;
  onClick?: () => void;
  className?: string;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      data-testid={testId}
      title={title}
      onClick={onClick}
      disabled={onClick === undefined}
      className={cn(
        'flex h-full min-w-0 items-center gap-[5px] px-1.5 transition-colors enabled:hover:bg-shell-hover enabled:hover:text-fg-2 disabled:cursor-default',
        className,
      )}
    >
      {children}
    </button>
  );
}

function clock(epochSec: number | undefined): string {
  if (epochSec === undefined) return '';
  const d = new Date(epochSec * 1000);
  return Number.isNaN(d.getTime()) ? '' : d.toTimeString().slice(0, 5);
}

export default function StatusBar() {
  useSystemPolling();
  useGitPolling();

  const checked = useSystemStore((st) => st.checked);
  const online = useSystemStore((st) => st.online);
  const snapshotOk = useSystemStore((st) => st.snapshotOk);
  const health = useSystemStore((st) => st.health);
  const project = useSystemStore((st) => st.project);
  const todos = useSystemStore((st) => st.todos);
  const engines = useSystemStore((st) => st.engines);
  const catalog = useSystemStore((st) => st.catalog);
  const catalogDefault = useSystemStore((st) => st.defaultModelId);
  const git = useGitStore((st) => st.status);

  const activeSession = useSessionStore((st) => st.sessions.find((s) => s.id === st.activeSessionId));
  const draftEngine = useSessionStore((st) => st.draftAgentEngine);
  const chatModels = useChatStore((st) => st.models);
  const chatDefault = useChatStore((st) => st.defaultModelId);
  // 轮询到的目录优先(availability 实时);首轮前退回 chatStore 已加载的目录
  const models = catalog.length > 0 ? catalog : chatModels;
  const defaultModelId = catalogDefault ?? chatDefault;
  const chatSessionId = useChatStore((st) => st.currentSessionId);
  const chatSelected = useChatStore((st) => st.selectedModelId);
  const running = useChatStore((st) => st.activeRunId !== null);

  const engine = describeEngine(activeSession?.agentEngine ?? draftEngine, engines);
  // 与 Composer 模型选择器同口径:chatStore 已对齐当前会话时读它,否则读会话记录
  const wantedId = activeSession
    ? chatSessionId === activeSession.id
      ? chatSelected
      : (activeSession.selectedModelId ?? null)
    : chatSelected;
  const model = describeModel({ checked, engine, models, wantedId, defaultModelId });

  const conn = !checked
    ? { color: 'var(--dot-idle)', text: '检测中…' }
    : !online
      ? { color: 'var(--dot-blocked)', text: '未连接' }
      : snapshotOk
        ? { color: 'var(--dot-done)', text: '已连接' }
        : { color: 'var(--dot-queued)', text: 'agentd 未连接' };
  const connTitle = !checked
    ? '正在探测本地服务'
    : [
        online
          ? `host v${health?.version ?? '?'} · 已运行 ${humanDuration(health?.uptimeSec)}`
          : 'host(127.0.0.1:3080)不可达',
        health?.agentd
          ? health.agentd.ok
            ? `agentd v${health.agentd.version ?? '?'} · 已运行 ${humanDuration(health.agentd.uptimeSec)}`
            : 'agentd(127.0.0.1:8103)不可达'
          : '',
        '点击查看关于与服务状态',
      ]
        .filter(Boolean)
        .join('\n');

  const openWorkspacePicker = () => {
    const wb = useWorkbenchStore.getState();
    if (wb.collapsed.sessions) wb.togglePane('sessions');
    useWorkspaceStore.getState().setPickerOpen(true);
  };

  const showChanges = () => {
    useGitStore.getState().setChangesOnly(true);
    const wb = useWorkbenchStore.getState();
    if (wb.tabs.length === 0) wb.setHomeDismissed(true);
    if (wb.collapsed.inspector) wb.togglePane('inspector');
    wb.setRightTab('files');
  };

  const gitCounts = git?.counts;
  const changed = git?.total ?? 0;
  const gitTitle = git?.isRepo
    ? [
        git.detached ? 'HEAD 处于分离状态' : `分支 ${git.branch ?? '?'}${git.upstream ? `,上游 ${git.upstream}` : ''}`,
        git.rootUntracked
          ? '当前工作区目录未被 git 跟踪'
          : changed > 0 && gitCounts
            ? [
                gitCounts.modified && `${gitCounts.modified} 个修改`,
                gitCounts.added && `${gitCounts.added} 个新增`,
                gitCounts.deleted && `${gitCounts.deleted} 个删除`,
                gitCounts.renamed && `${gitCounts.renamed} 个重命名`,
                gitCounts.untracked && `${gitCounts.untracked} 个未跟踪`,
                gitCounts.conflicted && `${gitCounts.conflicted} 个冲突`,
              ]
                .filter(Boolean)
                .join(' · ')
            : '工作区无改动',
        '点击在右栏只看改动',
      ].join('\n')
    : undefined;

  const quota = engine.quota;

  return (
    <footer
      data-testid="shell-statusbar"
      className="flex h-[26px] shrink-0 items-center border-t border-edge bg-shell-sunk px-1 text-[11px] text-fg-3"
    >
      <Seg testId="statusbar-host" title={connTitle} onClick={() => openSettings('about')}>
        <StatusDot color={conn.color} />
        {conn.text}
      </Seg>
      {project && (
        <Seg
          testId="statusbar-project-mode"
          title={`${project.name} · forge.toml mode=${project.mode}\n点击切换工作区`}
          onClick={openWorkspacePicker}
        >
          <Boxes size={11} className="shrink-0" />
          <span className="truncate">{`${project.name} · ${project.mode === '2d' ? '2D' : '3D'}`}</span>
        </Seg>
      )}
      {git?.isRepo && (
        <Seg testId="statusbar-git" title={gitTitle} onClick={showChanges}>
          <GitBranch size={11} className="shrink-0" />
          <span className="truncate">{git.detached ? 'HEAD' : (git.branch ?? '?')}</span>
          {(git.ahead ?? 0) > 0 && <span className="font-code">↑{git.ahead}</span>}
          {(git.behind ?? 0) > 0 && <span className="font-code">↓{git.behind}</span>}
          {git.rootUntracked ? (
            <span className="text-fg-4">· 未跟踪</span>
          ) : changed > 0 ? (
            <span className="flex items-center gap-1 font-code">
              <span className="text-sage">+{git.insertions ?? 0}</span>
              <span className="text-danger">−{git.deletions ?? 0}</span>
            </span>
          ) : null}
        </Seg>
      )}
      {running && (
        <Seg testId="statusbar-run" title="Agent 正在运行" className="text-acc">
          <StatusDot color="var(--dot-running)" pulse />
          运行中
        </Seg>
      )}

      <span className="min-w-2 flex-1" />

      {todos && todos.total > 0 && (
        <Seg
          testId="statusbar-todos"
          title={`当前会话待办:已完成 ${todos.done} / 共 ${todos.total}\n点击打开 Todo 看板`}
          onClick={() => useWorkbenchStore.getState().openTab('todo')}
        >
          <ListChecks size={11} className="shrink-0" />
          <span className="font-code">{`${todos.done}/${todos.total}`}</span>
        </Seg>
      )}
      <Seg
        testId="statusbar-model"
        title={`${model.title}\n点击打开设置 · ${engine.engine === 'codex' ? 'Codex' : '模型'}`}
        onClick={() => openSettings(engine.engine === 'codex' ? 'codex' : 'models')}
      >
        {engine.engine === 'codex' ? <Bot size={11} className="shrink-0" /> : <Cpu size={11} className="shrink-0" />}
        <span data-testid="statusbar-engine" className="shrink-0">
          {engine.label}
        </span>
        <span className="text-fg-4">·</span>
        <span
          data-testid="statusbar-provider"
          data-state={model.state}
          className={cn(
            'max-w-[180px] truncate',
            (model.state === 'unconfigured' || model.state === 'login') && 'text-warn',
          )}
        >
          {model.label}
        </span>
      </Seg>
      {engine.engine === 'codex' && quota.remaining !== undefined && (
        <Seg
          testId="statusbar-codex-limit"
          title={`Codex 主窗口剩余 ${quota.remaining}%${quota.resetsAt ? `,约 ${clock(quota.resetsAt)} 重置` : ''}`}
          onClick={() => openSettings('codex')}
          className={quota.remaining <= 15 ? 'text-warn' : undefined}
        >
          额度 {quota.remaining}%
        </Seg>
      )}
    </footer>
  );
}
