import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import StudioBoardView from '@/components/studio/StudioBoardView';
import { loadStudio, TEXT_TEMPLATES, useStudioStore, studioStorageKey, type StudioVersion } from '@/lib/studioStore';
import { useToastStore } from '@/lib/toastStore';
import { mockForgeBackend } from './forgeMock';

/**
 * 素材创作右键快捷键系统(素材创作波;与画板三套菜单同一 BoardContextMenu 外壳):
 * 主画布 画布/节点卡/连线标签 三套菜单的互斥与落点(port 归卡、标签优先)、
 * 「在此新建」落在右键处、菜单动作落 store、disabled 规则(生成/入库/复制产物)、
 * 等效键盘快捷键(数字键/↵/F2/G/Ctrl+D/Del/Esc)、输入框上右键让位原生菜单;
 * 详情画布 空白/中心卡/版本卡/上游卡 四套菜单与 Esc 返回。
 */

const initialStudio = useStudioStore.getState();
const studio = () => useStudioStore.getState();

beforeEach(() => {
  globalThis.localStorage.clear();
  useStudioStore.setState(
    {
      ...initialStudio,
      ...loadStudio(),
      openNodeId: null,
      selectedNodeId: null,
      pendingRenameId: null,
      pendingEdgeId: null,
      busyIds: [],
      lastError: null,
    },
    true,
  );
  useToastStore.setState({ items: [] });
  // 详情画布挂载即拉生成后端清单(StudioComposer);其余请求未 mock 即抛,保持测试诚实
  vi.stubGlobal(
    'fetch',
    mockForgeBackend(
      {},
      {
        '/api/forge/gen/backends': { backends: [] },
        '/api/forge/workspaces': { workspaces: [] },
      },
    ),
  );
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

/** jsdom 下视口 rect 全 0、缩放 1,client 坐标即世界坐标 */
function rightClick(el: Element, x = 120, y = 80): void {
  fireEvent.contextMenu(el, { clientX: x, clientY: y });
}

/** jsdom 无 PointerEvent 构造器,用同名类型的 MouseEvent 承载(与画板测试同法) */
function pointer(type: string, init: MouseEventInit = {}): MouseEvent {
  return new MouseEvent(type, { bubbles: true, cancelable: true, ...init });
}

/** 手工塞一版媒体产物并置为当前(生成管线不在本测试面;会话内即可,不落盘) */
function injectVersion(nodeId: string, v: StudioVersion): void {
  useStudioStore.setState((s) => ({
    nodes: s.nodes.map((n) =>
      n.id === nodeId ? { ...n, versions: [...n.versions, v], currentVersionId: v.id } : n,
    ),
  }));
}

describe('<StudioBoardView /> — 画布 / 节点 / 连线三套右键菜单', () => {
  it('空白右键 = 画布菜单,卡身右键 = 节点菜单(顺带选中),两者不会同时出', () => {
    const id = studio().addNode('outline')!;
    const { container } = render(<StudioBoardView />);

    rightClick(screen.getByTestId('studio-canvas'));
    expect(screen.getByTestId('studio-menu-canvas')).toHaveTextContent('在此新建大纲');
    expect(screen.queryByTestId('studio-menu-node')).not.toBeInTheDocument();
    expect(studio().selectedNodeId).toBeNull(); // 空白右键顺带清选中

    rightClick(container.querySelector(`[data-studio-node="${id}"]`)!);
    const nodeMenu = screen.getByTestId('studio-menu-node');
    expect(nodeMenu).toHaveTextContent('大纲 1'); // 标题行认到具体节点
    expect(nodeMenu).toHaveTextContent('打开创作画布');
    expect(screen.queryByTestId('studio-menu-canvas')).not.toBeInTheDocument();
    expect(studio().selectedNodeId).toBe(id);
  });

  it('连线 port 右键走节点菜单(不误弹画布菜单);线上标签右键 = 连线菜单', () => {
    const a = studio().addNode('outline')!;
    const b = studio().addNode('concept')!;
    const eid = studio().addEdge(a, b)!;
    studio().clearPendingEdge(); // 新边默认进编辑态,这里要按钮态的标签
    const { container } = render(<StudioBoardView />);

    rightClick(container.querySelector(`[data-studio-port="${a}"]`)!);
    expect(screen.getByTestId('studio-menu-node')).toHaveTextContent('大纲 1');
    expect(screen.queryByTestId('studio-menu-canvas')).not.toBeInTheDocument();

    rightClick(container.querySelector(`[data-studio-edge-label="${eid}"]`)!);
    const menu = screen.getByTestId('studio-menu-edge');
    expect(menu).toHaveTextContent('打开来源「大纲 1」');
    expect(menu).toHaveTextContent('打开目标「原画 1」');
    expect(screen.queryByTestId('studio-menu-node')).not.toBeInTheDocument();
  });

  it('连线路径与箭头使用已定义的主题强调色变量', () => {
    const a = studio().addNode('outline')!;
    const b = studio().addNode('concept')!;
    const eid = studio().addEdge(a, b)!;
    const { container } = render(<StudioBoardView />);

    expect(container.querySelector(`[data-studio-edge="${eid}"]`)).toHaveAttribute(
      'stroke',
      'var(--accent)',
    );
    expect(container.querySelector('#studio-arrow path')).toHaveAttribute('fill', 'var(--accent)');
  });

  it('画布菜单「在此新建」把卡放在右键落点,数字键则按网格落位', () => {
    render(<StudioBoardView />);
    rightClick(screen.getByTestId('studio-canvas'), 260, 140);
    fireEvent.click(screen.getByTestId('studio-menu-canvas-add-concept'));
    expect(studio().nodes).toHaveLength(1);
    expect(studio().nodes[0]).toMatchObject({ preset: 'concept', pos: [260, 140] });

    fireEvent.keyDown(screen.getByTestId('studio-canvas'), { key: '1' });
    expect(studio().nodes.map((n) => n.preset)).toEqual(['concept', 'outline']);
    expect(studio().nodes[1].pos).not.toEqual([260, 140]);
  });

  it('节点菜单:改名 / 复制 / 删除都落到右键那张卡上;复制带描述参数、不带版本历史', () => {
    const id = studio().addNode('outline')!;
    studio().setPrompt(id, '写一个横版动作游戏大纲');
    studio().setParam(id, 'template', 'script');
    studio().writeText(id, '第一章…');
    const { container } = render(<StudioBoardView />);
    const card = () => container.querySelector(`[data-studio-node="${id}"]`)!;

    rightClick(card());
    fireEvent.click(screen.getByTestId('studio-menu-node-rename'));
    expect(screen.getByTestId(`studio-node-name-input-${id}`)).toBeInTheDocument();

    rightClick(card());
    fireEvent.click(screen.getByTestId('studio-menu-node-duplicate'));
    expect(studio().nodes).toHaveLength(2);
    const copy = studio().nodes[1];
    expect(copy.name).toBe('大纲 1 副本');
    expect(copy.prompt).toBe('写一个横版动作游戏大纲');
    expect(copy.params.template).toBe('script');
    expect(copy.versions).toHaveLength(0); // 产物是生成结果,复制不造假历史
    expect(copy.currentVersionId).toBeNull();
    expect(studio().edges).toHaveLength(0); // 连线也不复制

    rightClick(card());
    fireEvent.click(screen.getByTestId('studio-menu-node-remove'));
    expect(studio().nodes.map((n) => n.id)).not.toContain(id);
  });

  it('生成项按 prompt / busy 置灰;text 节点入库恒置灰,复制产物按有无置灰', () => {
    const id = studio().addNode('outline')!;
    const { container } = render(<StudioBoardView />);
    const card = () => container.querySelector(`[data-studio-node="${id}"]`)!;

    rightClick(card());
    expect(screen.getByTestId('studio-menu-node-generate')).toBeDisabled(); // 还没写描述
    expect(screen.getByTestId('studio-menu-node-accept')).toBeDisabled(); // text 不可入库
    expect(screen.getByTestId('studio-menu-node-copy')).toBeDisabled(); // 没有产物
    fireEvent.keyDown(document, { key: 'Escape' });

    act(() => {
      studio().setPrompt(id, '写一个大纲');
      studio().writeText(id, '第一章…');
    });
    rightClick(card());
    expect(screen.getByTestId('studio-menu-node-generate')).not.toBeDisabled();
    expect(screen.getByTestId('studio-menu-node-copy')).not.toBeDisabled();
    fireEvent.keyDown(document, { key: 'Escape' });

    act(() => useStudioStore.setState({ busyIds: [id] }));
    rightClick(card());
    expect(screen.getByTestId('studio-menu-node-generate')).toBeDisabled();
    expect(screen.getByTestId('studio-menu-node-generate')).toHaveTextContent('生成中…');
  });

  it('图像节点:未入库产物可入库,guid 回写后置灰;复制产物路径走剪贴板并给回执', async () => {
    const id = studio().addNode('concept')!;
    injectVersion(id, {
      id: 'v-img',
      createdAt: 1,
      backendId: 'mock',
      prompt: 'rock',
      fileRef: '.forge/tmp/gen/rock.png',
    });
    const { container } = render(<StudioBoardView />);
    const card = () => container.querySelector(`[data-studio-node="${id}"]`)!;

    rightClick(card());
    expect(screen.getByTestId('studio-menu-node-accept')).not.toBeDisabled();

    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(globalThis.navigator, 'clipboard', {
      value: { writeText },
      configurable: true,
    });
    fireEvent.click(screen.getByTestId('studio-menu-node-copy'));
    await vi.waitFor(() => expect(writeText).toHaveBeenCalledWith('.forge/tmp/gen/rock.png'));
    expect(useToastStore.getState().items.map((t) => t.title)).toContain('已复制产物路径');
    Reflect.deleteProperty(globalThis.navigator, 'clipboard');

    act(() =>
      useStudioStore.setState((s) => ({
        nodes: s.nodes.map((n) =>
          n.id === id
            ? {
                ...n,
                versions: n.versions.map((v) => ({ ...v, assetPath: 'Concepts/rock.png', guid: 'g1' })),
              }
            : n,
        ),
      })),
    );
    rightClick(card());
    expect(screen.getByTestId('studio-menu-node-accept')).toBeDisabled();
  });

  it('连线菜单:编辑引用说明进入行内编辑,跳转两端,删除连线', () => {
    const a = studio().addNode('outline')!;
    const b = studio().addNode('concept')!;
    const eid = studio().addEdge(a, b)!;
    studio().clearPendingEdge();
    const { container } = render(<StudioBoardView />);
    const label = () => container.querySelector(`[data-studio-edge-label="${eid}"]`)!;

    rightClick(label());
    fireEvent.click(screen.getByTestId('studio-menu-edge-edit'));
    expect(screen.getByTestId(`studio-edge-input-${eid}`)).toBeInTheDocument();

    rightClick(label());
    fireEvent.click(screen.getByTestId('studio-menu-edge-open-to'));
    expect(studio().openNodeId).toBe(b); // 详情画布整面接管
    act(() => studio().closeNode());

    rightClick(label());
    fireEvent.click(screen.getByTestId('studio-menu-edge-remove'));
    expect(studio().edges).toHaveLength(0);
  });

  it('菜单项标注等效快捷键;Esc 与点外部都能关掉;输入框上右键不接管', () => {
    const id = studio().addNode('outline')!;
    const { container } = render(<StudioBoardView />);

    rightClick(container.querySelector(`[data-studio-node="${id}"]`)!);
    expect(screen.getByTestId('studio-menu-node-open')).toHaveTextContent('↵');
    expect(screen.getByTestId('studio-menu-node-duplicate')).toHaveTextContent('Ctrl+D');
    expect(screen.getByTestId('studio-menu-node-remove')).toHaveTextContent('Del');
    fireEvent.keyDown(document, { key: 'Escape' });
    expect(screen.queryByTestId('studio-menu-node')).not.toBeInTheDocument();

    rightClick(screen.getByTestId('studio-canvas'));
    expect(screen.getByTestId('studio-menu-canvas-fit')).toHaveTextContent('F');
    fireEvent.mouseDown(document.body);
    expect(screen.queryByTestId('studio-menu-canvas')).not.toBeInTheDocument();

    // 改名输入框上右键让位原生菜单(要能复制粘贴),不弹节点菜单
    fireEvent.click(screen.getByTestId(`studio-node-name-${id}`));
    rightClick(screen.getByTestId(`studio-node-name-input-${id}`));
    expect(screen.queryByTestId('studio-menu-node')).not.toBeInTheDocument();
  });
});

describe('<StudioBoardView /> — 画布快捷键', () => {
  it('数字键按类型放节点;0 / F 视口键不落节点', () => {
    render(<StudioBoardView />);
    const canvas = screen.getByTestId('studio-canvas');

    fireEvent.keyDown(canvas, { key: '1' });
    fireEvent.keyDown(canvas, { key: '6' });
    expect(studio().nodes.map((n) => n.preset)).toEqual(['outline', 'mesh']);

    fireEvent.keyDown(canvas, { key: '0' });
    fireEvent.keyDown(canvas, { key: 'f' });
    expect(studio().nodes).toHaveLength(2);
  });

  it('选中节点后 F2 改名 / Del 删除;Esc 取消选中后不再作用,卡框着重色随选中切换', () => {
    const id = studio().addNode('outline')!;
    const { container } = render(<StudioBoardView />);
    const canvas = screen.getByTestId('studio-canvas');
    const card = container.querySelector(`[data-studio-node="${id}"]`)!;

    fireEvent(card, pointer('pointerdown', { button: 0 }));
    expect(studio().selectedNodeId).toBe(id);
    expect(card.hasAttribute('data-studio-node-selected')).toBe(true);

    fireEvent.keyDown(canvas, { key: 'F2' });
    expect(screen.getByTestId(`studio-node-name-input-${id}`)).toBeInTheDocument();

    fireEvent.keyDown(canvas, { key: 'Escape' });
    expect(studio().selectedNodeId).toBeNull();
    expect(card.hasAttribute('data-studio-node-selected')).toBe(false);
    fireEvent.keyDown(canvas, { key: 'Delete' });
    expect(studio().nodes).toHaveLength(1); // 没有作用对象就不删

    fireEvent(card, pointer('pointerdown', { button: 0 }));
    fireEvent.keyDown(canvas, { key: 'Delete' });
    expect(studio().nodes).toHaveLength(0);
  });

  it('↵ 打开选中节点的创作画布(详情画布整面接管)', () => {
    const id = studio().addNode('outline')!;
    const { container } = render(<StudioBoardView />);
    fireEvent(
      container.querySelector(`[data-studio-node="${id}"]`)!,
      pointer('pointerdown', { button: 0 }),
    );
    fireEvent.keyDown(screen.getByTestId('studio-canvas'), { key: 'Enter' });
    expect(studio().openNodeId).toBe(id);
    expect(screen.getByTestId('studio-detail-canvas')).toBeInTheDocument();
  });

  it('G 对选中节点直接跑生成(text 通道),产出新版本', async () => {
    vi.stubGlobal(
      'fetch',
      mockForgeBackend(
        {},
        {
          '/api/forge/gen/backends': { backends: [] },
          '/api/forge/workspaces': { workspaces: [] },
          '/api/forge/studio/sessions': { session: { id: 'sess_studio_1' } },
          '/api/forge/sessions/sess_studio_1/ask:execute': {
            message: { text: '第一章:出发' },
            run: { id: 'run_1', status: 'completed' },
            mode: 'build',
          },
        },
      ),
    );
    const id = studio().addNode('outline')!;
    studio().setPrompt(id, '写一个大纲');
    const { container } = render(<StudioBoardView />);
    fireEvent(
      container.querySelector(`[data-studio-node="${id}"]`)!,
      pointer('pointerdown', { button: 0 }),
    );
    fireEvent.keyDown(screen.getByTestId('studio-canvas'), { key: 'g' });
    await vi.waitFor(() => expect(studio().nodes[0].versions).toHaveLength(1));
    expect(studio().nodes[0].versions[0].text).toBe('第一章:出发');
    expect(studio().nodes[0].versions[0].backendId).toBe('studio:run_1');
    expect(studio().nodes[0].versions[0].runId).toBe('run_1');
  });

  it('Ctrl+D 复制选中节点', () => {
    const id = studio().addNode('concept')!;
    const { container } = render(<StudioBoardView />);
    fireEvent(
      container.querySelector(`[data-studio-node="${id}"]`)!,
      pointer('pointerdown', { button: 0 }),
    );
    fireEvent.keyDown(screen.getByTestId('studio-canvas'), { key: 'd', ctrlKey: true });
    expect(studio().nodes).toHaveLength(2);
    expect(studio().nodes[1].name).toBe('原画 1 副本');
  });

  it('卡上输入框里打字不当快捷键(键归输入框)', () => {
    const id = studio().addNode('outline')!;
    const { container } = render(<StudioBoardView />);
    fireEvent(
      container.querySelector(`[data-studio-node="${id}"]`)!,
      pointer('pointerdown', { button: 0 }),
    ); // 选中:若快捷键误触,Delete 会删掉它
    fireEvent.click(screen.getByTestId(`studio-node-name-${id}`));
    fireEvent.keyDown(screen.getByTestId(`studio-node-name-input-${id}`), { key: 'Delete' });
    expect(studio().nodes).toHaveLength(1);
  });
});

describe('<StudioBoardView /> — 详情画布四套右键菜单与快捷键', () => {
  /** 建一个带上游与两版产物的 text 节点并下钻 */
  function setupDetail() {
    const up = studio().addNode('outline')!;
    const id = studio().addNode('mapdraft')!;
    const eid = studio().addEdge(up, id)!;
    studio().clearPendingEdge();
    studio().setEdgeLabel(eid, '按大纲画地图');
    studio().writeText(up, '世界观:…');
    studio().writeText(id, '版本一');
    studio().writeText(id, '版本二');
    studio().openNode(id);
    return { up, id, eid };
  }

  it('空白 / 中心卡 / 版本卡 / 上游卡四套菜单互斥;prompt 为空时生成项置灰', () => {
    const { up, id } = setupDetail();
    const { container } = render(<StudioBoardView />);
    const vid = studio().nodes.find((n) => n.id === id)!.versions[0].id;

    rightClick(screen.getByTestId('studio-detail-canvas'));
    expect(screen.getByTestId('studio-detail-menu-canvas')).toHaveTextContent('返回素材创作主画布');
    expect(screen.getByTestId('studio-detail-menu-canvas-generate')).toBeDisabled();

    rightClick(container.querySelector('[data-studio-center]')!);
    expect(screen.getByTestId('studio-detail-menu-center')).toHaveTextContent('地图草稿 1');
    expect(screen.queryByTestId('studio-detail-menu-canvas')).not.toBeInTheDocument();

    rightClick(container.querySelector(`[data-studio-version="${vid}"]`)!);
    expect(screen.getByTestId('studio-detail-menu-version')).toHaveTextContent('历史版本');
    expect(screen.queryByTestId('studio-detail-menu-center')).not.toBeInTheDocument();

    rightClick(container.querySelector(`[data-studio-upstream="${up}"]`)!);
    expect(screen.getByTestId('studio-detail-menu-upstream')).toHaveTextContent('大纲 1');
    expect(screen.queryByTestId('studio-detail-menu-version')).not.toBeInTheDocument();
  });

  it('版本卡:设为当前 / 删除该版本;当前版本上「设为当前」置灰', () => {
    const { id } = setupDetail();
    const { container } = render(<StudioBoardView />);
    const node = () => studio().nodes.find((n) => n.id === id)!;
    const [v1, v2] = node().versions;
    expect(node().currentVersionId).toBe(v2.id);

    rightClick(container.querySelector(`[data-studio-version="${v2.id}"]`)!);
    expect(screen.getByTestId('studio-detail-menu-version-current')).toBeDisabled();
    fireEvent.keyDown(document, { key: 'Escape' });

    rightClick(container.querySelector(`[data-studio-version="${v1.id}"]`)!);
    fireEvent.click(screen.getByTestId('studio-detail-menu-version-current'));
    expect(node().currentVersionId).toBe(v1.id);

    rightClick(container.querySelector(`[data-studio-version="${v2.id}"]`)!);
    fireEvent.click(screen.getByTestId('studio-detail-menu-version-remove'));
    expect(node().versions.map((v) => v.id)).toEqual([v1.id]);
  });

  it('上游卡:打开上游节点 / 断开引用', () => {
    const { up, id } = setupDetail();
    const { container } = render(<StudioBoardView />);

    rightClick(container.querySelector(`[data-studio-upstream="${up}"]`)!);
    fireEvent.click(screen.getByTestId('studio-detail-menu-upstream-open'));
    expect(studio().openNodeId).toBe(up);

    act(() => studio().openNode(id)); // 回到本节点
    rightClick(container.querySelector(`[data-studio-upstream="${up}"]`)!);
    fireEvent.click(screen.getByTestId('studio-detail-menu-upstream-detach'));
    expect(studio().edges).toHaveLength(0);
  });

  it('中心卡:删除当前版本回落上一版,重命名进顶栏输入框;Esc 快捷键返回主画布', () => {
    const { id } = setupDetail();
    const { container } = render(<StudioBoardView />);
    const node = () => studio().nodes.find((n) => n.id === id)!;

    rightClick(container.querySelector('[data-studio-center]')!);
    fireEvent.click(screen.getByTestId('studio-detail-menu-center-remove-version'));
    expect(node().versions).toHaveLength(1);
    expect(currentText(node().versions, node().currentVersionId)).toBe('版本一');

    rightClick(container.querySelector('[data-studio-center]')!);
    fireEvent.click(screen.getByTestId('studio-detail-menu-center-rename'));
    expect(screen.getByTestId('studio-detail-name-input')).toBeInTheDocument();

    fireEvent.keyDown(screen.getByTestId('studio-detail-canvas'), { key: 'Escape' });
    expect(studio().openNodeId).toBeNull();
    expect(screen.getByTestId('studio-canvas')).toBeInTheDocument();
  });

  it('生成输入条上右键让位原生菜单(要能复制粘贴)', () => {
    setupDetail();
    const { container } = render(<StudioBoardView />);
    rightClick(container.querySelector('[data-studio-composer]')!);
    expect(screen.queryByTestId('studio-detail-menu-canvas')).not.toBeInTheDocument();
  });
});

function currentText(versions: StudioVersion[], currentId: string | null): string {
  return versions.find((v) => v.id === currentId)?.text ?? '';
}

describe('素材创作 Agent 工具循环', () => {
  it('文本模板不再禁止调用工具,也不再走 /llm/chat', () => {
    for (const t of TEXT_TEMPLATES) {
      expect(t.prefix).not.toContain('不要调用工具');
    }
    expect(TEXT_TEMPLATES.some((t) => t.prefix.includes('检索'))).toBe(true);
  });

  it('文本生成走隐藏会话 + ask:execute,版本记下 runId', async () => {
    const calls: string[] = [];
    vi.stubGlobal(
      'fetch',
      mockForgeBackend(
        {},
        {
          '/api/forge/gen/backends': { backends: [] },
          '/api/forge/workspaces': { workspaces: [] },
          '/api/forge/studio/sessions': (init?: { body?: string }) => {
            calls.push(`studio:${init?.body ?? ''}`);
            return { session: { id: 'sess_studio_x' } };
          },
          '/api/forge/sessions/sess_studio_x/ask:execute': (init?: { body?: string }) => {
            calls.push(`ask:${init?.body ?? ''}`);
            return { message: { text: '地图草稿' }, run: { id: 'run_x', status: 'completed' }, mode: 'build' };
          },
        },
      ),
    );
    const id = studio().addNode('mapdraft')!;
    studio().setPrompt(id, '一座岛');
    await studio().generate(id);
    expect(calls.some((c) => c.startsWith('studio:'))).toBe(true);
    expect(calls.some((c) => c.includes('ask:execute') || c.startsWith('ask:'))).toBe(true);
    expect(calls.join('\n')).not.toContain('/llm/chat');
    const ask = calls.find((c) => c.startsWith('ask:'));
    expect(ask).toContain('readonlyWorkspaceIds');
    expect(ask).toContain('"mode":"build"');
    expect(studio().nodes[0].versions[0]).toMatchObject({
      text: '地图草稿',
      runId: 'run_x',
    });
  });

  it('mock/无密钥返回 LLM_KEY_REQUIRED 时不把假文本存成版本', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: unknown) => {
        const u = String(url);
        if (u.includes('/gen/backends')) {
          return { ok: true, status: 200, json: async () => ({ backends: [] }), text: async () => '{}' } as Response;
        }
        if (u.includes('/workspaces')) {
          return { ok: true, status: 200, json: async () => ({ workspaces: [] }), text: async () => '{}' } as Response;
        }
        if (u.includes('/studio/sessions')) {
          return {
            ok: true,
            status: 200,
            json: async () => ({ session: { id: 'sess_s' } }),
            text: async () => '{}',
          } as Response;
        }
        if (u.includes('ask:execute')) {
          return {
            ok: false,
            status: 400,
            json: async () => ({
              error: { code: 'LLM_KEY_REQUIRED', message: '请配置密钥' },
            }),
            text: async () => '{}',
          } as Response;
        }
        return { ok: true, status: 200, json: async () => ({}), text: async () => '{}' } as Response;
      }),
    );
    const id = studio().addNode('outline')!;
    studio().setPrompt(id, '大纲');
    await studio().generate(id);
    expect(studio().nodes[0].versions).toHaveLength(0);
    expect(studio().lastError?.code).toBe('LLM_KEY_REQUIRED');
  });

  it('画板按工作区分区,旧 key 迁入当前空间', () => {
    globalThis.localStorage.setItem(
      'forge:studioBoard',
      JSON.stringify({
        version: 1,
        seq: 3,
        nodes: [
          {
            id: 's1',
            preset: 'outline',
            name: '大纲 1',
            pos: [0, 0],
            prompt: '旧',
            params: { template: 'free' },
            versions: [],
            currentVersionId: null,
          },
        ],
        edges: [],
      }),
    );
    const loaded = loadStudio(null);
    expect(loaded.nodes).toHaveLength(1);
    expect(loaded.nodes[0].prompt).toBe('旧');
    expect(globalThis.localStorage.getItem(studioStorageKey(null))).toBeTruthy();
    expect(globalThis.localStorage.getItem('forge:studioBoard')).toBeNull();
  });

  it('详情输入条提供资源范围、审批与取消控件', () => {
    const id = studio().addNode('outline')!;
    studio().setPrompt(id, '写大纲');
    studio().openNode(id);
    useStudioStore.setState({
      busyIds: [id],
      nodeRuns: {
        [id]: { sessionId: 's', runId: 'r1', phase: 'pending-approval', tools: [{ name: 'mcp__store__store_install', status: 'running' }] },
      },
      pendingPermission: {
        nodeId: id,
        id: 'perm_1',
        tool: 'mcp__store__store_install',
        targetProjectId: 'ws_a',
        argsSummary: '{}',
      },
    });
    render(<StudioBoardView />);
    expect(screen.getByTestId('studio-scope-current')).toBeInTheDocument();
    expect(screen.getByTestId('studio-scope-library')).toBeInTheDocument();
    expect(screen.getByTestId('studio-run-status')).toHaveTextContent('待审批');
    expect(screen.getByTestId('studio-perm-approve')).toBeInTheDocument();
    expect(screen.getByTestId('studio-perm-deny')).toBeInTheDocument();
    expect(screen.getByTestId('studio-cancel')).toBeInTheDocument();
    expect(screen.queryByTestId('studio-send')).not.toBeInTheDocument();
  });

  it('SSE 工具事件写入版本审计字段', async () => {
    const frames =
      'event: agent.started\ndata: {"payload":{"runId":"run_sse"}}\n\n' +
      'event: agent.tool.invoked\ndata: {"payload":{"name":"resource_search"}}\n\n' +
      'event: agent.tool.completed\ndata: {"payload":{"name":"resource_search","output":"hit forge://project/ws_a/doc/1"}}\n\n';
    const enc = new TextEncoder();
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: unknown) => {
        const u = String(url);
        if (u.includes('/events/stream')) {
          return {
            ok: true,
            status: 200,
            body: new ReadableStream({
              start(c) {
                c.enqueue(enc.encode(frames));
                c.close();
              },
            }),
            json: async () => ({}),
            text: async () => '',
          } as Response;
        }
        if (u.includes('/workspaces')) {
          return { ok: true, status: 200, json: async () => ({ workspaces: [] }), text: async () => '{}' } as Response;
        }
        if (u.includes('/gen/backends')) {
          return { ok: true, status: 200, json: async () => ({ backends: [] }), text: async () => '{}' } as Response;
        }
        if (u.includes('/studio/sessions')) {
          return {
            ok: true,
            status: 200,
            json: async () => ({ session: { id: 'sess_sse' } }),
            text: async () => '{}',
          } as Response;
        }
        if (u.includes('ask:execute')) {
          await new Promise((r) => setTimeout(r, 30));
          return {
            ok: true,
            status: 200,
            json: async () => ({
              message: { text: '地图草稿' },
              run: { id: 'run_sse', status: 'completed' },
              mode: 'build',
            }),
            text: async () => '{}',
          } as Response;
        }
        return { ok: true, status: 200, json: async () => ({}), text: async () => '{}' } as Response;
      }),
    );
    const id = studio().addNode('mapdraft')!;
    studio().setPrompt(id, '一座岛');
    await studio().generate(id);
    const v = studio().nodes[0].versions[0];
    expect(v.runId).toBe('run_sse');
    expect(v.toolCalls?.some((t) => t.name === 'resource_search' && t.status === 'ok')).toBe(true);
    expect(v.resourceRefs).toContain('forge://project/ws_a/doc/1');
  });

  it('画板按工作区隔离,取消与审批打到对应接口', async () => {
    const calls: string[] = [];
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: unknown) => {
        const u = String(url);
        calls.push(u);
        return { ok: true, status: 200, json: async () => ({}), text: async () => '{}' } as Response;
      }),
    );
    const id = studio().addNode('outline')!;
    studio().setPrompt(id, 'A项目大纲');
    studio().setReadonlyWorkspaceIds(['ws_b']);
    studio().bindWorkspace('ws_b');
    expect(studio().nodes).toHaveLength(0);
    expect(studio().readonlyWorkspaceIds).toEqual([]);
    studio().bindWorkspace(null);
    expect(studio().nodes[0].prompt).toBe('A项目大纲');
    expect(studio().readonlyWorkspaceIds).toEqual(['ws_b']);

    useStudioStore.setState({
      nodeRuns: { [id]: { sessionId: 'sess_x', runId: 'run_x', phase: 'running', tools: [] } },
      pendingPermission: {
        nodeId: id,
        id: 'perm_1',
        tool: 'mcp__store__store_install',
        targetProjectId: 'ws_a',
      },
    });
    await studio().cancelGenerate(id);
    expect(calls.some((c) => c.includes('/api/forge/runs/run_x/cancel'))).toBe(true);
    expect(studio().nodeRuns[id].phase).toBe('failed');

    useStudioStore.setState({
      pendingPermission: {
        nodeId: id,
        id: 'perm_1',
        tool: 'mcp__store__store_install',
        targetProjectId: 'ws_a',
      },
    });
    await studio().resolvePermission(true);
    expect(calls.some((c) => c.includes('/api/forge/permissions/perm_1/approve'))).toBe(true);
  });
});
