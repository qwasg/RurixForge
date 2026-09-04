import { beforeEach, describe, expect, it } from 'vitest';
import { useSettingsStore } from '@/lib/settingsStore';
import { BOTTOM_CLAMP, useWorkbenchStore } from '@/lib/workbenchStore';

/**
 * F7 wave.5:settingsStore(当前页/Ctrl+Enter 持久化)+ workbenchStore 底部面板态
 * (开关/高度 clamp 120–520/激活 tab;持久化 forge:bottomPanel)。
 */

const initialSettings = useSettingsStore.getState();
const initialWorkbench = useWorkbenchStore.getState();

beforeEach(() => {
  useSettingsStore.setState(initialSettings, true);
  useWorkbenchStore.setState(initialWorkbench, true);
  globalThis.localStorage?.clear();
});

describe('settingsStore', () => {
  it('当前页切换 + 持久化 forge:settingsPage', () => {
    expect(useSettingsStore.getState().page).toBe('appearance');
    useSettingsStore.getState().setPage('models');
    expect(useSettingsStore.getState().page).toBe('models');
    expect(globalThis.localStorage?.getItem('forge:settingsPage')).toBe('models');
    useSettingsStore.getState().setPage('skills');
    expect(globalThis.localStorage?.getItem('forge:settingsPage')).toBe('skills');
  });

  it('Ctrl+Enter 发送开关 + 持久化 forge:submitCtrl', () => {
    expect(useSettingsStore.getState().submitCtrlEnter).toBe(false);
    useSettingsStore.getState().setSubmitCtrlEnter(true);
    expect(useSettingsStore.getState().submitCtrlEnter).toBe(true);
    expect(globalThis.localStorage?.getItem('forge:submitCtrl')).toBe('1');
    useSettingsStore.getState().setSubmitCtrlEnter(false);
    expect(globalThis.localStorage?.getItem('forge:submitCtrl')).toBe('0');
  });
});

describe('workbenchStore 底部面板', () => {
  it('toggleBottom 开关 + 持久化', () => {
    expect(useWorkbenchStore.getState().bottomOpen).toBe(false);
    useWorkbenchStore.getState().toggleBottom();
    expect(useWorkbenchStore.getState().bottomOpen).toBe(true);
    const raw = globalThis.localStorage?.getItem('forge:bottomPanel');
    expect(raw).toBeTruthy();
    expect(JSON.parse(raw as string).open).toBe(true);
  });

  it('setBottomH clamp 120–520 + 持久化', () => {
    useWorkbenchStore.getState().setBottomH(9999);
    expect(useWorkbenchStore.getState().bottomH).toBe(BOTTOM_CLAMP.max);
    useWorkbenchStore.getState().setBottomH(1);
    expect(useWorkbenchStore.getState().bottomH).toBe(BOTTOM_CLAMP.min);
    useWorkbenchStore.getState().setBottomH(300);
    expect(useWorkbenchStore.getState().bottomH).toBe(300);
    expect(JSON.parse(globalThis.localStorage?.getItem('forge:bottomPanel') as string).h).toBe(300);
  });

  it('setBottomTab 切换 + 持久化', () => {
    useWorkbenchStore.getState().setBottomTab('output');
    expect(useWorkbenchStore.getState().bottomTab).toBe('output');
    expect(JSON.parse(globalThis.localStorage?.getItem('forge:bottomPanel') as string).tab).toBe('output');
  });

  it('openTab 内建 tab 单例语义(重开=激活)', () => {
    const st = useWorkbenchStore.getState();
    st.openTab('proposals');
    st.openTab('todo');
    expect(useWorkbenchStore.getState().tabs.map((t) => t.id)).toEqual(['proposals', 'todo']);
    expect(useWorkbenchStore.getState().activeTabId).toBe('todo');
    // 重开 proposals = 激活而非重复
    useWorkbenchStore.getState().openTab('proposals');
    expect(useWorkbenchStore.getState().tabs.length).toBe(2);
    expect(useWorkbenchStore.getState().activeTabId).toBe('proposals');
  });

  it('openFile 按 path 单例:重开=激活;关闭后个人工作区空态', () => {
    const st = useWorkbenchStore.getState();
    st.openFile('07_FRONTEND_IDE.md');
    st.openFile('crates/foo.rs');
    expect(useWorkbenchStore.getState().tabs.map((t) => t.id)).toEqual([
      'file:07_FRONTEND_IDE.md',
      'file:crates/foo.rs',
    ]);
    expect(useWorkbenchStore.getState().activeTabId).toBe('file:crates/foo.rs');
    useWorkbenchStore.getState().openFile('07_FRONTEND_IDE.md');
    expect(useWorkbenchStore.getState().tabs.length).toBe(2);
    expect(useWorkbenchStore.getState().activeTabId).toBe('file:07_FRONTEND_IDE.md');
    useWorkbenchStore.getState().closeTab('file:07_FRONTEND_IDE.md');
    useWorkbenchStore.getState().closeTab('file:crates/foo.rs');
    expect(useWorkbenchStore.getState().tabs).toEqual([]);
    expect(useWorkbenchStore.getState().activeTabId).toBeNull();
  });
});
