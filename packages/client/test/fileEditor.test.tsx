import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { EditorView } from '@codemirror/view';
import Workbench from '@/components/shell/Workbench';
import FilePreviewTab, {
  clearFileDrafts,
  restoreEol,
  saveErrorLabel,
  sniffEol,
} from '@/components/workbench/FilePreviewTab';
import { langIdForPath } from '@/lib/cmLang';
import { ForgeApiError } from '@/lib/forgeApi';
import { useWorkbenchStore } from '@/lib/workbenchStore';

/**
 * F9 文件编辑器(CodeMirror 6):语言分派/EOL 保真/编辑→dirty→PUT 落盘/
 * 409 FILE_CONFLICT 如实 + 重新加载恢复/dirty 关闭拦截(保存/放弃/取消)。
 * 编辑经 EditorView.findFromDOM 拿真实视图 dispatch(jsdom 下 CM 渲染真实 DOM,
 * 仅布局测量为 setup.ts 空几何桩)。
 */

const initialWorkbench = useWorkbenchStore.getState();

interface PutCall {
  body: { path: string; content: string; baseModifiedAt: string };
}

/** 假 workspace/file 后端:GET 固定内容;PUT 记录 body,可注入错误。 */
function stubFileBackend(opts: {
  content: string;
  modifiedAt?: string;
  getError?: { status: number; code: string; message: string };
  putError?: { status: number; code: string; message: string };
}) {
  const puts: PutCall[] = [];
  const gets: string[] = [];
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: unknown, init?: RequestInit) => {
      const u = String(url);
      if (!u.startsWith('/api/forge/workspace/file')) {
        return { ok: true, status: 200, json: async () => ({}) } as Response;
      }
      if (init?.method === 'PUT') {
        const body = JSON.parse(String(init.body)) as PutCall['body'];
        puts.push({ body });
        if (opts.putError) {
          return {
            ok: false,
            status: opts.putError.status,
            json: async () => ({ error: { code: opts.putError?.code, message: opts.putError?.message } }),
          } as Response;
        }
        return {
          ok: true,
          status: 200,
          json: async () => ({
            path: body.path,
            name: body.path.split('/').pop() ?? body.path,
            size: body.content.length,
            modifiedAt: `tok-${puts.length + 1}`,
          }),
        } as Response;
      }
      const m = /[?&]path=([^&]*)/.exec(u);
      const path = decodeURIComponent(m?.[1] ?? '');
      gets.push(path);
      if (opts.getError) {
        return {
          ok: false,
          status: opts.getError.status,
          json: async () => ({ error: { code: opts.getError?.code, message: opts.getError?.message } }),
        } as Response;
      }
      return {
        ok: true,
        status: 200,
        json: async () => ({
          path,
          name: path.split('/').pop() ?? path,
          size: opts.content.length,
          content: opts.content,
          truncated: false,
          modifiedAt: opts.modifiedAt ?? 'tok-1',
        }),
      } as Response;
    }),
  );
  return { puts, gets };
}

/** 打开文件 tab 并等编辑器挂载,返回真实 CM 视图。 */
async function openEditor(path: string): Promise<EditorView> {
  act(() => {
    useWorkbenchStore.getState().openFile(path);
  });
  const host = await screen.findByTestId('ws-preview-content');
  const view = EditorView.findFromDOM(host as HTMLElement);
  expect(view).not.toBeNull();
  return view as EditorView;
}

function typeAtStart(view: EditorView, text: string): void {
  act(() => {
    view.dispatch({ changes: { from: 0, insert: text } });
  });
}

beforeEach(() => {
  useWorkbenchStore.setState(initialWorkbench, true);
  clearFileDrafts();
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe('cmLang.langIdForPath 扩展名分派', () => {
  it('ts/tsx/js→javascript;rs/rx→rust;json/rx 资产面→json;md/ps1/toml/yaml/sh 各归位;未知→null', () => {
    expect(langIdForPath('src/App.tsx')).toBe('javascript');
    expect(langIdForPath('src/lib/a.ts')).toBe('javascript');
    expect(langIdForPath('x.js')).toBe('javascript');
    expect(langIdForPath('crates/agentd/src/main.rs')).toBe('rust');
    expect(langIdForPath('Content/Scripts/maze.rx')).toBe('rust');
    expect(langIdForPath('package.json')).toBe('json');
    expect(langIdForPath('Content/Scenes/journey.rxscene')).toBe('json');
    expect(langIdForPath('Content/Graphs/door.rxgraph')).toBe('json');
    expect(langIdForPath('Content/Materials/Box.rxmat')).toBe('json');
    expect(langIdForPath('Content/Materials/Box.rxmat.meta')).toBe('json');
    expect(langIdForPath('README.md')).toBe('markdown');
    expect(langIdForPath('scripts/f0-stack-smoke.ps1')).toBe('powershell');
    expect(langIdForPath('Cargo.toml')).toBe('toml');
    expect(langIdForPath('a.yml')).toBe('yaml');
    expect(langIdForPath('run.sh')).toBe('shell');
    expect(langIdForPath('LICENSE')).toBeNull();
    expect(langIdForPath('bin/engine-host.exe')).toBeNull();
    // 点开头隐藏文件不误判整名为扩展名
    expect(langIdForPath('.gitignore')).toBeNull();
  });
});

describe('EOL 保真(sniffEol/restoreEol)', () => {
  it('CRLF 嗅探→还原;LF 保持;混合 EOL 保存后统一(如实)', () => {
    expect(sniffEol('a\r\nb')).toBe('\r\n');
    expect(sniffEol('a\nb')).toBe('\n');
    expect(sniffEol('无换行')).toBe('\n');
    expect(restoreEol('a\nb\nc', '\r\n')).toBe('a\r\nb\r\nc');
    expect(restoreEol('a\nb', '\n')).toBe('a\nb');
    // 混合:含 \r\n 即按 CRLF 嗅探,还原后统一为 CRLF
    const mixed = 'a\r\nb\nc';
    const lf = mixed.replace(/\r\n/g, '\n');
    expect(restoreEol(lf, sniffEol(mixed))).toBe('a\r\nb\r\nc');
  });

  it('saveErrorLabel:FILE_CONFLICT/FORGE_IO 如实,默认前缀为保存失败', () => {
    expect(saveErrorLabel(new ForgeApiError('FILE_CONFLICT', '外部已改', 409))).toContain('409 FILE_CONFLICT');
    expect(saveErrorLabel(new ForgeApiError('FORGE_IO', '磁盘写失败', 500))).toContain('500 FORGE_IO');
    expect(saveErrorLabel(new ForgeApiError('WEIRD', 'x', 418))).toContain('保存失败(418 WEIRD)');
  });
});

describe('文件编辑器:编辑→dirty→保存(PUT)', () => {
  it('编辑出 dirty(tabbar 圆点+保存钮);保存 PUT body 还原 CRLF + baseModifiedAt;成功后已保存', async () => {
    const { puts } = stubFileBackend({ content: 'line1\r\nline2', modifiedAt: 'tok-1' });
    render(<Workbench />);
    const view = await openEditor('Content/a.txt');
    // CM 内部 LF 归一渲染
    expect(screen.getByTestId('ws-preview-content')).toHaveTextContent('line1');
    expect(screen.getByTestId('ws-save-state').textContent).toContain('已保存');
    expect(screen.queryByTestId('tab-dirty-file:Content/a.txt')).not.toBeInTheDocument();

    typeAtStart(view, 'AB');
    expect(screen.getByTestId('ws-save-state').textContent).toContain('未保存');
    expect(screen.getByTestId('tab-dirty-file:Content/a.txt')).toBeInTheDocument();

    fireEvent.click(screen.getByTestId('ws-save-btn'));
    await waitFor(() => {
      expect(screen.getByTestId('ws-save-state').textContent).toContain('已保存');
    });
    expect(puts).toHaveLength(1);
    // EOL 还原:CM 内部 LF,落盘回 CRLF;乐观并发基线原样回传
    expect(puts[0].body).toMatchObject({
      path: 'Content/a.txt',
      content: 'ABline1\r\nline2',
      baseModifiedAt: 'tok-1',
    });
    expect(screen.queryByTestId('tab-dirty-file:Content/a.txt')).not.toBeInTheDocument();
  });

  it('409 FILE_CONFLICT 如实:保存失败条 + 重新加载放弃草稿回磁盘内容', async () => {
    const stub = stubFileBackend({
      content: 'disk-v2',
      modifiedAt: 'tok-disk',
      putError: { status: 409, code: 'FILE_CONFLICT', message: '文件已被外部修改,请刷新后重试(实: Content/b.txt)' },
    });
    render(<Workbench />);
    const view = await openEditor('Content/b.txt');
    typeAtStart(view, 'MINE-');
    fireEvent.click(screen.getByTestId('ws-save-btn'));
    const err = await screen.findByTestId('ws-save-error');
    expect(err.textContent).toContain('409 FILE_CONFLICT');
    expect(screen.getByTestId('ws-save-state').textContent).toContain('保存失败');
    // 重新加载 = 放弃本地草稿,重挂编辑器取磁盘现状
    fireEvent.click(screen.getByTestId('ws-reload'));
    await waitFor(() => {
      expect(screen.queryByTestId('ws-save-error')).not.toBeInTheDocument();
    });
    await waitFor(() => {
      expect(screen.getByTestId('ws-save-state').textContent).toContain('已保存');
    });
    expect(screen.getByTestId('ws-preview-content')).toHaveTextContent('disk-v2');
    expect(screen.getByTestId('ws-preview-content')).not.toHaveTextContent('MINE-');
    expect(stub.gets.length).toBe(2);
  });

  it('加载失败(413 FILE_TOO_LARGE)不挂编辑器:错误面如实,无 ws-preview-content', async () => {
    stubFileBackend({
      content: '',
      getError: { status: 413, code: 'FILE_TOO_LARGE', message: 'path 须为根内 ≤256KB 文本文件(实: big.txt)' },
    });
    render(<Workbench />);
    act(() => {
      useWorkbenchStore.getState().openFile('big.txt');
    });
    expect((await screen.findByTestId('ws-preview-error')).textContent).toContain('413 FILE_TOO_LARGE');
    expect(screen.queryByTestId('ws-preview-content')).not.toBeInTheDocument();
    expect(screen.queryByTestId('ws-save-state')).not.toBeInTheDocument();
  });
});

describe('文件编辑器:dirty 关闭拦截', () => {
  it('dirty tab 关闭 → 内联确认条;取消留下;放弃并关闭丢草稿关 tab', async () => {
    stubFileBackend({ content: 'keep', modifiedAt: 'tok-1' });
    render(<Workbench />);
    const view = await openEditor('Content/c.txt');
    typeAtStart(view, 'draft-');
    // tabbar 关闭钮 → dirty 拦截(不直接关)
    fireEvent.click(screen.getByLabelText('关闭 c.txt'));
    expect(screen.getByTestId('workbench-tab-file:Content/c.txt')).toBeInTheDocument();
    const bar = await screen.findByTestId('ws-close-confirm');
    expect(bar.textContent).toContain('未保存');
    // 取消:tab 留、确认条收
    fireEvent.click(screen.getByTestId('ws-close-cancel'));
    expect(screen.queryByTestId('ws-close-confirm')).not.toBeInTheDocument();
    expect(screen.getByTestId('workbench-tab-file:Content/c.txt')).toBeInTheDocument();
    // 再关 → 放弃并关闭:tab 消失
    fireEvent.click(screen.getByLabelText('关闭 c.txt'));
    fireEvent.click(await screen.findByTestId('ws-close-discard'));
    expect(screen.queryByTestId('workbench-tab-file:Content/c.txt')).not.toBeInTheDocument();
    expect(useWorkbenchStore.getState().pendingCloseTabId).toBeNull();
  });

  it('保存并关闭:PUT 落盘成功后 tab 关闭', async () => {
    const { puts } = stubFileBackend({ content: 'v1', modifiedAt: 'tok-1' });
    render(<Workbench />);
    const view = await openEditor('Content/d.txt');
    typeAtStart(view, 'v2-');
    fireEvent.click(screen.getByLabelText('关闭 d.txt'));
    fireEvent.click(await screen.findByTestId('ws-close-save'));
    await waitFor(() => {
      expect(screen.queryByTestId('workbench-tab-file:Content/d.txt')).not.toBeInTheDocument();
    });
    expect(puts).toHaveLength(1);
    expect(puts[0].body.content).toBe('v2-v1');
  });

  it('干净 tab 关闭不拦截(原语义)', async () => {
    stubFileBackend({ content: 'clean', modifiedAt: 'tok-1' });
    render(<Workbench />);
    await openEditor('Content/e.txt');
    fireEvent.click(screen.getByLabelText('关闭 e.txt'));
    expect(screen.queryByTestId('workbench-tab-file:Content/e.txt')).not.toBeInTheDocument();
    expect(screen.queryByTestId('ws-close-confirm')).not.toBeInTheDocument();
  });
});

describe('文件编辑器:草稿跨 tab 切换暂存', () => {
  it('切走再切回:未保存草稿还原,dirty 保持', async () => {
    stubFileBackend({ content: 'base', modifiedAt: 'tok-1' });
    render(<Workbench />);
    const view = await openEditor('Content/f.txt');
    typeAtStart(view, 'draft-');
    expect(screen.getByTestId('tab-dirty-file:Content/f.txt')).toBeInTheDocument();
    // 切到别的 tab(文件编辑器卸载)
    act(() => {
      useWorkbenchStore.getState().openTab('todo');
    });
    expect(screen.queryByTestId('ws-preview-content')).not.toBeInTheDocument();
    // dirty 圆点在 tabbar 上保持
    expect(screen.getByTestId('tab-dirty-file:Content/f.txt')).toBeInTheDocument();
    // 切回:草稿还原
    act(() => {
      useWorkbenchStore.getState().activateTab('file:Content/f.txt');
    });
    const host = await screen.findByTestId('ws-preview-content');
    await waitFor(() => {
      expect(host).toHaveTextContent('draft-base');
    });
    expect(screen.getByTestId('ws-save-state').textContent).toContain('未保存');
  });
});

describe('FilePreviewTab 直挂(不经 Workbench)', () => {
  it('渲染 monospace 宿主(font-code)且真实文档内容进 DOM', async () => {
    stubFileBackend({ content: '# 标题\n正文', modifiedAt: 'tok-1' });
    render(<FilePreviewTab path="README.md" tabId="file:README.md" />);
    const host = await screen.findByTestId('ws-preview-content');
    expect(host.className).toContain('font-code');
    await waitFor(() => {
      expect(host).toHaveTextContent('# 标题');
      expect(host).toHaveTextContent('正文');
    });
  });
});
