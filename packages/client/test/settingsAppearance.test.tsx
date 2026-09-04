import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import AppearancePage from '@/components/settings/AppearancePage';
import { useThemeStore } from '@/lib/themeStore';
import { useToastStore } from '@/lib/toastStore';

/**
 * F7 wave.5 外观页交互(G-F7-5 旗舰面):
 * hex 非法拒绝(toast + 不应用)/合法即改即存即预览(CSS 变量实测);
 * 对比度 slider;字号 stepper clamp(11–18/10–20);预设整块替换;模式三卡;半透明 toggle。
 */

const initialTheme = useThemeStore.getState();
const initialToast = useToastStore.getState();

beforeEach(() => {
  useThemeStore.setState(initialTheme, true);
  useToastStore.setState(initialToast, true);
  globalThis.localStorage?.clear();
  document.documentElement.removeAttribute('data-theme');
  document.documentElement.style.cssText = '';
  // 钉亮表(断言确定性)
  useThemeStore.getState().setMode('light');
});

afterEach(() => {
  cleanup();
});

describe('外观页 · hex 颜色行', () => {
  it('非法 hex:toast 报错 + 不应用 + 输入回退', () => {
    render(<AppearancePage />);
    const before = useThemeStore.getState().light.accent;
    const input = screen.getByTestId('hex-input-light-accent');
    fireEvent.change(input, { target: { value: 'ZZZZZZ' } });
    fireEvent.blur(input);
    expect(useThemeStore.getState().light.accent).toBe(before);
    const toasts = useToastStore.getState().items;
    expect(toasts.some((t) => t.kind === 'error' && t.title.includes('非法颜色值'))).toBe(true);
    expect((input as HTMLInputElement).value).toBe(before);
  });

  it('合法 hex:即改即存(localStorage)+ 即预览(--accent CSS 变量实测)', () => {
    render(<AppearancePage />);
    const input = screen.getByTestId('hex-input-light-accent');
    fireEvent.change(input, { target: { value: '#112233' } });
    fireEvent.blur(input);
    expect(useThemeStore.getState().light.accent).toBe('#112233');
    // 即预览:亮表激活 → --accent 实改
    expect(document.documentElement.style.getPropertyValue('--accent')).toBe('#112233');
    // 即存:forge:appearance 含新值
    const raw = globalThis.localStorage?.getItem('forge:appearance');
    expect(raw).toBeTruthy();
    expect(JSON.parse(raw as string).light.accent).toBe('#112233');
    // 无报错 toast
    expect(useToastStore.getState().items.every((t) => t.kind !== 'error')).toBe(true);
  });
});

describe('外观页 · 控件族', () => {
  it('对比度 slider:0–100 + 读数 + 即存', () => {
    render(<AppearancePage />);
    const range = screen.getByTestId('contrast-light-range');
    fireEvent.change(range, { target: { value: '80' } });
    expect(useThemeStore.getState().light.contrast).toBe(80);
    expect(screen.getByTestId('contrast-light-value')).toHaveTextContent('80');
    expect(JSON.parse(globalThis.localStorage?.getItem('forge:appearance') as string).light.contrast).toBe(80);
  });

  it('UI 字号 stepper clamp 11–18', () => {
    render(<AppearancePage />);
    const minus = screen.getByTestId('ui-size-minus');
    const plus = screen.getByTestId('ui-size-plus');
    for (let i = 0; i < 30; i += 1) fireEvent.click(plus);
    expect(useThemeStore.getState().uiSize).toBe(18);
    expect(screen.getByTestId('ui-size-value')).toHaveTextContent('18');
    for (let i = 0; i < 30; i += 1) fireEvent.click(minus);
    expect(useThemeStore.getState().uiSize).toBe(11);
  });

  it('代码字号 stepper clamp 10–20', () => {
    render(<AppearancePage />);
    const plus = screen.getByTestId('code-size-plus');
    const minus = screen.getByTestId('code-size-minus');
    for (let i = 0; i < 30; i += 1) fireEvent.click(plus);
    expect(useThemeStore.getState().codeSize).toBe(20);
    for (let i = 0; i < 30; i += 1) fireEvent.click(minus);
    expect(useThemeStore.getState().codeSize).toBe(10);
  });

  it('预设选择:github 整块双表替换 + 即存', () => {
    render(<AppearancePage />);
    fireEvent.click(screen.getByTestId('preset-select-trigger'));
    fireEvent.click(screen.getByTestId('preset-select-item-github'));
    const st = useThemeStore.getState();
    expect(st.presetId).toBe('github');
    expect(st.light.accent).toBe('#0969DA');
    expect(st.dark.accent).toBe('#4493F8');
    expect(JSON.parse(globalThis.localStorage?.getItem('forge:appearance') as string).presetId).toBe('github');
  });

  it('模式三卡:深色卡 → data-theme=dark + --accent 暗表实测', () => {
    render(<AppearancePage />);
    fireEvent.click(screen.getByTestId('theme-mode-dark'));
    expect(useThemeStore.getState().mode).toBe('dark');
    expect(document.documentElement.dataset.theme).toBe('dark');
    expect(document.documentElement.style.getPropertyValue('--accent')).toBe('#F47B33');
  });

  it('半透明侧边栏 toggle:双 alpha 0xCC 实测(sidebar 变量带 alpha)', () => {
    render(<AppearancePage />);
    const before = document.documentElement.style.getPropertyValue('--bg-sidebar');
    expect(before.endsWith('CC')).toBe(false); // forge 默认实体
    fireEvent.click(screen.getByTestId('translucent-light'));
    expect(useThemeStore.getState().light.translucentSidebar).toBe(true);
    expect(document.documentElement.style.getPropertyValue('--bg-sidebar')).toMatch(/CC$/);
  });
});
