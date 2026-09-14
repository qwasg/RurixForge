import { useCallback, useEffect, useRef, useState } from 'react';
import type { MouseEvent as ReactMouseEvent } from 'react';
import { runCommand } from '@/lib/commands';
import { useOverlayStore } from '@/lib/overlayStore';
import { useSessionStore } from '@/lib/sessionStore';
import { useChatStore } from '@/lib/chatStore';
import { useWorkspaceStore } from '@/lib/workspaceStore';
import { PANE_CLAMP, useHomeMode, useWorkbenchStore } from '@/lib/workbenchStore';
import { clampMiniPos, MINI_CHAT } from '@/lib/chatVariant';
import { cn } from '@/lib/cn';
import ChatColumn from './ChatColumn';
import BottomPanel from './BottomPanel';
import RightPane from './RightPane';
import { PaneResizer } from './primitives';
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
 * titlebar 36 + 主体(侧栏 | 对话列 | 主区 flex-1 | 右栏)+ statusbar 26;
 * 各栏之间 1px 分栏线(亮色主题下 panel 与 bg 同色,无线则区域粘连);
 * 三栏折叠由各栏内图标钮控制,展开由 TitleBar 面板开关;宽高持久化 forge:paneSizes;
 * 全局 Esc 关全部浮层。
 *
 * 分栏线可拉(2026-08-25 用户拍板「四个区的分界线要能自由拉」):三条线各骑一个 PaneResizer
 * 热区,拉的是相邻定宽栏(会话栏/对话列/右栏),主区 flex-1 吃剩下的宽度,四个区跟着一起变;
 * 双击回默认宽。热区绝对定位骑在线上,所以定宽栏要 relative。
 *
 * 全屏对话主页(2026-08-24 用户拍板):workbench 一个 tab 都没有时不再摆空态卡,
 * 对话直接接管主区(隐藏主区/右栏,忽略对话栏折叠),像 Codex 一样先给整屏输入;
 * 开任一 tab 或点头部「工作台」即回三栏壳。底部面板跟着对话走,Ctrl+J 在主页照常可用。
 *
 * 对话缩小态(2026-08-25 用户拍板):对话列头缩小钮把对话从三栏流里摘出来,收成贴主区
 * 左下角的浮窗(MINI_CHAT),主区吃回整片宽度,会话栏一并收起;还原钮回三栏。
 * 浮窗与列共用同一个 pane 容器节点(只换定位/尺寸),切换不重挂 ChatColumn,草稿不丢。
 *
 * 浮窗拖动(2026-08-25 用户拍板「缩小窗口要能随意移动」):按住浮窗头(ChatColumn 头标了
 * data-mini-drag)在主体区里任意拖,落点持久化;双击头归位回左下角。拖拽走事件委派——
 * 监听挂在 pane 上按 data-mini-drag 认起拖点,ChatColumn 不必知道自己被谁定位。
 */

export default function Shell() {
  const paneW = useWorkbenchStore((st) => st.paneW);
  const collapsed = useWorkbenchStore((st) => st.collapsed);
  const chatMini = useWorkbenchStore((st) => st.chatMini);
  const miniPos = useWorkbenchStore((st) => st.miniPos);
  const setMiniPos = useWorkbenchStore((st) => st.setMiniPos);
  const homeMode = useHomeMode();
  const loadAll = useSessionStore((st) => st.loadAll);
  const loadWorkspaces = useWorkspaceStore((st) => st.loadAll);
  const closeAll = useOverlayStore((st) => st.closeAll);
  const bodyRef = useRef<HTMLDivElement>(null);
  const [miniDragging, setMiniDragging] = useState(false);

  // 首次加载会话 + 文件夹 + 工作区 + 模型快照
  useEffect(() => {
    void loadAll();
    void loadWorkspaces();
    void useChatStore.getState().ensureModels();
  }, [loadAll, loadWorkspaces]);

  // 全局快捷键:Esc 关全部浮层;Ctrl+K 命令面板;Ctrl+Shift+N 新建会话;Ctrl+J 底部面板;Ctrl+S 挡浏览器保存
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
      // F9:全局挡浏览器保存对话框;真实保存由文件编辑器(CM keymap/自身监听)消费。
      if ((e.ctrlKey || e.metaKey) && !e.shiftKey && e.key.toLowerCase() === 's') {
        e.preventDefault();
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

  // 主页优先:全屏对话接管时忽略缩小态(整屏与浮窗二选一)。
  const miniChat = !homeMode && chatMini && !collapsed.chat;

  /** 没搬过时的默认停靠横坐标:贴主区左边,会话栏若被放回来就让开它。 */
  const miniDockX = (collapsed.sessions ? 0 : paneW.sessions) + MINI_CHAT.gap;

  const onMiniDragStart = useCallback(
    (e: ReactMouseEvent<HTMLDivElement>) => {
      const target = e.target as HTMLElement | null;
      if (!target?.closest('[data-mini-drag]')) return;
      // 头上的钮/改名框照常点,不当拖把手
      if (target.closest('button, input, textarea, [role="menu"]')) return;
      const body = bodyRef.current;
      if (!body) return;
      const b = body.getBoundingClientRect();
      const from =
        miniPos ??
        clampMiniPos(miniDockX, b.height - MINI_CHAT.h - MINI_CHAT.gap, b.width, b.height);
      // 抓点相对窗左上角的偏移:全程按它换算,窗角不会跳到指针下
      const grabX = e.clientX - b.left - from.x;
      const grabY = e.clientY - b.top - from.y;
      e.preventDefault();
      setMiniDragging(true);
      const onMove = (ev: MouseEvent) => {
        const r = bodyRef.current?.getBoundingClientRect() ?? b;
        setMiniPos(
          clampMiniPos(ev.clientX - r.left - grabX, ev.clientY - r.top - grabY, r.width, r.height),
        );
      };
      const onUp = () => {
        setMiniDragging(false);
        window.removeEventListener('mousemove', onMove);
        window.removeEventListener('mouseup', onUp);
      };
      window.addEventListener('mousemove', onMove);
      window.addEventListener('mouseup', onUp);
    },
    [miniDockX, miniPos, setMiniPos],
  );

  /** 双击窗头归位:搬丢了也能一键回左下角。 */
  const onMiniDockBack = useCallback(
    (e: ReactMouseEvent<HTMLDivElement>) => {
      if ((e.target as HTMLElement | null)?.closest('[data-mini-drag]')) setMiniPos(null);
    },
    [setMiniPos],
  );

  // 窗口/布局变小后把浮窗收回可视区,免得落点留在边界外抓不回来
  useEffect(() => {
    if (!miniChat || !miniPos) return;
    const onResize = () => {
      const r = bodyRef.current?.getBoundingClientRect();
      if (!r || r.width === 0 || r.height === 0) return;
      setMiniPos(clampMiniPos(miniPos.x, miniPos.y, r.width, r.height));
    };
    window.addEventListener('resize', onResize);
    return () => window.removeEventListener('resize', onResize);
  }, [miniChat, miniPos, setMiniPos]);

  return (
    <div data-testid="shell" className="relative flex h-full flex-col bg-shell-bg text-fg">
      <TitleBar />
      <div ref={bodyRef} data-testid="shell-body" className="relative flex min-h-0 flex-1">
        {!collapsed.sessions && (
          <div
            data-testid="pane-sessions"
            className="relative h-full shrink-0 border-r border-edge-strong"
            style={{ width: paneW.sessions, minWidth: PANE_CLAMP.sessions.min, maxWidth: PANE_CLAMP.sessions.max }}
          >
            <Sidebar />
            <PaneResizer kind="sessions" side="right" />
          </div>
        )}
        {(homeMode || !collapsed.chat) && (
          <div
            data-testid={homeMode ? 'pane-home' : 'pane-chat'}
            data-mini={miniChat ? '1' : undefined}
            onMouseDown={miniChat ? onMiniDragStart : undefined}
            onDoubleClick={miniChat ? onMiniDockBack : undefined}
            className={cn(
              'flex flex-col',
              homeMode && 'h-full min-w-0 flex-1',
              !homeMode && !miniChat && 'relative h-full shrink-0 border-r border-edge-strong',
              miniChat &&
                'absolute z-30 overflow-hidden rounded-xl border border-edge-strong bg-shell-float shadow-float',
              miniDragging && 'cursor-grabbing select-none',
            )}
            style={
              homeMode
                ? undefined
                : miniChat
                  ? {
                      width: MINI_CHAT.w,
                      height: MINI_CHAT.h,
                      // 搬过就认落点;没搬过贴主区左下角,会话栏若被手动放回来则让开它
                      left: miniPos ? miniPos.x : miniDockX,
                      ...(miniPos ? { top: miniPos.y } : { bottom: MINI_CHAT.gap }),
                    }
                  : { width: paneW.chat, minWidth: PANE_CLAMP.chat.min, maxWidth: PANE_CLAMP.chat.max }
            }
          >
            {/* 内层壳固定存在:主页/对话列/浮窗切换时 ChatColumn 不重挂,草稿与模式不丢 */}
            <div className="min-h-0 flex-1">
              <ChatColumn variant={homeMode ? 'home' : miniChat ? 'mini' : 'column'} />
            </div>
            {homeMode && <BottomPanel />}
            {/* 浮窗态不定宽(拉的是浮窗尺寸,不归栏宽管),主页态吃 flex-1 也没得拉 */}
            {!homeMode && !miniChat && <PaneResizer kind="chat" side="right" />}
          </div>
        )}
        {!homeMode && (
          <div data-testid="pane-main" className="flex h-full min-w-0 flex-1 flex-col">
            <Workbench />
            <BottomPanel />
          </div>
        )}
        {!homeMode && !collapsed.inspector && (
          <div
            data-testid="pane-inspector"
            className="relative h-full shrink-0 border-l border-edge-strong"
            style={{ width: paneW.inspector, minWidth: PANE_CLAMP.inspector.min, maxWidth: PANE_CLAMP.inspector.max }}
          >
            <RightPane />
            <PaneResizer kind="inspector" side="left" />
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
