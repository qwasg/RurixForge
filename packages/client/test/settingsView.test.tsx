import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import SettingsView from '@/views/SettingsView';
import { TAB_IDS } from '@/lib/forgeSettingsTabs';
import { mockForgeBackend } from './forgeMock';

/**
 * F3 wave.4 G-F3-4 设置页:骨架 + ?tab= 深链 + skills tab 真实功能。
 * skills 列表来自 /api/forge/skills/list(REST mock);禁用经 /api/forge/skills/config/write。
 */

interface SkillItem {
  name: string;
  description: string;
  enabled: boolean;
}

const SKILLS: SkillItem[] = [
  { name: 'asset-cleanup', description: '资产清理', enabled: true },
  { name: 'scene-greybox', description: '灰盒搭建', enabled: true },
  { name: 'skill-creator', description: 'skill 脚手架(seam)', enabled: true },
];

function setUrl(search: string) {
  // jsdom 同源限制:只能用相对 URL replaceState(绝对 URL 跨端口即 SecurityError)
  window.history.replaceState(null, '', search === '' ? '/' : `/${search.startsWith('?') ? search : `?${search}`}`);
}

beforeEach(() => setUrl('?tab=skills'));
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  setUrl('');
});

describe('<SettingsView />', () => {
  it('骨架:11 个 tab 菜单 + ?tab=skills 深链直达 skills 列表', async () => {
    vi.stubGlobal('fetch', mockForgeBackend({}, { '/api/forge/skills/list': { skills: SKILLS } }));

    render(<SettingsView />);

    // 左菜单 11 tab(07 §7.2 首发冻结清单)
    const navButtons = document.querySelectorAll('[data-tab-id]');
    expect(navButtons).toHaveLength(TAB_IDS.length);
    expect(TAB_IDS).toHaveLength(11);

    // ?tab=skills 深链:列表真实数据渲染
    expect(await screen.findByText('asset-cleanup')).toBeInTheDocument();
    expect(document.querySelectorAll('[data-skill-name]')).toHaveLength(3);
    // seam 标注说明在列
    expect(screen.getByText(/seam 技能正文首行有依赖标注/)).toBeInTheDocument();
  });

  it('skills 禁用:点 switch → config/write 载荷 disabled → 重拉反映 enabled=false', async () => {
    let disabled: string[] = [];
    const writeCalls: Array<Record<string, unknown>> = [];
    vi.stubGlobal(
      'fetch',
      mockForgeBackend(
        {},
        {
          '/api/forge/skills/list': () => ({
            skills: SKILLS.map((s) => ({ ...s, enabled: !disabled.includes(s.name) })),
          }),
          '/api/forge/skills/config/write': (init?: { body?: string }) => {
            const body = JSON.parse(init?.body ?? '{}') as { disabled: string[] };
            writeCalls.push(body);
            disabled = body.disabled;
            return { written: true };
          },
        },
      ),
    );

    render(<SettingsView />);
    const row = (await screen.findByText('asset-cleanup')).closest('li')!;
    const toggleBtn = row.querySelector('button[role="switch"]')!;
    expect(toggleBtn.getAttribute('aria-checked')).toBe('true');

    fireEvent.click(toggleBtn);

    await waitFor(() => expect(writeCalls).toHaveLength(1));
    expect(writeCalls[0]).toEqual({ disabled: ['asset-cleanup'] });
    // 重拉后 aria-checked 翻转为 false
    await waitFor(() =>
      expect(
        screen.getByText('asset-cleanup').closest('li')!.querySelector('button[role="switch"]')!
          .getAttribute('aria-checked'),
      ).toBe('false'),
    );
  });

  it('tab 切换:点 General → URL 深链同步 + 占位如实标注(不伪造功能)', async () => {
    vi.stubGlobal('fetch', mockForgeBackend({}, { '/api/forge/skills/list': { skills: SKILLS } }));

    render(<SettingsView />);
    fireEvent.click(screen.getByText('General'));

    expect(window.location.search).toContain('tab=general');
    expect(await screen.findByText(/此 tab 未落地/)).toBeInTheDocument();
    // 未落地 tab 不渲染 skills 列表
    expect(document.querySelectorAll('[data-skill-name]')).toHaveLength(0);
  });

  it('非法 ?tab= 值回落 DEFAULT_TAB(skills)', async () => {
    setUrl('?tab=no-such-tab');
    vi.stubGlobal('fetch', mockForgeBackend({}, { '/api/forge/skills/list': { skills: SKILLS } }));

    render(<SettingsView />);
    expect(await screen.findByText('asset-cleanup')).toBeInTheDocument();
  });
});
