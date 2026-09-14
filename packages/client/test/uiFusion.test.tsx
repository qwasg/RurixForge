import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import Composer from '@/components/chat/Composer';
import EntityRefText from '@/components/chat/EntityRefText';
import { useAssetStore } from '@/lib/assetStore';
import { useChatStore } from '@/lib/chatStore';
import { useEditorStore, type EntityData } from '@/lib/editorStore';
import { notifyAgentToolSettled } from '@/lib/editorSync';
import { jumpToEntity } from '@/lib/entityJump';
import { useSessionStore } from '@/lib/sessionStore';
import { useWorkbenchStore } from '@/lib/workbenchStore';
import { mockForgeBackend } from './forgeMock';

/**
 * UI 融合波(2026-08-20)测试:
 * C2 #id 实体引用链接化 + 回跳守卫;
 * C3 agent 工具落定 → 编辑器/资产精准刷新(白名单 + 300ms 拖尾防抖)。
 * C1 上下文 chip 与 C4 collectProblems 用例随各自功能退役删除。
 */

const E42: EntityData = {
  id: 42,
  name: 'Crate',
  transform: { translation: [1, 0, 2], rotation: [0, 0, 0, 1], scale: [1, 1, 1] },
  components: [],
};

const initialEditor = useEditorStore.getState();
const initialAssets = useAssetStore.getState();
const initialChat = useChatStore.getState();
const initialSessions = useSessionStore.getState();
const initialWorkbench = useWorkbenchStore.getState();

beforeEach(() => {
  useEditorStore.setState(initialEditor, true);
  useAssetStore.setState(initialAssets, true);
  useChatStore.setState(initialChat, true);
  useChatStore.getState().reset();
  useSessionStore.setState(initialSessions, true);
  useWorkbenchStore.setState(initialWorkbench, true);
  globalThis.localStorage?.clear();
  vi.stubGlobal('fetch', mockForgeBackend({}, {}));
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

// ---------- C1 上下文 chip 退役守卫 ----------

describe('C1 上下文 chip 已退役', () => {
  it('三源全选中也不渲染 chip 行;发送正文即草稿原文,不带【上下文】前缀', () => {
    useEditorStore.setState({ sceneName: 'maze.rxscene', entities: [E42], selectedId: 42 });
    useAssetStore.setState({
      items: [{ path: 'Content/Textures/brick.png', guid: 'g1', type: 'texture', size: 1 }],
      selectedGuid: 'g1',
    });
    const sendMessage = vi.fn();
    useChatStore.setState({ sendMessage });
    useSessionStore.setState({ activeSessionId: 'sess_1' });
    render(<Composer />);

    expect(screen.queryByTestId('context-chips')).toBeNull();
    fireEvent.change(screen.getByTestId('composer-input'), { target: { value: '调整一下' } });
    fireEvent.click(screen.getByTestId('composer-send'));
    expect(sendMessage).toHaveBeenCalledWith('调整一下', 'build');
  });
});

// ---------- C2 实体引用回跳 ----------

describe('C2 EntityRefText / jumpToEntity', () => {
  it('#id 链接化;命中实体点击 → 开编辑器 tab + 选中 + 聚焦', () => {
    const focusSelected = vi.fn();
    useEditorStore.setState({ entities: [E42], focusSelected });
    render(<EntityRefText text="已创建实体 #42,请查收" />);
    fireEvent.click(screen.getByTestId('entity-ref-42'));
    expect(useEditorStore.getState().selectedId).toBe(42);
    expect(useWorkbenchStore.getState().tabs.some((t) => t.kind === 'editor')).toBe(true);
    expect(focusSelected).toHaveBeenCalledTimes(1);
  });

  it('未命中实体的 #id 点击不动作(不伪造跳转)', () => {
    const focusSelected = vi.fn();
    useEditorStore.setState({ entities: [E42], selectedId: null, focusSelected });
    render(<EntityRefText text="编号 #999 不存在" />);
    fireEvent.click(screen.getByTestId('entity-ref-999'));
    expect(useEditorStore.getState().selectedId).toBeNull();
    expect(focusSelected).not.toHaveBeenCalled();
  });

  it('jumpToEntity:id 为 null 选中流不扰动', () => {
    useEditorStore.setState({ entities: [E42], selectedId: 7 });
    jumpToEntity(7); // 7 不在清单 → 守卫返回
    expect(useEditorStore.getState().selectedId).toBe(7);
  });
});

// ---------- C3 工具事件 → 编辑器精准刷新 ----------

describe('C3 notifyAgentToolSettled', () => {
  it('变更类工具 → 300ms 拖尾防抖合并刷新(实体/摘要)', () => {
    vi.useFakeTimers();
    const loadEntities = vi.fn();
    const refreshSummary = vi.fn();
    useEditorStore.setState({ loadEntities, refreshSummary });
    notifyAgentToolSettled('mcp__engine-scene__entity_create');
    notifyAgentToolSettled('mcp__engine-scene__transform_set'); // 拖尾合并
    vi.advanceTimersByTime(299);
    expect(loadEntities).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(loadEntities).toHaveBeenCalledTimes(1);
    expect(refreshSummary).toHaveBeenCalledTimes(1);
  });

  it('只读工具不触发;play.* 附刷 playState;资产变更类刷资产清单', () => {
    vi.useFakeTimers();
    const loadEntities = vi.fn();
    const refreshPlayState = vi.fn();
    const assetLoad = vi.fn();
    useEditorStore.setState({ loadEntities, refreshPlayState });
    useAssetStore.setState({ load: assetLoad });

    notifyAgentToolSettled('mcp__engine-scene__entity_list'); // 只读
    vi.advanceTimersByTime(500);
    expect(loadEntities).not.toHaveBeenCalled();

    notifyAgentToolSettled('mcp__engine-scene__play_enter');
    vi.advanceTimersByTime(300);
    expect(refreshPlayState).toHaveBeenCalledTimes(1);
    expect(loadEntities).not.toHaveBeenCalled(); // play 不触实体重拉

    notifyAgentToolSettled('mcp__gen-image__gen_accept');
    vi.advanceTimersByTime(300);
    expect(assetLoad).toHaveBeenCalledTimes(1);
  });
});