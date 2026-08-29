import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { EditorView } from '@codemirror/view';
import Composer from '@/components/chat/Composer';
import SkillsTab from '@/components/workbench/SkillsTab';
import { useChatStore } from '@/lib/chatStore';
import { useSessionStore } from '@/lib/sessionStore';
import { useSkillStore } from '@/lib/skillStore';
import { useToastStore } from '@/lib/toastStore';
import { useWorkbenchStore } from '@/lib/workbenchStore';

/**
 * F11 wave.5 Skill 管理 tab:列表/空态/加载失败如实 → 选中详情 → 编辑 dirty → 保存 PUT →
 * 校验拦截 → 新建内联输入 → 启停乐观回滚 → 删除三阶段(确认条 → 409 提案 → 批准重发);
 * 外加 Composer 两条:菜单按 enabled 过滤、发送走结构化 skills 字段(不再拼技能名文本前缀)。
 *
 * 后端由本文件的假 agentd 承担(wave.2 的真实端点契约见 11 §2.7):有状态,
 * 写操作真的改假磁盘,故「乐观更新后重拉」这类断言不会被桩代绿。
 */

const initialSkill = useSkillStore.getState();
const initialChat = useChatStore.getState();
const initialSessions = useSessionStore.getState();
const initialWorkbench = useWorkbenchStore.getState();

interface SkillFixture {
  name: string;
  description: string;
  content: string;
  builtin?: boolean;
  version?: string;
  license?: string;
  tags?: string[];
  dir?: string;
}

interface Call {
  method: string;
  url: string;
  body: Record<string, unknown> | null;
}

interface FakeOpts {
  skills?: SkillFixture[];
  listError?: { status: number; code: string; message: string };
  configError?: { status: number; code: string; message: string };
  putError?: { status: number; code: string; message: string };
  /** 校验结果(缺省 = 全绿);按草稿内容裁决,供「errors 阻止保存」用例注入。 */
  validate?: (content: string) => { valid: boolean; errors: string[]; warnings: string[] };
}

function res(status: number, body: unknown): Response {
  return {
    ok: status >= 200 && status < 300,
    status,
    json: async () => body,
    text: async () => (body === null ? '' : JSON.stringify(body)),
  } as Response;
}

const TEMPLATE = '---\nname: NAME\ndescription: 待补充\n---\n\n## 用途\n\n待补充。\n';

/** 有状态假 skills 后端(list/read/create/update/delete/validate/config + proposals)。 */
function stubSkillsBackend(opts: FakeOpts = {}) {
  const disk = new Map<string, SkillFixture>();
  for (const s of opts.skills ?? []) disk.set(s.name, { ...s });
  const state = {
    disabled: [] as string[],
    extraDirs: [] as string[],
    approved: new Set<string>(),
    proposalSeq: 0,
    calls: [] as Call[],
  };

  const detailOf = (s: SkillFixture) => ({
    name: s.name,
    content: s.content,
    front: {
      name: s.name,
      description: s.description,
      version: s.version ?? null,
      license: s.license ?? null,
      tags: s.tags ?? [],
      allowedTools: [],
    },
    builtin: s.builtin ?? true,
    path: `${s.dir ?? `skills/${s.name}`}/SKILL.md`,
  });

  vi.stubGlobal(
    'fetch',
    vi.fn(async (rawUrl: unknown, init?: { method?: string; body?: string }) => {
      const url = String(rawUrl);
      const method = init?.method ?? 'GET';
      const body = init?.body ? (JSON.parse(init.body) as Record<string, unknown>) : null;
      state.calls.push({ method, url, body });

      if (url === '/api/forge/skills/list') {
        if (opts.listError) {
          return res(opts.listError.status, {
            error: { code: opts.listError.code, message: opts.listError.message },
          });
        }
        return res(200, {
          skills: [...disk.values()].map((s) => ({
            name: s.name,
            description: s.description,
            enabled: !state.disabled.includes(s.name),
            version: s.version ?? null,
            license: s.license ?? null,
            tags: s.tags ?? [],
            allowedTools: [],
            builtin: s.builtin ?? true,
            dir: s.dir ?? `skills/${s.name}`,
          })),
        });
      }

      if (url === '/api/forge/skills/config/write') {
        if (opts.configError) {
          return res(opts.configError.status, {
            error: { code: opts.configError.code, message: opts.configError.message },
          });
        }
        if (Array.isArray(body?.disabled)) state.disabled = body.disabled as string[];
        if (Array.isArray(body?.extraDirs)) state.extraDirs = body.extraDirs as string[];
        return res(200, {
          written: true,
          disabled: state.disabled,
          extraDirs: state.extraDirs,
        });
      }

      if (url === '/api/forge/skills' && method === 'POST') {
        const name = String(body?.name ?? '');
        if (disk.has(name)) {
          return res(409, {
            error: { code: 'SKILL_ALREADY_EXISTS', message: `技能已存在: ${name}` },
          });
        }
        disk.set(name, {
          name,
          description: '待补充',
          content: TEMPLATE.replace('NAME', name),
        });
        return res(200, { name, created: true });
      }

      if (url.startsWith('/api/forge/skills/')) {
        const tail = url.slice('/api/forge/skills/'.length);
        if (tail.endsWith(':validate')) {
          const name = decodeURIComponent(tail.slice(0, -':validate'.length));
          const content = typeof body?.content === 'string' ? body.content : (disk.get(name)?.content ?? '');
          return res(200, opts.validate?.(content) ?? { valid: true, errors: [], warnings: [] });
        }
        const name = decodeURIComponent(tail);
        const hit = disk.get(name);
        if (!hit) {
          return res(404, { error: { code: 'SKILL_NOT_FOUND', message: `技能不存在: ${name}` } });
        }
        if (method === 'GET') return res(200, detailOf(hit));
        if (method === 'PUT') {
          if (opts.putError) {
            return res(opts.putError.status, {
              error: { code: opts.putError.code, message: opts.putError.message },
            });
          }
          hit.content = String(body?.content ?? '');
          return res(200, { name, updated: true });
        }
        if (method === 'DELETE') {
          if (!state.approved.has(name)) {
            state.proposalSeq += 1;
            const proposalId = `prop_${state.proposalSeq}`;
            return res(409, {
              error: {
                code: 'GOV_PROPOSAL_REQUIRED',
                message: `删除技能为 destructive,须先批准 Proposal(I-6): ${name}`,
                proposalId,
              },
            });
          }
          disk.delete(name);
          return res(200, { deleted: true, name });
        }
      }

      if (url.startsWith('/api/forge/proposals/') && method === 'PATCH') {
        // 假治理面:批准 = 放行下一次同名 DELETE(真后端按 impact.assets 覆盖判定)。
        for (const n of disk.keys()) state.approved.add(n);
        return res(200, { id: url.split('/').pop(), status: 'approved' });
      }

      throw new Error(`未 mock 的请求: ${method} ${url}`);
    }),
  );
  return { state, disk };
}

const SKILLS: SkillFixture[] = [
  {
    name: 'asset-cleanup',
    description: '整理资产',
    version: '1.2.0',
    license: 'MIT',
    tags: ['asset'],
    content: '---\nname: asset-cleanup\ndescription: 整理资产\n---\n\n## 用途\n\n清理无引用资产。\n',
  },
  {
    name: 'scene-greybox',
    description: '灰盒搭建',
    builtin: false,
    dir: 'vendor/skills/scene-greybox',
    content: '---\nname: scene-greybox\ndescription: 灰盒搭建\n---\n\n## 用途\n\n搭灰盒。\n',
  },
];

/** 打开技能详情并等 CM6 挂载,返回真实视图。 */
async function openSkill(name: string): Promise<EditorView> {
  fireEvent.click(await screen.findByTestId(`skill-select-${name}`));
  const host = await screen.findByTestId('skill-editor');
  const view = EditorView.findFromDOM(host as HTMLElement);
  expect(view).not.toBeNull();
  return view as EditorView;
}

beforeEach(() => {
  useSkillStore.setState(initialSkill, true);
  useChatStore.setState(initialChat, true);
  useChatStore.getState().reset();
  useSessionStore.setState(initialSessions, true);
  useWorkbenchStore.setState(initialWorkbench, true);
  useToastStore.getState().clear();
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe('<SkillsTab /> 列表', () => {
  it('渲染技能行:名/描述/版本 chip/来源徽标;未选中时右侧是技能目录卡', async () => {
    stubSkillsBackend({ skills: SKILLS });
    render(<SkillsTab />);
    expect(await screen.findByTestId('skill-row-asset-cleanup')).toBeInTheDocument();
    expect(screen.getByTestId('skill-row-asset-cleanup')).toHaveTextContent('整理资产');
    expect(screen.getByTestId('skill-version-asset-cleanup')).toHaveTextContent('v1.2.0');
    expect(screen.getByTestId('skill-origin-asset-cleanup')).toHaveTextContent('内置');
    expect(screen.getByTestId('skill-origin-scene-greybox')).toHaveTextContent('外部');
    expect(screen.getByTestId('skill-dirs')).toBeInTheDocument();
    expect(screen.queryByTestId('skills-error')).not.toBeInTheDocument();
  });

  it('搜索过滤;无技能显示「未发现技能」', async () => {
    stubSkillsBackend({ skills: SKILLS });
    const { unmount } = render(<SkillsTab />);
    fireEvent.change(await screen.findByTestId('skill-search'), { target: { value: '灰盒' } });
    expect(screen.queryByTestId('skill-row-asset-cleanup')).not.toBeInTheDocument();
    expect(screen.getByTestId('skill-row-scene-greybox')).toBeInTheDocument();
    unmount();

    useSkillStore.setState(initialSkill, true);
    vi.unstubAllGlobals();
    stubSkillsBackend({ skills: [] });
    render(<SkillsTab />);
    expect(await screen.findByTestId('skills-empty')).toHaveTextContent('未发现技能');
  });

  it('加载失败如实显示状态码与错误码,不伪造空列表', async () => {
    stubSkillsBackend({
      skills: SKILLS,
      listError: { status: 500, code: 'FORGE_IO', message: '读 skills 目录失败' },
    });
    render(<SkillsTab />);
    const err = await screen.findByTestId('skills-error');
    expect(err).toHaveTextContent('500 FORGE_IO');
    expect(err).toHaveTextContent('读 skills 目录失败');
    expect(screen.queryByTestId('skills-empty')).not.toBeInTheDocument();
  });
});

describe('<SkillsTab /> 详情与编辑', () => {
  it('选中技能 → 详情加载 → 编辑器显示 SKILL.md 全文 + 版本/许可证 chips', async () => {
    stubSkillsBackend({ skills: SKILLS });
    render(<SkillsTab />);
    const view = await openSkill('asset-cleanup');
    expect(view.state.doc.toString()).toContain('清理无引用资产');
    expect(screen.getByTestId('skill-detail-name')).toHaveTextContent('asset-cleanup');
    expect(screen.getByTestId('skill-detail-version')).toHaveTextContent('v1.2.0');
    expect(screen.getByTestId('skill-detail-license')).toHaveTextContent('MIT');
    expect(screen.queryByTestId('skill-dirty')).not.toBeInTheDocument();
  });

  it('编辑 → dirty 圆点 → 保存走 PUT(先校验后落盘),保存后 dirty 消失', async () => {
    const { state, disk } = stubSkillsBackend({ skills: SKILLS });
    render(<SkillsTab />);
    const view = await openSkill('asset-cleanup');
    act(() => {
      view.dispatch({ changes: { from: view.state.doc.length, insert: '\n补一行。\n' } });
    });
    expect(await screen.findByTestId('skill-dirty')).toBeInTheDocument();

    fireEvent.click(screen.getByTestId('skill-save'));
    await waitFor(() => {
      expect(state.calls.some((c) => c.method === 'PUT')).toBe(true);
    });
    const put = state.calls.find((c) => c.method === 'PUT');
    expect(put?.url).toBe('/api/forge/skills/asset-cleanup');
    expect(String(put?.body?.content)).toContain('补一行。');
    // 校验在 PUT 之前发生(不静默落盘)
    const validateIdx = state.calls.findIndex((c) => c.url.endsWith(':validate'));
    const putIdx = state.calls.findIndex((c) => c.method === 'PUT');
    expect(validateIdx).toBeGreaterThanOrEqual(0);
    expect(validateIdx).toBeLessThan(putIdx);
    expect(disk.get('asset-cleanup')?.content).toContain('补一行。');
    await waitFor(() => expect(screen.queryByTestId('skill-dirty')).not.toBeInTheDocument());
  });

  it('校验 errors 非空:阻止保存 + 逐条列出 errors/warnings', async () => {
    const { state } = stubSkillsBackend({
      skills: SKILLS,
      validate: () => ({
        valid: false,
        errors: ['缺少 frontmatter 的 description 键'],
        warnings: ['正文缺少「## 用途」小节'],
      }),
    });
    render(<SkillsTab />);
    const view = await openSkill('asset-cleanup');
    act(() => {
      view.dispatch({ changes: { from: 0, insert: 'x' } });
    });
    fireEvent.click(screen.getByTestId('skill-save'));

    expect(await screen.findByTestId('skill-validation-error')).toHaveTextContent(
      '缺少 frontmatter 的 description 键',
    );
    expect(screen.getByTestId('skill-validation-warning')).toHaveTextContent('正文缺少「## 用途」小节');
    expect(state.calls.some((c) => c.method === 'PUT')).toBe(false);
    expect(
      useToastStore.getState().items.some((t) => t.title.includes('已阻止保存')),
    ).toBe(true);
    // 手动「校验」钮同样出结果
    fireEvent.click(screen.getByTestId('skill-validate'));
    await waitFor(() => expect(screen.getByTestId('skill-validation')).toBeInTheDocument());
  });

  it('详情加载失败:错误条如实 + 右侧留白说明,不伪造内容', async () => {
    stubSkillsBackend({ skills: SKILLS });
    render(<SkillsTab />);
    await screen.findByTestId('skill-row-asset-cleanup');
    await act(async () => {
      await useSkillStore.getState().select('ghost');
    });
    expect(screen.getByTestId('skills-error')).toHaveTextContent('404 SKILL_NOT_FOUND');
    expect(screen.getByTestId('skill-detail-missing')).toBeInTheDocument();
    expect(screen.queryByTestId('skill-editor')).not.toBeInTheDocument();
  });

  it('保存失败(403 SKILL_READONLY_DIR)如实 toast,dirty 不清', async () => {
    stubSkillsBackend({
      skills: SKILLS,
      putError: { status: 403, code: 'SKILL_READONLY_DIR', message: '目录只读' },
    });
    render(<SkillsTab />);
    const view = await openSkill('scene-greybox');
    act(() => {
      view.dispatch({ changes: { from: 0, insert: 'x' } });
    });
    fireEvent.click(screen.getByTestId('skill-save'));
    await waitFor(() => {
      expect(
        useToastStore.getState().items.some((t) => t.title.includes('403 SKILL_READONLY_DIR')),
      ).toBe(true);
    });
    expect(screen.getByTestId('skill-dirty')).toBeInTheDocument();
  });
});

describe('<SkillsTab /> 新建', () => {
  it('内联输入行 → 回车 POST /skills → 自动选中并加载骨架内容', async () => {
    const { state } = stubSkillsBackend({ skills: SKILLS });
    render(<SkillsTab />);
    await screen.findByTestId('skill-row-asset-cleanup');
    fireEvent.click(screen.getByTestId('skill-new'));
    fireEvent.change(screen.getByTestId('skill-new-name'), { target: { value: 'perf-budget' } });
    fireEvent.keyDown(screen.getByTestId('skill-new-name'), { key: 'Enter' });

    expect(await screen.findByTestId('skill-row-perf-budget')).toBeInTheDocument();
    const post = state.calls.find((c) => c.url === '/api/forge/skills' && c.method === 'POST');
    expect(post?.body).toEqual({ name: 'perf-budget' });
    await waitFor(() =>
      expect(screen.getByTestId('skill-detail-name')).toHaveTextContent('perf-budget'),
    );
    const view = EditorView.findFromDOM(screen.getByTestId('skill-editor') as HTMLElement);
    expect(view?.state.doc.toString()).toContain('name: perf-budget');
    // 输入行已收起
    expect(screen.queryByTestId('skill-new-name')).not.toBeInTheDocument();
  });

  it('非法名:内联提示 + 不发请求;重名 409 内联如实', async () => {
    const { state } = stubSkillsBackend({ skills: SKILLS });
    render(<SkillsTab />);
    await screen.findByTestId('skill-row-asset-cleanup');
    fireEvent.click(screen.getByTestId('skill-new'));
    fireEvent.change(screen.getByTestId('skill-new-name'), { target: { value: 'Bad_Name' } });
    expect(screen.getByTestId('skill-new-name-hint')).toBeInTheDocument();
    fireEvent.keyDown(screen.getByTestId('skill-new-name'), { key: 'Enter' });
    expect(state.calls.some((c) => c.url === '/api/forge/skills' && c.method === 'POST')).toBe(false);

    fireEvent.change(screen.getByTestId('skill-new-name'), { target: { value: 'asset-cleanup' } });
    fireEvent.keyDown(screen.getByTestId('skill-new-name'), { key: 'Enter' });
    expect(await screen.findByTestId('skill-new-error')).toHaveTextContent('409 SKILL_ALREADY_EXISTS');
    // 输入行保留,不吞错
    expect(screen.getByTestId('skill-new-name')).toBeInTheDocument();
  });
});

describe('<SkillsTab /> 启停', () => {
  it('乐观更新 + config/write 全量 disabled 写回', async () => {
    const { state } = stubSkillsBackend({ skills: SKILLS });
    render(<SkillsTab />);
    const toggle = await screen.findByTestId('skill-toggle-asset-cleanup');
    expect(toggle).toHaveAttribute('aria-checked', 'true');
    fireEvent.click(toggle);
    await waitFor(() => {
      expect(screen.getByTestId('skill-toggle-asset-cleanup')).toHaveAttribute(
        'aria-checked',
        'false',
      );
    });
    const write = state.calls.find(
      (c) => c.url === '/api/forge/skills/config/write' && Array.isArray(c.body?.disabled),
    );
    expect(write?.body?.disabled).toEqual(['asset-cleanup']);
  });

  it('写回失败:回滚到原状态 + 错误 toast', async () => {
    stubSkillsBackend({
      skills: SKILLS,
      configError: { status: 500, code: 'FORGE_IO', message: '写配置失败' },
    });
    render(<SkillsTab />);
    const toggle = await screen.findByTestId('skill-toggle-asset-cleanup');
    fireEvent.click(toggle);
    await waitFor(() => {
      expect(
        useToastStore.getState().items.some((t) => t.title.includes('技能启停写回失败')),
      ).toBe(true);
    });
    expect(screen.getByTestId('skill-toggle-asset-cleanup')).toHaveAttribute('aria-checked', 'true');
  });
});

describe('<SkillsTab /> 删除(Proposal 两阶段)', () => {
  it('确认条 → 409 拿 proposalId → 在此批准 → 重发 DELETE 成功', async () => {
    const { state, disk } = stubSkillsBackend({ skills: SKILLS });
    render(<SkillsTab />);
    await openSkill('asset-cleanup');

    fireEvent.click(screen.getByTestId('skill-delete'));
    const bar = await screen.findByTestId('skill-delete-bar');
    expect(bar).toHaveTextContent('删除技能 asset-cleanup');
    expect(bar).toHaveTextContent('此操作需要提案确认');

    fireEvent.click(screen.getByTestId('skill-delete-continue'));
    expect(await screen.findByTestId('skill-delete-proposal')).toHaveTextContent('已创建提案 prop_1');
    expect(disk.has('asset-cleanup')).toBe(true); // 未批准前不落删除

    fireEvent.click(screen.getByTestId('skill-delete-approve'));
    await waitFor(() => expect(disk.has('asset-cleanup')).toBe(false));
    const patch = state.calls.find((c) => c.method === 'PATCH');
    expect(patch?.url).toBe('/api/forge/proposals/prop_1');
    expect(patch?.body).toEqual({ action: 'approve' });
    expect(state.calls.filter((c) => c.method === 'DELETE')).toHaveLength(2);
    // 列表刷新 + 右侧回落目录卡
    await waitFor(() =>
      expect(screen.queryByTestId('skill-row-asset-cleanup')).not.toBeInTheDocument(),
    );
    expect(screen.getByTestId('skill-dirs')).toBeInTheDocument();
  });

  it('「去提案页批准」开 proposals tab(只读消费 workbenchStore);取消关掉确认条', async () => {
    stubSkillsBackend({ skills: SKILLS });
    render(<SkillsTab />);
    await openSkill('asset-cleanup');
    fireEvent.click(screen.getByTestId('skill-delete'));
    fireEvent.click(screen.getByTestId('skill-delete-continue'));
    await screen.findByTestId('skill-delete-proposal');

    fireEvent.click(screen.getByTestId('skill-delete-goto-proposals'));
    expect(useWorkbenchStore.getState().tabs.some((t) => t.kind === 'proposals')).toBe(true);
    expect(useWorkbenchStore.getState().activeTabId).toBe('proposals');

    fireEvent.click(screen.getByTestId('skill-delete-cancel'));
    expect(screen.queryByTestId('skill-delete-bar')).not.toBeInTheDocument();
  });
});

describe('<SkillsTab /> 技能目录(extraDirs)', () => {
  it('添加目录 → config/write {extraDirs};移除同理', async () => {
    const { state } = stubSkillsBackend({ skills: SKILLS });
    render(<SkillsTab />);
    expect(await screen.findByTestId('skill-dirs-empty')).toBeInTheDocument();
    fireEvent.change(screen.getByTestId('skill-dir-input'), { target: { value: 'vendor/skills' } });
    fireEvent.click(screen.getByTestId('skill-dir-add'));
    expect(await screen.findByTestId('skill-dir-vendor/skills')).toBeInTheDocument();
    const write = state.calls.find(
      (c) => c.url === '/api/forge/skills/config/write' && Array.isArray(c.body?.extraDirs),
    );
    expect(write?.body?.extraDirs).toEqual(['vendor/skills']);

    fireEvent.click(screen.getByTestId('skill-dir-remove-vendor/skills'));
    await waitFor(() => expect(screen.getByTestId('skill-dirs-empty')).toBeInTheDocument());
  });
});

describe('<Composer /> 技能接线(F11)', () => {
  function stubComposerBackend(skills: Array<{ name: string; description: string; enabled: boolean }>) {
    const asks: Array<Record<string, unknown>> = [];
    vi.stubGlobal(
      'fetch',
      vi.fn(async (rawUrl: unknown, init?: { method?: string; body?: string }) => {
        const url = String(rawUrl);
        if (url === '/api/forge/skills/list') return res(200, { skills });
        if (url.includes('ask:execute')) {
          asks.push(JSON.parse(init?.body ?? '{}') as Record<string, unknown>);
          return res(200, { ok: true });
        }
        return res(200, {});
      }),
    );
    return asks;
  }

  it('菜单只列启用项(设置页禁用的技能不再出现)', async () => {
    stubComposerBackend([
      { name: 'scene-greybox', description: '灰盒搭建', enabled: true },
      { name: 'asset-cleanup', description: '整理资产', enabled: false },
    ]);
    useSessionStore.setState({ activeSessionId: 'sess_1' });
    render(<Composer />);
    fireEvent.click(screen.getByTestId('composer-skills'));
    expect(await screen.findByTestId('skill-item-scene-greybox')).toBeInTheDocument();
    expect(screen.queryByTestId('skill-item-asset-cleanup')).not.toBeInTheDocument();
  });

  it('每次开菜单都重拉清单(设置页改了配置这里跟着变)', async () => {
    const fetchMock = vi.fn(async (rawUrl: unknown) => {
      const url = String(rawUrl);
      if (url === '/api/forge/skills/list') return res(200, { skills: [] });
      return res(200, {});
    });
    vi.stubGlobal('fetch', fetchMock);
    useSessionStore.setState({ activeSessionId: 'sess_1' });
    render(<Composer />);
    fireEvent.click(screen.getByTestId('composer-skills'));
    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(1));
    fireEvent.click(screen.getByTestId('composer-skills')); // 关
    fireEvent.click(screen.getByTestId('composer-skills')); // 再开
    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(2));
  });

  it('发送:ask:execute 请求体含结构化 skills 字段,正文无技能名文本前缀', async () => {
    const asks = stubComposerBackend([
      { name: 'scene-greybox', description: '灰盒搭建', enabled: true },
    ]);
    useSessionStore.setState({ activeSessionId: 'sess_1' });
    render(<Composer />);
    fireEvent.click(screen.getByTestId('composer-skills'));
    fireEvent.click(await screen.findByTestId('skill-item-scene-greybox'));
    fireEvent.change(screen.getByTestId('composer-input'), { target: { value: '整理场景' } });
    fireEvent.click(screen.getByTestId('composer-send'));

    await waitFor(() => expect(asks).toHaveLength(1));
    expect(asks[0]).toEqual({ userInput: '整理场景', mode: 'build', skills: ['scene-greybox'] });
    // 退役的文本前缀协议不得复活(正则比字面量更宽,连变体一并挡住)
    expect(JSON.stringify(asks[0])).not.toMatch(/use\s+skills/i);
    // chips 已清空
    expect(screen.queryByTestId('skill-chip-scene-greybox')).not.toBeInTheDocument();
  });

  it('未选技能:请求体不带 skills 字段', async () => {
    const asks = stubComposerBackend([]);
    useSessionStore.setState({ activeSessionId: 'sess_1' });
    render(<Composer />);
    fireEvent.change(screen.getByTestId('composer-input'), { target: { value: '你好' } });
    fireEvent.click(screen.getByTestId('composer-send'));
    await waitFor(() => expect(asks).toHaveLength(1));
    expect(asks[0]).toEqual({ userInput: '你好', mode: 'build' });
  });
});
