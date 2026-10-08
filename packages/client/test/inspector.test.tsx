import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import Inspector from '@/components/shell/Inspector';
import Workbench from '@/components/shell/Workbench';
import { useGitStore } from '@/lib/gitStore';
import { useThemeStore } from '@/lib/themeStore';
import { useToastStore } from '@/lib/toastStore';
import { useWorkbenchStore } from '@/lib/workbenchStore';

/**
 * F7 wave.5 Inspector 工作区树:懒加载(展开才拉子层)/本地过滤/隐藏降档/错误面 toast 如实。
 * F8 wave.1:文件点击 → 个人工作区只读预览 tab(monospace 渲染 + 413/415 等错误态如实)。
 */

const initialToast = useToastStore.getState();
const initialWorkbench = useWorkbenchStore.getState();
const initialGit = useGitStore.getState();

function renderWorkspace() {
  return render(
    <>
      <Inspector />
      <Workbench />
    </>,
  );
}

interface Entry {
  name: string;
  kind: 'dir' | 'file';
  relPath: string;
  size: number;
  modifiedAt: string;
  hidden: boolean;
}

function entry(name: string, kind: 'dir' | 'file', relPath: string, hidden = false): Entry {
  return { name, kind, relPath, size: kind === 'file' ? 10 : 0, modifiedAt: '2026-08-18T10:00:00Z', hidden };
}

/** 按 path query 分派的假 workspace/tree + workspace/file 后端(D-040:git 状态请求单独应答,不计入 calls)。 */
function stubTree(
  map: Record<string, Entry[]>,
  files: Record<string, { content: string } | { status: number; code: string; message: string }> = {},
  git: unknown = { isRepo: false, reason: 'not a git repository' },
) {
  const calls: string[] = [];
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: unknown) => {
      const u = String(url);
      if (u.startsWith('/api/forge/workspace/git')) {
        return { ok: true, status: 200, json: async () => git } as Response;
      }
      const m = /[?&]path=([^&]*)/.exec(u);
      const path = decodeURIComponent(m?.[1] ?? '');
      calls.push(path);
      if (u.startsWith('/api/forge/workspace/file')) {
        const f = files[path];
        if (f === undefined) {
          return {
            ok: false,
            status: 404,
            json: async () => ({ error: { code: 'PATH_NOT_FOUND', message: `不存在: ${path}` } }),
          } as Response;
        }
        if ('status' in f) {
          return {
            ok: false,
            status: f.status,
            json: async () => ({ error: { code: f.code, message: f.message } }),
          } as Response;
        }
        return {
          ok: true,
          status: 200,
          json: async () => ({
            path,
            name: path.split('/').pop() ?? path,
            size: f.content.length,
            content: f.content,
            truncated: false,
            // F9:真实端点含纳秒级 mtime 令牌(编辑器乐观并发基线),stub 同形。
            modifiedAt: '2026-08-18T10:00:00.000000000Z',
          }),
        } as Response;
      }
      if (!(path in map)) {
        return {
          ok: false,
          status: 404,
          json: async () => ({ error: { code: 'PATH_NOT_FOUND', message: `不存在: ${path}` } }),
        } as Response;
      }
      return {
        ok: true,
        status: 200,
        json: async () => ({ path, entries: map[path], total: map[path].length, truncated: false }),
      } as Response;
    }),
  );
  return calls;
}

beforeEach(() => {
  useToastStore.setState(initialToast, true);
  useWorkbenchStore.setState(initialWorkbench, true);
  useGitStore.setState(initialGit, true);
  useThemeStore.getState().setDiffMarkers('color');
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe('Inspector 工作区树', () => {
  it('首挂拉根;懒加载:点目录才拉子层(chevron 展开)', async () => {
    const calls = stubTree({
      '': [entry('crates', 'dir', 'crates'), entry('README.md', 'file', 'README.md')],
      crates: [entry('forge-agentd', 'dir', 'crates/forge-agentd'), entry('gend', 'dir', 'crates/gend')],
      'crates/gend': [entry('Cargo.toml', 'file', 'crates/gend/Cargo.toml')],
    });
    renderWorkspace();
    // 根条目
    expect(await screen.findByTestId('ws-dir-crates')).toBeInTheDocument();
    expect(screen.getByTestId('ws-file-README.md')).toBeInTheDocument();
    expect(calls).toEqual(['']); // 仅根被拉
    // 展开 crates → 拉子层
    fireEvent.click(screen.getByTestId('ws-dir-crates'));
    expect(await screen.findByTestId('ws-dir-crates/gend')).toBeInTheDocument();
    expect(calls).toEqual(['', 'crates']);
    // 再展开 gend → 三层
    fireEvent.click(screen.getByTestId('ws-dir-crates/gend'));
    expect(await screen.findByTestId('ws-file-crates/gend/Cargo.toml')).toBeInTheDocument();
    expect(calls).toEqual(['', 'crates', 'crates/gend']);
    // 折叠不重复拉(缓存)
    fireEvent.click(screen.getByTestId('ws-dir-crates'));
    fireEvent.click(screen.getByTestId('ws-dir-crates'));
    expect(calls.length).toBe(3);
  });

  it('本地过滤:name 子串命中;无匹配 → 空态文案', async () => {
    stubTree({
      '': [entry('crates', 'dir', 'crates'), entry('README.md', 'file', 'README.md'), entry('Cargo.toml', 'file', 'Cargo.toml')],
    });
    renderWorkspace();
    await screen.findByTestId('ws-dir-crates');
    fireEvent.change(screen.getByTestId('ws-search'), { target: { value: 'cargo' } });
    expect(screen.queryByTestId('ws-file-README.md')).not.toBeInTheDocument();
    expect(screen.getByTestId('ws-file-Cargo.toml')).toBeInTheDocument();
    fireEvent.change(screen.getByTestId('ws-search'), { target: { value: 'zzzz' } });
    expect(screen.getByText('无匹配结果')).toBeInTheDocument();
  });

  it('隐藏文件 text_3 降档;文件点击 → 只读预览面板渲染内容(monospace)', async () => {
    stubTree(
      {
        '': [entry('.gitignore', 'file', '.gitignore', true), entry('README.md', 'file', 'README.md')],
      },
      { 'README.md': { content: '# 标题\n正文第二行' } },
    );
    renderWorkspace();
    const hidden = await screen.findByTestId('ws-file-.gitignore');
    expect(hidden.className).toContain('text-fg-3');
    expect(screen.getByTestId('ws-file-README.md').className).toContain('text-fg');
    // 点击文件 → 预览面板真实渲染(不再是 toast 占位)
    fireEvent.click(screen.getByTestId('ws-file-README.md'));
    expect(await screen.findByTestId('ws-preview')).toBeInTheDocument();
    expect(screen.getByTestId('ws-preview-name')).toHaveTextContent('README.md');
    const content = screen.getByTestId('ws-preview-content');
    expect(content).toHaveTextContent('# 标题');
    expect(content).toHaveTextContent('正文第二行');
    expect(content.className).toContain('font-code');
    // 关闭预览
    fireEvent.click(screen.getByTestId('ws-preview-close'));
    expect(screen.queryByTestId('ws-preview')).not.toBeInTheDocument();
  });

  it('预览错误态如实:415 BINARY_FILE / 413 FILE_TOO_LARGE / 404 PATH_NOT_FOUND', async () => {
    stubTree(
      { '': [entry('a.bin', 'file', 'a.bin'), entry('big.txt', 'file', 'big.txt'), entry('gone.txt', 'file', 'gone.txt')] },
      {
        'a.bin': { status: 415, code: 'BINARY_FILE', message: 'path 须为根内 ≤256KB 文本文件(实: a.bin)' },
        'big.txt': { status: 413, code: 'FILE_TOO_LARGE', message: 'path 须为根内 ≤256KB 文本文件(实: big.txt)' },
      },
    );
    renderWorkspace();
    fireEvent.click(await screen.findByTestId('ws-file-a.bin'));
    expect((await screen.findByTestId('ws-preview-error')).textContent).toContain('415 BINARY_FILE');
    fireEvent.click(screen.getByTestId('ws-file-big.txt'));
    expect((await screen.findByTestId('ws-preview-error')).textContent).toContain('413 FILE_TOO_LARGE');
    // gone.txt 无 stub → 404
    fireEvent.click(screen.getByTestId('ws-file-gone.txt'));
    expect((await screen.findByTestId('ws-preview-error')).textContent).toContain('404 PATH_NOT_FOUND');
  });

  it('D-040:文件类型图标 + git 改动标记(字母/目录点);+/- 模式显示增删行', async () => {
    stubTree(
      {
        '': [entry('src', 'dir', 'src'), entry('README.md', 'file', 'README.md'), entry('a.ts', 'file', 'a.ts')],
      },
      {},
      {
        isRepo: true,
        branch: 'main',
        rootUntracked: false,
        files: [
          { path: 'a.ts', status: 'M', staged: false, dir: false, insertions: 4, deletions: 1, origPath: null },
          { path: 'src/new.ts', status: 'U', staged: false, dir: false, insertions: null, deletions: null, origPath: null },
        ],
        total: 2,
      },
    );
    renderWorkspace();
    const a = await screen.findByTestId('ws-file-a.ts');
    await vi.waitFor(() => expect(a).toHaveAttribute('data-git', 'M'));
    expect(a).toHaveTextContent('M');
    expect(screen.getByTestId('ws-file-README.md')).not.toHaveAttribute('data-git');
    expect(screen.getByTestId('ws-dir-src').querySelector('[data-testid="ws-dir-dirty"]')).not.toBeNull();
    act(() => useThemeStore.getState().setDiffMarkers('plusminus'));
    expect(screen.getByTestId('ws-file-a.ts')).toHaveTextContent('+4');
    expect(screen.getByTestId('ws-file-a.ts')).toHaveTextContent('−1');
  });

  it('D-040:「仅看改动」平铺改动列表,点开文件;删除项不可打开', async () => {
    stubTree(
      { '': [entry('a.ts', 'file', 'a.ts')] },
      { 'a.ts': { content: 'x' } },
      {
        isRepo: true,
        branch: 'dev',
        rootUntracked: false,
        insertions: 4,
        deletions: 3,
        files: [
          { path: 'a.ts', status: 'M', staged: false, dir: false, insertions: 4, deletions: 1, origPath: null },
          { path: 'old/gone.ts', status: 'D', staged: false, dir: false, insertions: 0, deletions: 2, origPath: null },
        ],
        total: 2,
      },
    );
    renderWorkspace();
    const toggle = screen.getByTestId('ws-changes-toggle');
    await vi.waitFor(() => expect(toggle).not.toBeDisabled());
    fireEvent.click(toggle);
    const list = screen.getByTestId('ws-changes');
    expect(list).toHaveTextContent('dev');
    expect(list).toHaveTextContent('2 个改动');
    expect(screen.getByTestId('ws-change-old/gone.ts')).toBeDisabled();
    fireEvent.click(screen.getByTestId('ws-change-a.ts'));
    expect(useWorkbenchStore.getState().activeTabId).toBe('file:a.ts');
  });

  it('D-040:工作区目录整体未跟踪如实提示;非仓库时改动开关禁用', async () => {
    stubTree({ '': [entry('a.ts', 'file', 'a.ts')] }, {}, { isRepo: true, branch: 'main', rootUntracked: true, files: [], total: 0 });
    renderWorkspace();
    await vi.waitFor(() => expect(screen.getByTestId('ws-changes-toggle')).not.toBeDisabled());
    fireEvent.click(screen.getByTestId('ws-changes-toggle'));
    expect(screen.getByTestId('ws-changes')).toHaveTextContent('整体未被 git 跟踪');
    cleanup();
    useGitStore.setState(initialGit, true);
    stubTree({ '': [entry('a.ts', 'file', 'a.ts')] });
    renderWorkspace();
    await screen.findByTestId('ws-file-a.ts');
    expect(screen.getByTestId('ws-changes-toggle')).toBeDisabled();
  });

  it('错误面(如 confined 400/不存在 404):toast 如实 + 空态「工作区加载失败」', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => ({
        ok: false,
        status: 400,
        json: async () => ({ error: { code: 'PATH_OUTSIDE_ROOT', message: 'path 须为根内已存在目录' } }),
      }) as Response),
    );
    renderWorkspace();
    expect(await screen.findByText('工作区加载失败')).toBeInTheDocument();
    const toasts = useToastStore.getState().items;
    expect(toasts.some((t) => t.kind === 'error' && t.title.includes('工作区目录加载失败'))).toBe(true);
  });
});
