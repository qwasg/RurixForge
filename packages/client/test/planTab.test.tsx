import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import Workbench from '@/components/shell/Workbench';
import { useChatStore } from '@/lib/chatStore';
import { usePlanStore } from '@/lib/planStore';
import { useSessionStore } from '@/lib/sessionStore';
import { clearFileDrafts } from '@/lib/useFileEditor';
import { planTabId, useWorkbenchStore } from '@/lib/workbenchStore';

/**
 * D-035 Plan 页签:计划文件页(按 path 多开)。
 * 覆盖:front matter 渲染 / 待办进度按 planTodoId 映射 / 编辑-预览切换 /
 * Build 结构化下发 planPath(dirty 时先保存)/ run 进行中禁用 / plan.* 事件自动开页签。
 */

const PLAN_PATH = '.forge/plans/敌人波次系统.plan.md';
const PLAN_TEXT = `---
name: 敌人波次系统
overview: 分三波刷怪
todos:
- id: wave-config
  content: 新增 WaveConfig 组件
  status: pending
- id: spawner
  content: 写生成器脚本
  status: pending
---

# 敌人波次系统

## 现状
暂无波次概念。
`;

const initialWorkbench = useWorkbenchStore.getState();
const initialChat = useChatStore.getState();
const initialSession = useSessionStore.getState();

interface Posted {
  url: string;
  body: Record<string, unknown>;
}

/** 假后端:GET 计划文件 / PUT 落盘 / POST ask:execute;记录调用供断言。 */
function stubBackend(text = PLAN_TEXT) {
  const posts: Posted[] = [];
  const puts: Array<{ content: string }> = [];
  let disk = text;
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: unknown, init?: RequestInit) => {
      const u = String(url);
      const method = init?.method ?? 'GET';
      if (u.startsWith('/api/forge/workspace/file')) {
        if (method === 'PUT') {
          const body = JSON.parse(String(init?.body)) as { content: string };
          puts.push(body);
          disk = body.content;
          return {
            ok: true,
            status: 200,
            json: async () => ({
              path: PLAN_PATH,
              name: '敌人波次系统.plan.md',
              size: body.content.length,
              modifiedAt: `tok-${puts.length + 1}`,
            }),
          } as Response;
        }
        return {
          ok: true,
          status: 200,
          json: async () => ({
            path: PLAN_PATH,
            name: '敌人波次系统.plan.md',
            size: disk.length,
            content: disk,
            truncated: false,
            modifiedAt: 'tok-1',
          }),
        } as Response;
      }
      if (method === 'POST') {
        posts.push({ url: u, body: JSON.parse(String(init?.body ?? '{}')) });
        return { ok: true, status: 200, json: async () => ({ run: { id: 'run_1', status: 'running' } }) } as Response;
      }
      return { ok: true, status: 200, json: async () => ({}) } as Response;
    }),
  );
  return { posts, puts };
}

async function openPlanTab(): Promise<void> {
  act(() => {
    useWorkbenchStore.getState().openPlan(PLAN_PATH);
  });
  await screen.findByTestId('plan-todo-list');
}

beforeEach(() => {
  useWorkbenchStore.setState(initialWorkbench, true);
  useChatStore.setState(initialChat, true);
  useSessionStore.setState({ ...initialSession, activeSessionId: 'sess_1' }, true);
  usePlanStore.getState().reset();
  clearFileDrafts();
  globalThis.localStorage?.clear();
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe('Plan 页签', () => {
  it('渲染 front matter 名称/概述与正文,待办清单来自文件', async () => {
    stubBackend();
    render(<Workbench />);
    await openPlanTab();
    expect(screen.getByTestId('plan-tab')).toHaveTextContent('敌人波次系统');
    expect(screen.getByTestId('plan-tab')).toHaveTextContent('分三波刷怪');
    expect(screen.getByTestId('plan-body')).toHaveTextContent('暂无波次概念');
    expect(screen.getByTestId('plan-todo-wave-config')).toHaveTextContent('新增 WaveConfig 组件');
    expect(screen.getByTestId('plan-todo-list')).toHaveTextContent('2 项待办');
    // tabbar 标题回填为 front matter 真名(不是文件名)
    expect(screen.getByTestId(`workbench-tab-${planTabId(PLAN_PATH)}`)).toHaveTextContent('敌人波次系统');
  });

  it('进度按 planTodoId 映射会话待办的实时状态', async () => {
    stubBackend();
    useChatStore.setState({
      todos: [
        { id: 'todo_1', title: '新增 WaveConfig 组件', status: 'completed', planTodoId: 'wave-config' },
        { id: 'todo_2', title: '写生成器脚本', status: 'running', planTodoId: 'spawner' },
      ],
    });
    render(<Workbench />);
    await openPlanTab();
    expect(screen.getByTestId('plan-progress')).toHaveTextContent('1/2');
    expect(screen.getByTestId('plan-todo-wave-config').querySelector('.line-through')).not.toBeNull();
  });

  it('Build:POST ask:execute 带结构化 planPath 与 build 模式(不拼正文前缀)', async () => {
    const { posts } = stubBackend();
    render(<Workbench />);
    await openPlanTab();
    const btn = screen.getByTestId('plan-start-build');
    expect(btn).toBeEnabled();
    await act(async () => {
      fireEvent.click(btn);
    });
    await waitFor(() => expect(posts).toHaveLength(1));
    expect(posts[0].url).toContain('/ask:execute');
    expect(posts[0].body.mode).toBe('build');
    expect(posts[0].body.planPath).toBe(PLAN_PATH);
    expect(posts[0].body.userInput).toBe('按计划《敌人波次系统》实施');
  });

  it('run 进行中 Build 禁用(不并发开第二轮)', async () => {
    stubBackend();
    useChatStore.setState({ activeRunId: 'run_x' });
    render(<Workbench />);
    await openPlanTab();
    expect(screen.getByTestId('plan-start-build')).toBeDisabled();
  });

  it('编辑态改动 → dirty;Build 先落盘再发,PUT 内容为编辑后的全文', async () => {
    const { posts, puts } = stubBackend();
    render(<Workbench />);
    await openPlanTab();
    act(() => {
      fireEvent.click(screen.getByTestId('plan-toggle-edit'));
    });
    const host = await screen.findByTestId('plan-editor');
    const { EditorView } = await import('@codemirror/view');
    const view = EditorView.findFromDOM(host as HTMLElement);
    expect(view).not.toBeNull();
    act(() => {
      view?.dispatch({ changes: { from: view.state.doc.length, insert: '\n补一句。' } });
    });
    await waitFor(() =>
      expect(useWorkbenchStore.getState().tabs.find((t) => t.id === planTabId(PLAN_PATH))?.dirty).toBe(true),
    );
    await act(async () => {
      fireEvent.click(screen.getByTestId('plan-start-build'));
    });
    await waitFor(() => expect(posts).toHaveLength(1));
    expect(puts).toHaveLength(1);
    expect(puts[0].content).toContain('补一句。');
  });

  it('front matter 损坏:如实提示且正文照常渲染,Build 仍可用', async () => {
    stubBackend('# 没有 front matter 的计划\n\n正文在这。');
    render(<Workbench />);
    act(() => {
      useWorkbenchStore.getState().openPlan(PLAN_PATH);
    });
    await screen.findByTestId('plan-parse-error');
    expect(screen.getByTestId('plan-parse-error')).toHaveTextContent('起始');
    expect(screen.getByTestId('plan-body')).toHaveTextContent('正文在这');
    expect(screen.getByTestId('plan-start-build')).toBeEnabled();
  });

  it('文件打不开:如实报错,不伪造空计划', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => ({
        ok: false,
        status: 404,
        json: async () => ({ error: { code: 'PATH_NOT_FOUND', message: '文件不存在' } }),
      })) as unknown as typeof fetch,
    );
    render(<Workbench />);
    act(() => {
      useWorkbenchStore.getState().openPlan(PLAN_PATH);
    });
    await screen.findByTestId('plan-load-error');
    expect(screen.getByTestId('plan-load-error')).toHaveTextContent('PATH_NOT_FOUND');
    expect(screen.getByTestId('plan-start-build')).toBeDisabled();
  });
});

describe('plan.* 事件接线', () => {
  const evt = (type: string, path: string) => ({
    id: `e-${type}-${path}`,
    sessionId: 'sess_1',
    seq: 1,
    type,
    ts: '2026-09-03T10:00:00.000Z',
    payload: { path, name: 'x', todoCount: 2 },
  });

  it('plan.created:回填路径 + 自动开 Plan 页签', () => {
    stubBackend();
    act(() => {
      useChatStore.getState().applyEvent(evt('plan.created', PLAN_PATH));
    });
    expect(usePlanStore.getState().activePlanPath).toBe(PLAN_PATH);
    expect(useWorkbenchStore.getState().tabs.map((t) => t.id)).toContain(planTabId(PLAN_PATH));
    expect(useWorkbenchStore.getState().activeTabId).toBe(planTabId(PLAN_PATH));
  });

  it('plan.updated:reloadNonce 递增(已开页签据此重载)', () => {
    stubBackend();
    const before = usePlanStore.getState().reloadNonce;
    act(() => {
      useChatStore.getState().applyEvent(evt('plan.updated', PLAN_PATH));
    });
    expect(usePlanStore.getState().reloadNonce).toBe(before + 1);
  });

  it('plan 模式发送 → planning 置位;run 收束复位', async () => {
    stubBackend();
    await act(async () => {
      await useChatStore.getState().sendMessage('规划一下', 'plan');
    });
    expect(usePlanStore.getState().planning).toBe(true);
    act(() => {
      useChatStore.getState().applyEvent({
        id: 'e-done',
        sessionId: 'sess_1',
        seq: 2,
        type: 'agent.completed',
        ts: '2026-09-03T10:00:01.000Z',
        payload: { runId: 'run_1', text: '已出计划' },
      });
    });
    expect(usePlanStore.getState().planning).toBe(false);
  });

  it('todo.created 携带 planTodoId/source 时入库(Plan 页签靠它映射)', () => {
    act(() => {
      useChatStore.getState().applyEvent({
        id: 'e-todo',
        sessionId: 'sess_1',
        seq: 3,
        type: 'todo.created',
        ts: '2026-09-03T10:00:02.000Z',
        payload: {
          id: 'todo_9',
          title: '新增 WaveConfig 组件',
          status: 'queued',
          planTodoId: 'wave-config',
          source: 'plan',
        },
      });
    });
    const t = useChatStore.getState().todos.find((x) => x.id === 'todo_9');
    expect(t?.planTodoId).toBe('wave-config');
    expect(t?.source).toBe('plan');
  });
});
