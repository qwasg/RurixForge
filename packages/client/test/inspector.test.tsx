import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import Inspector from '@/components/shell/Inspector';
import { useToastStore } from '@/lib/toastStore';

/**
 * F7 wave.5 Inspector 工作区树:懒加载(展开才拉子层)/本地过滤/隐藏降档/
 * 文件点击 toast/错误面(confined 400 等)toast 如实。
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

/** 按 path query 分派的假 workspace/tree 后端。 */
function stubTree(map: Record<string, Entry[]>) {
  const calls: string[] = [];
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: unknown) => {
      const u = String(url);
      const m = /[?&]path=([^&]*)/.exec(u);
      const path = decodeURIComponent(m?.[1] ?? '');
      calls.push(path);
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

  it('隐藏文件 text_3 降档;文件点击 → toast「文件预览留待后续」', async () => {
    stubTree({
      '': [entry('.gitignore', 'file', '.gitignore', true), entry('README.md', 'file', 'README.md')],
    });
    render(<Inspector />);
    const hidden = await screen.findByTestId('ws-file-.gitignore');
    expect(hidden.className).toContain('text-fg-3');
    expect(screen.getByTestId('ws-file-README.md').className).toContain('text-fg');
    fireEvent.click(hidden);
    expect(useToastStore.getState().items.some((t) => t.title === '文件预览留待后续')).toBe(true);
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
