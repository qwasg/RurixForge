import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import StatusBar from '@/components/shell/StatusBar';
import { useChatStore } from '@/lib/chatStore';
import { useGitStore } from '@/lib/gitStore';
import { useOverlayStore } from '@/lib/overlayStore';
import { useSessionStore } from '@/lib/sessionStore';
import { useSettingsStore } from '@/lib/settingsStore';
import { useSystemStore } from '@/lib/systemStore';
import { useWorkbenchStore } from '@/lib/workbenchStore';
import { useWorkspaceStore } from '@/lib/workspaceStore';
import { mockForgeBackend } from './forgeMock';

/** D-040 状态栏:检测中首态、左右分组、每段直达;数据来自 systemStore / gitStore 统一轮询。 */

const initial = {
  system: useSystemStore.getState(),
  git: useGitStore.getState(),
  chat: useChatStore.getState(),
  sessions: useSessionStore.getState(),
  workbench: useWorkbenchStore.getState(),
  overlay: useOverlayStore.getState(),
  workspace: useWorkspaceStore.getState(),
};

function stubBackend(opts: { git?: unknown; models?: unknown[]; defaultModelId?: string } = {}) {
  vi.stubGlobal(
    'fetch',
    mockForgeBackend({}, {
      '/api/forge/health': {
        status: 'ok',
        version: '0.1.0',
        uptimeSec: 90,
        agentd: { ok: true, version: '0.1.0', uptimeSec: 60 },
      },
      '/api/forge/design-snapshot': {
        sessions: [],
        project: { name: 'Code Sentinels', mode: '2d' },
        models: {
          models: opts.models ?? [
            { id: 'openai-compat', label: 'openai-compatible(未配置)', provider: 'openai-compat', availability: 'needs-key' },
            { id: 'mock', label: 'Mock provider', provider: 'mock', availability: 'available' },
          ],
          defaultModelId: opts.defaultModelId ?? 'openai-compat',
        },
        agents: { defaultEngine: 'local', engines: [] },
        todos: [],
      },
      '/api/forge/workspace/git': opts.git ?? {
        isRepo: true,
        branch: 'main',
        upstream: 'origin/main',
        ahead: 2,
        behind: 0,
        rootUntracked: false,
        files: [{ path: 'a.ts', status: 'M', staged: false, dir: false, insertions: 5, deletions: 2, origPath: null }],
        counts: { modified: 1, added: 0, deleted: 0, renamed: 0, untracked: 0, conflicted: 0 },
        insertions: 5,
        deletions: 2,
        total: 1,
        truncated: false,
      },
    }),
  );
}

beforeEach(() => {
  useSystemStore.setState(initial.system, true);
  useGitStore.setState(initial.git, true);
  useChatStore.setState(initial.chat, true);
  useSessionStore.setState(initial.sessions, true);
  useWorkbenchStore.setState(initial.workbench, true);
  useOverlayStore.setState(initial.overlay, true);
  useWorkspaceStore.setState(initial.workspace, true);
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe('<StatusBar /> D-040', () => {
  it('首帧「检测中」,轮询后显示已连接 + 项目 + 分支(ahead + 增删行)', async () => {
    stubBackend();
    render(<StatusBar />);
    expect(screen.getByTestId('statusbar-host')).toHaveTextContent('检测中');
    expect(await screen.findByText('已连接')).toBeInTheDocument();
    expect(screen.getByTestId('statusbar-project-mode')).toHaveTextContent('Code Sentinels · 2D');
    const git = await screen.findByTestId('statusbar-git');
    expect(git).toHaveTextContent('main');
    expect(git).toHaveTextContent('↑2');
    expect(git).toHaveTextContent('+5');
    expect(git).toHaveTextContent('−2');
    expect(screen.getByTestId('statusbar-host').getAttribute('title')).toContain('agentd v0.1.0');
  });

  it('默认模型缺密钥 → 「模型未配置」,点击打开设置 · 模型', async () => {
    stubBackend();
    render(<StatusBar />);
    const provider = await screen.findByText('模型未配置');
    expect(provider).toHaveAttribute('data-state', 'unconfigured');
    fireEvent.click(screen.getByTestId('statusbar-model'));
    expect(useOverlayStore.getState().settings).toBe(true);
    expect(useSettingsStore.getState().page).toBe('models');
  });

  it('分支段点击:只看改动 + 右栏文件树(主页态先进工作台)', async () => {
    stubBackend();
    render(<StatusBar />);
    fireEvent.click(await screen.findByTestId('statusbar-git'));
    expect(useGitStore.getState().changesOnly).toBe(true);
    const wb = useWorkbenchStore.getState();
    expect(wb.homeDismissed).toBe(true);
    expect(wb.rightTab).toBe('files');
    expect(wb.collapsed.inspector).toBe(false);
  });

  it('工作区目录未被跟踪 → 分支后标「未跟踪」;非仓库 → 不渲染分支段', async () => {
    stubBackend({ git: { isRepo: true, branch: 'main', rootUntracked: true, files: [], total: 0 } });
    render(<StatusBar />);
    expect(await screen.findByText('· 未跟踪')).toBeInTheDocument();
    cleanup();
    useGitStore.setState(initial.git, true);
    stubBackend({ git: { isRepo: false, reason: 'not a git repository' } });
    render(<StatusBar />);
    await screen.findByText('已连接');
    expect(screen.queryByTestId('statusbar-git')).toBeNull();
  });

  it('项目段点击:展开会话栏并打开工作区选择器', async () => {
    stubBackend();
    useWorkbenchStore.setState({ collapsed: { sessions: true, chat: false, inspector: false } });
    render(<StatusBar />);
    fireEvent.click(await screen.findByTestId('statusbar-project-mode'));
    expect(useWorkspaceStore.getState().pickerOpen).toBe(true);
    expect(useWorkbenchStore.getState().collapsed.sessions).toBe(false);
  });
});
