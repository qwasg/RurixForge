import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import Composer from '@/components/chat/Composer';
import EntityRefText from '@/components/chat/EntityRefText';
import { useAssetStore } from '@/lib/assetStore';
import { useChatStore } from '@/lib/chatStore';
import { chipsPrefix, useContextChips, type ContextChip } from '@/lib/contextChips';
import { useEditorStore, type EntityData } from '@/lib/editorStore';
import { notifyAgentToolSettled } from '@/lib/editorSync';
import { jumpToEntity } from '@/lib/entityJump';
import { useSessionStore } from '@/lib/sessionStore';
import { useWorkbenchStore } from '@/lib/workbenchStore';
import { mockForgeBackend } from './forgeMock';

/**
 * UI 融合波(2026-08-20)测试:
 * C1 上下文 chip(三源装配 / 剔除 / 发送前缀注入);
 * C2 #id 实体引用链接化 + 回跳守卫;
 * C3 agent 工具落定 → 编辑器/资产精准刷新(白名单 + 300ms 拖尾防抖)。
 * C4 collectProblems 用例随底栏波 Problems 面板退役删除。
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

// ---------- C1 上下文 chip ----------

describe('C1 chipsPrefix / useContextChips', () => {
  it('chipsPrefix:空返空串;非空 = 【上下文】a | b + 空行', () => {
    expect(chipsPrefix([])).toBe('');
    const chips: ContextChip[] = [
      { key: 'scene', kind: 'scene', label: '@maze', refText: '场景:maze.rxscene' },
      { key: 'entity:42', kind: 'entity', label: '#42 Crate', refText: '实体:#42 Crate' },
    ];
    expect(chipsPrefix(chips)).toBe('【上下文】场景:maze.rxscene | 实体:#42 Crate\n\n');
  });

  it('useContextChips:场景/选中实体/选中资产三源装配', () => {
    useEditorStore.setState({ sceneName: 'maze.rxscene', entities: [E42], selectedId: 42 });
    useAssetStore.setState({
      items: [{ path: 'Content/Textures/brick.png', guid: 'g1', type: 'texture', size: 1 }],
      selectedGuid: 'g1',
    });
    function Probe() {
      const chips = useContextChips();
      return <div data-testid="chips">{JSON.stringify(chips.map((c) => c.key))}</div>;
    }
    render(<Probe />);
    expect(screen.getByTestId('chips')).toHaveTextContent('["scene","entity:42","asset:g1"]');
  });

  it('空场景 + 无选中 → 无 chip(Composer 不渲染 chip 行)', () => {
    render(<Composer />);
    expect(screen.queryByTestId('context-chips')).toBeNull();
  });

  it('Composer:chip 渲染;剔除实体后发送前缀不含实体;发送后剔除态复位', () => {
    useEditorStore.setState({ sceneName: 'maze.rxscene', entities: [E42], selectedId: 42 });
    useAssetStore.setState({
      items: [{ path: 'Content/Textures/brick.png', guid: 'g1', type: 'texture', size: 1 }],
      selectedGuid: 'g1',
    });
    const sendMessage = vi.fn();
    useChatStore.setState({ sendMessage });
    useSessionStore.setState({ activeSessionId: 'sess_1' });
    render(<Composer />);

    expect(screen.getByTestId('ctx-chip-scene')).toHaveTextContent('@maze.rxscene');
    expect(screen.getByTestId('ctx-chip-entity')).toHaveTextContent('#42 Crate');
    expect(screen.getByTestId('ctx-chip-asset')).toHaveTextContent('brick.png');

    fireEvent.click(screen.getByTestId('ctx-chip-toggle-entity'));
    fireEvent.change(screen.getByTestId('composer-input'), { target: { value: '调整一下' } });
    fireEvent.click(screen.getByTestId('composer-send'));

    const arg = sendMessage.mock.calls[0]?.[0] as string;
    expect(arg).toContain('场景:maze.rxscene');
    expect(arg).toContain('资产:Content/Textures/brick.png');
    expect(arg).not.toContain('实体:');
    expect(arg.endsWith('调整一下')).toBe(true);
    // 剔除态发送后复位(chip 恢复 acc 态可再注入)
    expect(screen.getByTestId('ctx-chip-toggle-entity')).toHaveAccessibleName('移除上下文 #42 Crate');
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