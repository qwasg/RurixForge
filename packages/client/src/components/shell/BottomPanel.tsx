import { useCallback, useRef, useState } from 'react';
import { Activity, ChevronDown, ChevronRight, Logs, ScrollText, Trash2, X } from 'lucide-react';
import { cn } from '@/lib/cn';
import { useChatStore, type ForgeEventWire } from '@/lib/chatStore';
import { useWorkbenchStore, type BottomTab } from '@/lib/workbenchStore';

/**
 * F7 wave.5 底部面板(参考 render_bottom_panel):
 * 32px tab 条(激活顶 2px accent;Agent Logs[scroll-text+计数] / Output[logs+计数] / Metrics[activity])
 * + 右侧清空/关闭钮;顶部 4px 拖拽条(默认高 260 clamp 120–520,持久化 forge:bottomPanel);
 * Ctrl+J 开关(Shell 接线);statusbar 开关钮接线。
 *
 * - Agent Logs:最近 120 条原始事件可折叠树(#seq + type,展开 pretty JSON 前 40 行);
 * - Output:事件派生文本行(时间 + type + 一句话摘要,最近 200 条);
 * - Metrics:本地派生卡(Total tokens[chatStore usage 累计] / Tool calls[消息时间线计数] /
 *   Sessions[todo 完成数/总数] / Run 状态)。
 *
 * 差异留痕:参考还有 Problems/Terminal tab——本仓无诊断面/无 PTY,不落(RD-F7-002),
 * 不加占位空 tab(诚实);maximize 钮参考为空操作,不落。
 */

/** 事件 → Output 一句话摘要(派生,不伪造载荷外信息)。 */
export function summarizeEvent(evt: ForgeEventWire): string {
  const p = (evt.payload ?? {}) as Record<string, unknown>;
  const str = (k: string) => (typeof p[k] === 'string' ? (p[k] as string) : '');
  switch (evt.type) {
    case 'composer.user.message':
      return `用户消息(${str('composerMode') || 'build'}):${str('text').slice(0, 60)}`;
    case 'agent.started':
      return `run 开始 · model=${str('model') || 'default'}`;
    case 'agent.tool.invoked':
      return `工具调用:${str('name')}`;
    case 'agent.tool.completed':
      return `工具完成:${str('name')}${typeof p.durationMs === 'number' ? ` · ${p.durationMs}ms` : ''}`;
    case 'agent.tool.failed':
      return `工具失败:${str('name')} · ${str('error').slice(0, 60)}`;
    case 'agent.message':
      return `助手答复:${str('text').slice(0, 60)}`;
    case 'agent.completed':
      return 'run 完成';
    case 'agent.failed':
      return `run 失败:${str('error').slice(0, 60)}`;
    case 'agent.cancelled':
      return 'run 已中止';
    case 'agent.usage': {
      const t = typeof p.totalTokens === 'number' ? p.totalTokens : 0;
      return `用量 +${t} tokens`;
    }
    case 'todo.created':
      return `待办创建:${str('title')}`;
    case 'todo.updated':
      return `待办更新:${str('title') || str('id')} → ${str('status')}`;
    case 'session.created':
      return '会话创建';
    case 'session.updated':
      return '会话更新';
    case 'session.forked':
      return '会话分叉';
    case 'session.reverted':
      return '会话回退';
    default:
      return evt.type;
  }
}

function hhmmss(ts: string | undefined): string {
  return ts && ts.length >= 19 ? ts.slice(11, 19) : '--:--:--';
}

// ---------- Agent Logs(可折叠事件树) ----------

function LogsView() {
  const eventsRing = useChatStore((st) => st.eventsRing);
  const [expanded, setExpanded] = useState<Record<string, boolean>>({});
  const events = eventsRing.slice(-120).reverse();
  if (events.length === 0) {
    return <div className="px-3 py-2 text-[11px] text-fg-4">（暂无事件）</div>;
  }
  return (
    <div className="flex flex-col px-2 py-1" data-testid="logs-tree">
      {events.map((evt) => {
        const key = evt.id || `seq-${evt.seq}-${evt.type}`;
        const open = expanded[key] === true;
        return (
          <div key={key}>
            <button
              type="button"
              data-testid={`log-row-${evt.seq}`}
              onClick={() => setExpanded((m) => ({ ...m, [key]: !open }))}
              className="flex w-full items-center gap-1.5 rounded px-1 py-[1px] text-left hover:bg-shell-hover"
            >
              {open ? (
                <ChevronDown size={9} className="shrink-0 text-fg-4" />
              ) : (
                <ChevronRight size={9} className="shrink-0 text-fg-4" />
              )}
              <span className="font-code text-[10.5px] text-fg-4">#{evt.seq}</span>
              <span className="font-code text-[10.5px] text-fg-2">{evt.type}</span>
            </button>
            {open && (
              <pre
                data-testid={`log-payload-${evt.seq}`}
                className="my-0.5 ml-[18px] max-h-[300px] overflow-auto rounded bg-shell-sunk p-1.5 font-code text-[10.5px] text-fg-3"
              >
                {JSON.stringify(evt.payload ?? {}, null, 2).split('\n').slice(0, 40).join('\n')}
              </pre>
            )}
          </div>
        );
      })}
    </div>
  );
}

// ---------- Output(派生文本行) ----------

function OutputView() {
  const eventsRing = useChatStore((st) => st.eventsRing);
  const events = eventsRing.slice(-200).reverse();
  if (events.length === 0) {
    return <div className="px-3 py-2 text-[11px] text-fg-4">（无输出）</div>;
  }
  return (
    <div className="flex flex-col px-3 py-1" data-testid="output-lines">
      {events.map((evt) => (
        <div key={evt.id || `seq-${evt.seq}-${evt.type}`} className="flex items-baseline gap-2 py-px">
          <span className="shrink-0 font-code text-[10.5px] text-fg-4">{hhmmss(evt.ts)}</span>
          <span className="shrink-0 font-code text-[10.5px] text-fg-3">{evt.type}</span>
          <span className="min-w-0 truncate text-[11px] text-fg-2">{summarizeEvent(evt)}</span>
        </div>
      ))}
    </div>
  );
}

// ---------- Metrics(本地派生卡) ----------

function MetricsView() {
  const tokens = useChatStore((st) => st.tokens);
  const messages = useChatStore((st) => st.messages);
  const todos = useChatStore((st) => st.todos);
  const activeRunId = useChatStore((st) => st.activeRunId);

  // Tool calls = 消息时间线工具块计数(含成功/失败/进行中,如实口径)
  const toolCalls = messages.reduce(
    (n, m) => n + m.blocks.filter((b) => b.kind === 'tool').length,
    0,
  );
  const done = todos.filter((t) => t.status === 'completed' || t.status === 'done').length;

  const card = (label: string, value: string, testId: string) => (
    <div
      data-testid={testId}
      className="flex min-w-[120px] flex-1 flex-col gap-0.5 rounded-lg border border-edge bg-shell-panel p-2.5"
    >
      <span className="text-[10px] text-fg-4">{label}</span>
      <span className="font-code text-[18px] text-fg">{value}</span>
    </div>
  );

  return (
    <div className="flex gap-2 p-2.5" data-testid="metrics-cards">
      {card('Total tokens', String(tokens.total), 'metric-tokens')}
      {card('Tool calls', String(toolCalls), 'metric-toolcalls')}
      {card('Sessions', `${done}/${todos.length}`, 'metric-sessions')}
      {card('Run 状态', activeRunId ? 'running' : 'idle', 'metric-run')}
    </div>
  );
}

// ---------- 面板总装 ----------

const TABS: Array<{ id: BottomTab; label: string; icon: typeof ScrollText }> = [
  { id: 'logs', label: 'Agent Logs', icon: ScrollText },
  { id: 'output', label: 'Output', icon: Logs },
  { id: 'metrics', label: 'Metrics', icon: Activity },
];

export default function BottomPanel() {
  const open = useWorkbenchStore((st) => st.bottomOpen);
  const height = useWorkbenchStore((st) => st.bottomH);
  const active = useWorkbenchStore((st) => st.bottomTab);
  const setBottomTab = useWorkbenchStore((st) => st.setBottomTab);
  const setBottomH = useWorkbenchStore((st) => st.setBottomH);
  const toggleBottom = useWorkbenchStore((st) => st.toggleBottom);
  const count = useChatStore((st) => st.eventsRing.length);
  const clearEventsRing = useChatStore((st) => st.clearEventsRing);
  const [dragging, setDragging] = useState(false);
  const dragRef = useRef<{ startY: number; startH: number } | null>(null);

  const onDragStart = useCallback(
    (e: React.MouseEvent) => {
      e.preventDefault();
      dragRef.current = { startY: e.clientY, startH: height };
      setDragging(true);
      const onMove = (ev: MouseEvent) => {
        const d = dragRef.current;
        if (!d) return;
        setBottomH(d.startH + (d.startY - ev.clientY));
      };
      const onUp = () => {
        dragRef.current = null;
        setDragging(false);
        window.removeEventListener('mousemove', onMove);
        window.removeEventListener('mouseup', onUp);
      };
      window.addEventListener('mousemove', onMove);
      window.addEventListener('mouseup', onUp);
    },
    [height, setBottomH],
  );

  if (!open) return null;

  return (
    <div
      data-testid="bottom-panel"
      className={cn('flex shrink-0 flex-col border-t border-edge bg-shell-bg', dragging && 'select-none')}
      style={{ height }}
    >
      {/* 顶部 4px 拖拽条 */}
      <div
        data-testid="bottom-drag"
        onMouseDown={onDragStart}
        className="h-[4px] shrink-0 cursor-row-resize bg-transparent transition-colors hover:bg-acc-ring"
      />
      {/* 32px tab 条 */}
      <div className="flex h-[32px] shrink-0 items-center border-b border-edge bg-shell-sunk">
        {TABS.map((t) => {
          const isActive = active === t.id;
          const Icon = t.icon;
          return (
            <button
              key={t.id}
              type="button"
              data-testid={`bottom-tab-${t.id}`}
              onClick={() => setBottomTab(t.id)}
              className={cn(
                'flex h-[32px] items-center gap-[5px] border-t-2 px-3 text-[12px]',
                isActive ? 'border-acc bg-shell-bg text-fg' : 'border-transparent text-fg-3',
              )}
              style={{ borderTopColor: isActive ? 'var(--accent)' : 'transparent' }}
            >
              <Icon size={11} className={isActive ? 'text-fg-2' : 'text-fg-4'} />
              {t.label}
              {t.id !== 'metrics' && (
                <span className="font-code text-[10px] text-fg-4">{t.id === 'logs' ? Math.min(count, 120) : Math.min(count, 200)}</span>
              )}
            </button>
          );
        })}
        <span className="flex-1" />
        <button
          type="button"
          title="清空事件"
          aria-label="清空事件"
          data-testid="bottom-clear"
          onClick={() => clearEventsRing()}
          className="flex h-full items-center px-1.5 text-fg-3 hover:text-fg"
        >
          <Trash2 size={12} />
        </button>
        <button
          type="button"
          title="关闭面板(Ctrl+J)"
          aria-label="关闭底部面板"
          data-testid="bottom-close"
          onClick={toggleBottom}
          className="flex h-full items-center px-1.5 text-fg-3 hover:text-fg"
        >
          <X size={12} />
        </button>
      </div>
      {/* 体 */}
      <div className="min-h-0 flex-1 overflow-y-auto">
        {active === 'logs' && <LogsView />}
        {active === 'output' && <OutputView />}
        {active === 'metrics' && <MetricsView />}
      </div>
    </div>
  );
}
