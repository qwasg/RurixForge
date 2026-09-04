import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import App from '@/App';
import { MINI_CHAT } from '@/lib/chatVariant';
import { useOverlayStore } from '@/lib/overlayStore';
import { useSessionStore } from '@/lib/sessionStore';
import { useThemeStore } from '@/lib/themeStore';
import { PANE_CLAMP, useWorkbenchStore } from '@/lib/workbenchStore';
import { mockForgeBackend } from './forgeMock';

/**
 * F7 wave.3 壳组件测试:三栏渲染 / 折叠 / clamp / 分隔条拖拽模拟 / 编辑器 tab 嵌入。
 * 2026-08-24 起默认空态是全屏对话主页(workbench 无 tab 即接管),三栏要点「工作台」或开 tab 才现。
 * fetch mock:sessions/folders 空 + health + design-snapshot + 编辑器 MCP 面
 * (EditorView 挂载走真实 action,错误经 run() 收进 lastError,不崩)。
 */

const initialSessions = useSessionStore.getState();
const initialWorkbench = useWorkbenchStore.getState();
const initialTheme = useThemeStore.getState();
const initialOverlay = useOverlayStore.getState();

function stubShellFetch() {
  // 有状态迷你会话后端(wire 对齐 agentd:POST {session} / PATCH {session} / DELETE 空体)
  let seq = 0;
  const sessions: Array<Record<string, unknown>> = [];
  const sessionsHandler = (init?: { body?: string; method?: string }) => {
    const method = init?.method ?? 'GET';
    if (method === 'POST') {
      const body = JSON.parse(init?.body ?? '{}') as { title?: string };
      const ts = new Date().toISOString();
      const s = {
        id: `sess_t${++seq}`,
        title: body.title ?? '',
        status: 'idle',
        agentKind: 'coding',
        selectedModelId: null,
        webSearchEnabled: true,
        activeRunId: null,
        createdAt: ts,
        updatedAt: ts,
        pinned: false,
        titleManuallySet: false,
        folderId: null,
      };
      sessions.unshift(s);
      return { session: s };
    }
    if (method === 'PATCH') {
      const body = JSON.parse(init?.body ?? '{}') as Record<string, unknown>;
      const s = sessions[0];
      if (body.title !== undefined) {
        s.title = body.title;
        s.titleManuallySet = true;
      }
      if (body.pinned !== undefined) s.pinned = body.pinned;
      if ('folderId' in body) s.folderId = body.folderId;
      return { session: s };
    }
    if (method === 'DELETE') {
      sessions.splice(0, sessions.length);
      return null;
    }
    return { sessions };
  };
  vi.stubGlobal(
    'fetch',
    mockForgeBackend(
      {
        entity_list: { entities: [] },
        scene_summary: {
          name: 'Demo',
          entityCount: 1,
          playState: 'edit',
          render: { frames: 1, lastTris: 0, lastNonZeroPixels: 0 },
        },
        play_state: { state: 'edit' },
        host_events: [],
      },
      {
        '/api/forge/sessions': sessionsHandler,
        '/api/forge/chat-folders': { folders: [] },
        '/api/forge/health': { status: 'ok' },
        '/api/forge/design-snapshot': {
          sessions: [],
          activeSession: null,
          events: [],
          todos: [],
          run: null,
          models: {
            models: [
              { id: 'deepseek-chat', label: 'deepseek-chat', provider: 'deepseek', availability: 'unavailable' },
              { id: 'mock', label: 'Mock provider', provider: 'mock', availability: 'available' },
            ],
          },
          latestSeq: 0,
          chatFolders: [],
        },
      },
    ),
  );
}

beforeEach(() => {
  useSessionStore.setState(initialSessions, true);
  useWorkbenchStore.setState(initialWorkbench, true);
  useThemeStore.setState(initialTheme, true);
  useOverlayStore.setState(initialOverlay, true);
  globalThis.localStorage?.clear();
  stubShellFetch();
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe('<App /> 新壳', () => {
  it('默认空态:全屏对话主页接管主区(titlebar/侧栏/statusbar 在位,主区与右栏让位)', async () => {
    render(<App />);
    expect(screen.getByTestId('shell-titlebar')).toBeInTheDocument();
    expect(screen.getByTestId('pane-sessions')).toBeInTheDocument();
    expect(screen.getByTestId('shell-statusbar')).toBeInTheDocument();
    // workbench 无 tab → 对话接管整屏,主区/右栏/对话列窄栏都不在
    expect(screen.getByTestId('pane-home')).toBeInTheDocument();
    expect(screen.queryByTestId('pane-chat')).not.toBeInTheDocument();
    expect(screen.queryByTestId('pane-main')).not.toBeInTheDocument();
    expect(screen.queryByTestId('pane-inspector')).not.toBeInTheDocument();
    expect(screen.getByTestId('home-hero')).toBeInTheDocument();
    // 侧栏 New Agent + 空态文案
    expect(screen.getByTestId('pane-toggle-sessions')).toBeInTheDocument();
    expect(screen.getByTestId('sidebar-new-agent')).toBeInTheDocument();
    expect(await screen.findByText('暂无会话，可点击「New Agent」创建。')).toBeInTheDocument();
    // StatusBar provider 段(design-snapshot:deepseek unavailable → Mock provider)
    expect(await screen.findByText('Mock provider')).toBeInTheDocument();
  });

  it('主页「工作台」→ 三栏 + 主区空态卡;「回到全屏对话」→ 退回主页', async () => {
    render(<App />);
    fireEvent.click(screen.getByTestId('home-to-workbench'));
    expect(useWorkbenchStore.getState().homeDismissed).toBe(true);
    expect(screen.getByTestId('pane-chat')).toBeInTheDocument();
    expect(screen.getByTestId('pane-main')).toBeInTheDocument();
    expect(screen.getByTestId('pane-inspector')).toBeInTheDocument();
    expect(screen.getByTestId('pane-toggle-chat')).toBeInTheDocument();
    expect(screen.getByTestId('pane-toggle-inspector')).toBeInTheDocument();
    expect(screen.getByText('选择左侧会话或点击 New Agent')).toBeInTheDocument();
    expect(screen.getByText('个人工作区')).toBeInTheDocument();
    fireEvent.click(screen.getByTestId('empty-back-home'));
    expect(await screen.findByTestId('pane-home')).toBeInTheDocument();
    expect(screen.queryByTestId('pane-main')).not.toBeInTheDocument();
  });

  it('面板折叠:栏内图标折叠 + StatusBar 展开 + 持久化 forge:paneSizes', () => {
    render(<App />);
    fireEvent.click(screen.getByTestId('pane-toggle-sessions'));
    expect(useWorkbenchStore.getState().collapsed.sessions).toBe(true);
    expect(screen.queryByTestId('pane-sessions')).not.toBeInTheDocument();
    const raw = globalThis.localStorage?.getItem('forge:paneSizes');
    expect(raw).toBeTruthy();
    expect(JSON.parse(raw as string).collapsed.sessions).toBe(true);
    fireEvent.click(screen.getByLabelText('切换会话栏'));
    expect(useWorkbenchStore.getState().collapsed.sessions).toBe(false);
    expect(screen.getByTestId('pane-sessions')).toBeInTheDocument();
  });

  it('栏宽 clamp(200–360 / 300–560 / 240–420)', () => {
    const st = useWorkbenchStore.getState();
    st.setPaneW('sessions', 9999);
    expect(useWorkbenchStore.getState().paneW.sessions).toBe(360);
    st.setPaneW('sessions', 1);
    expect(useWorkbenchStore.getState().paneW.sessions).toBe(200);
    st.setPaneW('chat', 9999);
    expect(useWorkbenchStore.getState().paneW.chat).toBe(560);
    st.setPaneW('inspector', 1);
    expect(useWorkbenchStore.getState().paneW.inspector).toBe(240);
  });

  it('开编辑器 tab:主页让位三栏 + EditorView 骨架挂载;关掉最后一个 tab 自动回主页', async () => {
    render(<App />);
    useWorkbenchStore.getState().openEditor();
    expect(await screen.findByTestId('workbench-tab-editor')).toBeInTheDocument();
    expect(screen.getByTestId('pane-main')).toBeInTheDocument();
    expect(screen.queryByTestId('pane-home')).not.toBeInTheDocument();
    // EditorView(游戏原生)骨架:Viewport 页签;层级由右栏接管(响应式波)
    expect(screen.getByRole('button', { name: 'Viewport' })).toBeInTheDocument();
    expect(await screen.findByTestId('hierarchy-panel')).toBeInTheDocument();
    expect(screen.queryByTestId('inspector')).not.toBeInTheDocument();
    fireEvent.click(screen.getByLabelText('关闭 编辑器'));
    expect(screen.queryByTestId('workbench-tab-editor')).not.toBeInTheDocument();
    expect(await screen.findByTestId('home-hero')).toBeInTheDocument();
    // 手动进工作台才看得到空态卡,右栏也随之回落工作区文件树
    fireEvent.click(screen.getByTestId('home-to-workbench'));
    expect(await screen.findByText('个人工作区')).toBeInTheDocument();
    expect(await screen.findByTestId('inspector')).toBeInTheDocument();
  });

  it('主题切换:store action → data-theme + CSS 变量实测变化', () => {
    render(<App />);
    useThemeStore.getState().setMode('light');
    expect(document.documentElement.dataset.theme).toBe('light');
    expect(document.documentElement.style.getPropertyValue('--accent')).toBe('#C94F12');
    useThemeStore.getState().toggleMode();
    expect(document.documentElement.dataset.theme).toBe('dark');
    expect(document.documentElement.style.getPropertyValue('--accent')).toBe('#F47B33');
    expect(document.documentElement.style.getPropertyValue('--bg')).toBe('#191A1D');
  });

  it('会话流:New Agent 建行 → 选中 → 对话列头标题;重命名/置顶/删除', async () => {
    render(<App />);
    // 直接经 store(等价 POST /sessions;stub 见上)
    const created = await useSessionStore.getState().create('冒烟会话');
    expect(created).not.toBeNull();
    // 侧栏行 + 对话列头双处呈现
    expect((await screen.findAllByText('冒烟会话')).length).toBeGreaterThanOrEqual(1);
    expect(useSessionStore.getState().activeSessionId).toBe(created!.id);
    // 对话列头标题
    expect(screen.getByTestId('chat-head-title')).toHaveTextContent('冒烟会话');
    // 状态点:无 activeRunId → idle(灰)
    // 重命名(双击行内)
    const row = screen.getByTestId(`session-row-${created!.id}`);
    fireEvent.doubleClick(row);
    const input = await screen.findByTestId(`session-rename-${created!.id}`);
    fireEvent.change(input, { target: { value: '改名后' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect((await screen.findAllByText('改名后')).length).toBeGreaterThanOrEqual(1);
    // 置顶 → PINNED 区
    fireEvent.click(screen.getByLabelText('置顶'));
    expect(await screen.findByText('PINNED')).toBeInTheDocument();
    // 删除 → 回空态
    fireEvent.click(screen.getByLabelText('删除会话'));
    expect(await screen.findByText('暂无会话，可点击「New Agent」创建。')).toBeInTheDocument();
  });
});

/**
 * 分栏线拖拽(2026-08-25 用户拍板「四个区的分界线要能自由拉」):
 * 三条线各骑一个热区,拉相邻定宽栏,主区吃剩下的宽;clamp 与持久化沿用 setPaneW。
 */
describe('分栏线拖拽', () => {
  const enterWorkbench = () => fireEvent.click(screen.getByTestId('home-to-workbench'));
  /** 按住分栏线从 from 拉到 to(拖拽期走 window 上的 mousemove/mouseup)。 */
  function dragX(testId: string, from: number, to: number) {
    fireEvent.mouseDown(screen.getByTestId(testId), { clientX: from });
    fireEvent.mouseMove(window, { clientX: to });
    fireEvent.mouseUp(window);
  }

  it('拖会话栏分栏线:栏宽跟指针走 + 持久化', () => {
    render(<App />);
    dragX('pane-resize-sessions', 256, 320);
    expect(useWorkbenchStore.getState().paneW.sessions).toBe(320);
    expect(screen.getByTestId('pane-sessions').style.width).toBe('320px');
    const raw = JSON.parse(globalThis.localStorage?.getItem('forge:paneSizes') as string);
    expect(raw.w.sessions).toBe(320);
  });

  it('拖过头:卡在 clamp 上下界', () => {
    render(<App />);
    dragX('pane-resize-sessions', 256, 2000);
    expect(useWorkbenchStore.getState().paneW.sessions).toBe(PANE_CLAMP.sessions.max);
    dragX('pane-resize-sessions', 256, -2000);
    expect(useWorkbenchStore.getState().paneW.sessions).toBe(PANE_CLAMP.sessions.min);
  });

  it('对话列往右拉变宽;右栏在右侧,往左拉才变宽(方向取反)', () => {
    render(<App />);
    enterWorkbench();
    dragX('pane-resize-chat', 600, 700);
    expect(useWorkbenchStore.getState().paneW.chat).toBe(PANE_CLAMP.chat.def + 100);
    expect(screen.getByTestId('pane-chat').style.width).toBe(`${PANE_CLAMP.chat.def + 100}px`);
    dragX('pane-resize-inspector', 1000, 940);
    expect(useWorkbenchStore.getState().paneW.inspector).toBe(PANE_CLAMP.inspector.def + 60);
    expect(screen.getByTestId('pane-inspector').style.width).toBe(
      `${PANE_CLAMP.inspector.def + 60}px`,
    );
  });

  it('双击分栏线:回默认宽', () => {
    render(<App />);
    dragX('pane-resize-sessions', 256, 300);
    expect(useWorkbenchStore.getState().paneW.sessions).not.toBe(PANE_CLAMP.sessions.def);
    fireEvent.doubleClick(screen.getByTestId('pane-resize-sessions'));
    expect(useWorkbenchStore.getState().paneW.sessions).toBe(PANE_CLAMP.sessions.def);
  });

  it('不定宽/已折叠的栏不摆分栏线(主页态对话、浮窗态对话、收起的会话栏)', () => {
    render(<App />);
    expect(screen.getByTestId('pane-home')).toBeInTheDocument();
    expect(screen.getByTestId('pane-resize-sessions')).toBeInTheDocument();
    expect(screen.queryByTestId('pane-resize-chat')).not.toBeInTheDocument();

    enterWorkbench();
    expect(screen.getByTestId('pane-resize-chat')).toBeInTheDocument();
    expect(screen.getByTestId('pane-resize-inspector')).toBeInTheDocument();

    fireEvent.click(screen.getByTestId('pane-toggle-sessions'));
    expect(screen.queryByTestId('pane-resize-sessions')).not.toBeInTheDocument();
    fireEvent.click(screen.getByTestId('chat-mini-toggle'));
    expect(screen.queryByTestId('pane-resize-chat')).not.toBeInTheDocument();
  });
});

/**
 * 对话缩小态(2026-08-25 用户拍板):对话列头折叠钮左边多一枚缩小钮,
 * 点下去对话收成贴主区左下角的浮窗,会话栏一并收起;再点还原回三栏。
 */
describe('对话缩小成浮窗', () => {
  const enterWorkbench = () => fireEvent.click(screen.getByTestId('home-to-workbench'));

  it('缩小钮:对话脱离三栏收成左下角浮窗 + 会话栏自动收起 + 持久化', () => {
    render(<App />);
    enterWorkbench();
    expect(screen.getByTestId('pane-sessions')).toBeInTheDocument();

    fireEvent.click(screen.getByTestId('chat-mini-toggle'));

    expect(useWorkbenchStore.getState().chatMini).toBe(true);
    expect(useWorkbenchStore.getState().collapsed.sessions).toBe(true);
    expect(screen.queryByTestId('pane-sessions')).not.toBeInTheDocument();

    const pane = screen.getByTestId('pane-chat');
    expect(pane).toHaveAttribute('data-mini', '1');
    expect(pane.className).toContain('absolute');
    expect(pane.style.width).toBe(`${MINI_CHAT.w}px`);
    expect(pane.style.height).toBe(`${MINI_CHAT.h}px`);
    expect(pane.style.left).toBe(`${MINI_CHAT.gap}px`);
    expect(pane.style.bottom).toBe(`${MINI_CHAT.gap}px`);
    // 对话面切到 mini 变体,主区照常在位(吃回整片宽度)
    expect(screen.getByTestId('chat-column')).toHaveAttribute('data-variant', 'mini');
    expect(screen.getByTestId('pane-main')).toBeInTheDocument();

    const raw = JSON.parse(globalThis.localStorage?.getItem('forge:paneSizes') as string);
    expect(raw.chatMini).toBe(true);
    expect(raw.collapsed.sessions).toBe(true);
  });

  it('还原钮:回三栏对话列,会话栏放回缩小前的样子', () => {
    render(<App />);
    enterWorkbench();
    fireEvent.click(screen.getByTestId('chat-mini-toggle'));
    fireEvent.click(screen.getByLabelText('还原对话窗口'));

    expect(useWorkbenchStore.getState().chatMini).toBe(false);
    expect(screen.getByTestId('pane-sessions')).toBeInTheDocument();
    const pane = screen.getByTestId('pane-chat');
    expect(pane).not.toHaveAttribute('data-mini');
    expect(pane.style.width).toBe(`${PANE_CLAMP.chat.def}px`);
    expect(screen.getByTestId('chat-column')).toHaveAttribute('data-variant', 'column');
  });

  it('缩小前会话栏本就收着:还原后不擅自展开', () => {
    render(<App />);
    enterWorkbench();
    fireEvent.click(screen.getByTestId('pane-toggle-sessions'));
    fireEvent.click(screen.getByTestId('chat-mini-toggle'));
    fireEvent.click(screen.getByLabelText('还原对话窗口'));
    expect(useWorkbenchStore.getState().collapsed.sessions).toBe(true);
    expect(screen.queryByTestId('pane-sessions')).not.toBeInTheDocument();
  });

  it('缩小态里手动放回会话栏:浮窗右移让开,不压在会话列表上', () => {
    render(<App />);
    enterWorkbench();
    fireEvent.click(screen.getByTestId('chat-mini-toggle'));
    fireEvent.click(screen.getByLabelText('切换会话栏'));
    expect(screen.getByTestId('pane-chat').style.left).toBe(
      `${PANE_CLAMP.sessions.def + MINI_CHAT.gap}px`,
    );
  });

  // jsdom 不排版,主体区尺寸靠桩给:浮窗搬运全程只读这一个矩形
  const BODY = { w: 1200, h: 800 };
  function stubBodyRect(w = BODY.w, h = BODY.h) {
    const rect = { left: 0, top: 0, right: w, bottom: h, width: w, height: h };
    Object.defineProperty(screen.getByTestId('shell-body'), 'getBoundingClientRect', {
      configurable: true,
      value: () => ({ ...rect, x: 0, y: 0, toJSON: () => rect }),
    });
  }
  /** 进缩小态并把主体区尺寸打上桩,返回浮窗与窗头(拖把手)。 */
  function mini() {
    render(<App />);
    enterWorkbench();
    fireEvent.click(screen.getByTestId('chat-mini-toggle'));
    stubBodyRect();
    const pane = screen.getByTestId('pane-chat');
    return { pane, head: pane.querySelector('[data-mini-drag]') as HTMLElement };
  }
  /** 默认停靠位(左下角):x=gap,y=主体高 - 窗高 - gap。 */
  const DOCK = { x: MINI_CHAT.gap, y: BODY.h - MINI_CHAT.h - MINI_CHAT.gap };

  it('按住窗头拖动:浮窗跟指针走(抓点偏移不跳)+ 落点持久化', () => {
    const { pane, head } = mini();
    // 抓在窗头 (100,360) → 相对窗左上角偏移 (88, 12)
    fireEvent.mouseDown(head, { clientX: 100, clientY: 360 });
    fireEvent.mouseMove(window, { clientX: 400, clientY: 200 });
    fireEvent.mouseUp(window);

    expect(useWorkbenchStore.getState().miniPos).toEqual({ x: 312, y: 188 });
    expect(pane.style.left).toBe('312px');
    expect(pane.style.top).toBe('188px');
    expect(pane.style.bottom).toBe('');
    const raw = JSON.parse(globalThis.localStorage?.getItem('forge:paneSizes') as string);
    expect(raw.miniPos).toEqual({ x: 312, y: 188 });
  });

  it('拖出边界:整窗夹在主体区内,不会甩到抓不回来的地方', () => {
    const { pane, head } = mini();
    fireEvent.mouseDown(head, { clientX: DOCK.x + 8, clientY: DOCK.y + 8 });
    fireEvent.mouseMove(window, { clientX: 5000, clientY: 5000 });
    expect(pane.style.left).toBe(`${BODY.w - MINI_CHAT.w}px`);
    expect(pane.style.top).toBe(`${BODY.h - MINI_CHAT.h}px`);
    fireEvent.mouseMove(window, { clientX: -5000, clientY: -5000 });
    expect(pane.style.left).toBe('0px');
    expect(pane.style.top).toBe('0px');
    fireEvent.mouseUp(window);
  });

  it('双击窗头:归位回左下角(落点清空,重新跟会话栏让位)', () => {
    const { pane, head } = mini();
    fireEvent.mouseDown(head, { clientX: DOCK.x, clientY: DOCK.y });
    fireEvent.mouseMove(window, { clientX: 600, clientY: 100 });
    fireEvent.mouseUp(window);
    expect(useWorkbenchStore.getState().miniPos).not.toBeNull();

    fireEvent.doubleClick(head);
    expect(useWorkbenchStore.getState().miniPos).toBeNull();
    expect(pane.style.left).toBe(`${MINI_CHAT.gap}px`);
    expect(pane.style.bottom).toBe(`${MINI_CHAT.gap}px`);
    expect(pane.style.top).toBe('');
  });

  it('搬过之后落点定死:放回会话栏浮窗原地不动', () => {
    const { pane, head } = mini();
    fireEvent.mouseDown(head, { clientX: DOCK.x, clientY: DOCK.y });
    fireEvent.mouseMove(window, { clientX: 600, clientY: 300 });
    fireEvent.mouseUp(window);
    const { left, top } = pane.style;
    fireEvent.click(screen.getByLabelText('切换会话栏'));
    expect(pane.style.left).toBe(left);
    expect(pane.style.top).toBe(top);
  });

  it('窗头上的钮照常点:按在按钮上不起拖', () => {
    const { pane } = mini();
    fireEvent.mouseDown(screen.getByTestId('pane-toggle-chat'), { clientX: 300, clientY: 360 });
    fireEvent.mouseMove(window, { clientX: 900, clientY: 100 });
    expect(useWorkbenchStore.getState().miniPos).toBeNull();
    expect(pane.style.left).toBe(`${MINI_CHAT.gap}px`);
  });

  it('主体区变小:浮窗被收回可视区', () => {
    const { pane, head } = mini();
    fireEvent.mouseDown(head, { clientX: DOCK.x, clientY: DOCK.y });
    fireEvent.mouseMove(window, { clientX: 1000, clientY: 700 });
    fireEvent.mouseUp(window);
    expect(pane.style.left).toBe(`${BODY.w - MINI_CHAT.w}px`);

    stubBodyRect(700, 500);
    fireEvent.resize(window);
    expect(pane.style.left).toBe(`${700 - MINI_CHAT.w}px`);
    expect(pane.style.top).toBe(`${500 - MINI_CHAT.h}px`);
  });

  it('全屏对话主页优先:缩小态下没有 tab 时仍是整屏,不摆浮窗', async () => {
    render(<App />);
    enterWorkbench();
    fireEvent.click(screen.getByTestId('chat-mini-toggle'));
    useWorkbenchStore.getState().setHomeDismissed(false);
    const pane = await screen.findByTestId('pane-home');
    expect(pane).not.toHaveAttribute('data-mini');
    expect(screen.getByTestId('chat-column')).toHaveAttribute('data-variant', 'home');
  });
});
