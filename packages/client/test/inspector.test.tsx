import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import Inspector from '@/components/shell/Inspector';
import { useToastStore } from '@/lib/toastStore';

/**
 * F7 wave.5 Inspector 工作区树:懒加载(展开才拉子层)/本地过滤/隐藏降档/错误面 toast 如实。
 * F8 wave.1:文件点击 → 真实只读预览面板(monospace 渲染 + 413/415 等错误态如实)。
 */

const initialToast = useToastStore.getState();

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

/** 按 path query 分派的假 workspace/tree + workspace/file 后端。 */
function stubTree(
  map: Record<string, Entry[]>,
  files: Record<string, { content: string } | { status: number; code: string; message: string }> = {},
) {
  const calls: string[] = [];
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: unknown) => {
      const u = String(url);
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
    render(<Inspector />);
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
    render(<Inspector />);
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
    render(<Inspector />);
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
    render(<Inspector />);
    fireEvent.click(await screen.findByTestId('ws-file-a.bin'));
    expect((await screen.findByTestId('ws-preview-error')).textContent).toContain('415 BINARY_FILE');
    fireEvent.click(screen.getByTestId('ws-file-big.txt'));
    expect((await screen.findByTestId('ws-preview-error')).textContent).toContain('413 FILE_TOO_LARGE');
    // gone.txt 无 stub → 404
    fireEvent.click(screen.getByTestId('ws-file-gone.txt'));
    expect((await screen.findByTestId('ws-preview-error')).textContent).toContain('404 PATH_NOT_FOUND');
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
    render(<Inspector />);
    expect(await screen.findByText('工作区加载失败')).toBeInTheDocument();
    const toasts = useToastStore.getState().items;
    expect(toasts.some((t) => t.kind === 'error' && t.title.includes('工作区目录加载失败'))).toBe(true);
  });
});
