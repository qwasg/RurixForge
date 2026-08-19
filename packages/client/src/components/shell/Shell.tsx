import { useCallback, useEffect, useRef, useState } from 'react';
import { ChevronLeft, ChevronRight } from 'lucide-react';
import { cn } from '@/lib/cn';
import { runCommand } from '@/lib/commands';
import { useOverlayStore } from '@/lib/overlayStore';
import { useSessionStore } from '@/lib/sessionStore';
import { PANE_CLAMP, useWorkbenchStore, type PaneKind } from '@/lib/workbenchStore';
import ChatColumn from './ChatColumn';
import BottomPanel from './BottomPanel';
import Inspector from './Inspector';
import Sidebar from './Sidebar';
import StatusBar from './StatusBar';
import TitleBar from './TitleBar';
import Workbench from './Workbench';
import SettingsOverlay from '../settings/SettingsOverlay';
import CommandPalette from './overlays/CommandPalette';
import Modals from './overlays/Modals';
import ToastStack from './overlays/ToastStack';

/**
 * F7 wave.3 壳总装(D-F7-B):
 * titlebar 36 + 主体(侧栏 | 9px 分隔条 | 对话列 | 9px 分隔条 | 主区 flex-1 | 9px 分隔条
 * | Inspector)+ statusbar 26;三栏宽拖拽(实时改宽,clamp)+ 折叠胶囊(点击折叠/展开);
 * 宽高持久化 forge:paneSizes(workbenchStore);全局 Esc 关全部浮层。
 */

/** 9px 分隔条 + 11×24 折叠胶囊(参考 pane_divider) */
function PaneDivider({
  kind,
  collapsed,
  onToggle,
  onDragStart,
}: {
  kind: PaneKind;
  collapsed: boolean;
  onToggle: () => void;
  onDragStart: (e: React.MouseEvent) => void;
}) {
  // inspector 在右:展开时点胶囊向右收(chevron-right),折叠后向左开(chevron-left);
  // sessions/chat 在左:展开时向左收,折叠后向右开。
  const pointLeft = kind === 'inspector' ? collapsed : !collapsed;
  return (
    <div
      data-testid={`pane-divider-${kind}`}
      onMouseDown={onDragStart}
      className={cn(
        'flex h-full w-[9px] shrink-0 cursor-col-resize items-center bg-shell-sunk transition-colors hover:bg-shell-hover',
        kind === 'inspector' ? 'justify-end' : 'justify-center',
      )}
    >
      <button
        type="button"
        title={collapsed ? '展开' : '折叠'}
        aria-label={`${collapsed ? '展开' : '折叠'} ${kind}`}
        data-testid={`pane-toggle-${kind}`}
        onMouseDown={(e) => e.stopPropagation()}
        onClick={(e) => {
          e.stopPropagation();
          onToggle();
        }}
        className="flex h-6 w-[11px] items-center justify-center rounded-full border border-edge bg-shell-panel text-fg-3 transition-colors hover:bg-shell-hover"
      >
        {pointLeft ? <ChevronLeft size={9} /> : <ChevronRight size={9} />}
      </button>
    </div>
  );
}

export default function Shell() {
  const paneW = useWorkbenchStore((st) => st.paneW);
  const collapsed = useWorkbenchStore((st) => st.collapsed);
  const setPaneW = useWorkbenchStore((st) => st.setPaneW);
  const togglePane = useWorkbenchStore((st) => st.togglePane);
  const loadAll = useSessionStore((st) => st.loadAll);
  const closeAll = useOverlayStore((st) => st.closeAll);

  const [dragging, setDragging] = useState<PaneKind | null>(null);
  const dragRef = useRef<{ kind: PaneKind; startX: number; startW: number } | null>(null);

  // 首次加载会话 + 文件夹
  useEffect(() => {
    void loadAll();
  }, [loadAll]);

  // 全局快捷键:Esc 关全部浮层;Ctrl+K 命令面板;Ctrl+Shift+N 新建会话;Ctrl+J 底部面板
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        closeAll();
        return;
      }
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'k') {
        e.preventDefault();
        runCommand('palette.open');
        return;
      }
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'j') {
        e.preventDefault();
        useWorkbenchStore.getState().toggleBottom();
        return;
      }
      if ((e.ctrlKey || e.metaKey) && e.shiftKey && e.key.toLowerCase() === 'n') {
        e.preventDefault();
        runCommand('session.new');
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [closeAll]);

  const onDragStart = useCallback(
    (kind: PaneKind) => (e: React.MouseEvent) => {
      e.preventDefault();
      dragRef.current = { kind, startX: e.clientX, startW: paneW[kind] };
      setDragging(kind);
    },
    [paneW],
  );

  useEffect(() => {
    if (!dragging) return;
    const onMove = (e: MouseEvent) => {
      const d = dragRef.current;
      if (!d) return;
      const delta = e.clientX - d.startX;
      const next = d.kind === 'inspector' ? d.startW - delta : d.startW + delta;
      setPaneW(d.kind, next);
    };
    const onUp = () => {
      dragRef.current = null;
      setDragging(null);
    };
    window.addEventListener('mousemove', onMove);
    window.addEventListener('mouseup', onUp);
    return () => {
      window.removeEventListener('mousemove', onMove);
      window.removeEventListener('mouseup', onUp);
    };
  }, [dragging, setPaneW]);

  return (
    <div
      data-testid="shell"
      className={cn('relative flex h-full flex-col bg-shell-bg text-fg', dragging && 'select-none')}
    >
      <TitleBar />
      <div className="flex min-h-0 flex-1">
        {!collapsed.sessions && (
          <div
            data-testid="pane-sessions"
            className="h-full shrink-0"
            style={{ width: paneW.sessions, minWidth: PANE_CLAMP.sessions.min, maxWidth: PANE_CLAMP.sessions.max }}
          >
            <Sidebar />
          </div>
        )}
        <PaneDivider
          kind="sessions"
          collapsed={collapsed.sessions}
          onToggle={() => togglePane('sessions')}
          onDragStart={onDragStart('sessions')}
        />
        {!collapsed.chat && (
          <div
            data-testid="pane-chat"
            className="h-full shrink-0"
            style={{ width: paneW.chat, minWidth: PANE_CLAMP.chat.min, maxWidth: PANE_CLAMP.chat.max }}
          >
            <ChatColumn />
          </div>
        )}
        <PaneDivider
          kind="chat"
          collapsed={collapsed.chat}
          onToggle={() => togglePane('chat')}
          onDragStart={onDragStart('chat')}
        />
        <div data-testid="pane-main" className="flex h-full min-w-0 flex-1 flex-col">
          <Workbench />
          <BottomPanel />
        </div>
        <PaneDivider
          kind="inspector"
          collapsed={collapsed.inspector}
          onToggle={() => togglePane('inspector')}
          onDragStart={onDragStart('inspector')}
        />
        {!collapsed.inspector && (
          <div
            data-testid="pane-inspector"
            className="h-full shrink-0"
            style={{ width: paneW.inspector, minWidth: PANE_CLAMP.inspector.min, maxWidth: PANE_CLAMP.inspector.max }}
          >
            <Inspector />
          </div>
        )}
      </div>
      <StatusBar />
      {/* 浮层层 */}
      <CommandPalette />
      <Modals />
      <SettingsOverlay />
      <ToastStack />
    </div>
  );
}
