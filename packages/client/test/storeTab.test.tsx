import { act, cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import StoreTab from '@/components/workbench/StoreTab';
import { useAssetStore } from '@/lib/assetStore';
import type {
  InstallRecord,
  LibraryItem,
  PackageDetail,
  PackageManifest,
  PackageSummary,
  SourceError,
  StoreSource,
  StoreTask,
  UpdateInfo,
} from '@/lib/storeApi';
import { useStoreStore } from '@/lib/storeStore';
import { useToastStore } from '@/lib/toastStore';

/**
 * F11 wave.4 资产商店 tab 测试(照 shell.test.tsx 隔离范式:只挂 StoreTab + 假 fetch)。
 * 假后端按 11_API_CONTRACTS.md §2.7 路由,数据形态取自仓内 registry/ 种子。
 * 全程 fake timers:安装轮询(1s 间隔)靠 advanceTimersByTime 精确推进,
 * 并据此断言组件卸载后定时器确已停止(不再发 /tasks/ 请求)。
 */

// ---------- 种子数据(形态对齐 registry/index.json 与 packages/*/1.0.0.json) ----------

const OFFICIAL: StoreSource = {
  id: 'official',
  name: 'RurixForge 官方源',
  baseUrl: 'file://registry',
  enabled: true,
  hasToken: false,
};
const COMMUNITY: StoreSource = {
  id: 'community',
  name: '社区源',
  baseUrl: 'https://example.invalid/registry',
  enabled: true,
  hasToken: true,
};

const STARTER: PackageSummary = {
  id: 'forge.starter-props',
  name: '入门示例道具包',
  kind: 'asset-pack',
  description: '三类资产各一件(网格/贴图/材质),用于验证商店安装链路与按扩展名分流落地。',
  latestVersion: '1.0.0',
  tags: ['示例', '道具', '入门'],
  license: { id: 'CC0-1.0', url: 'https://creativecommons.org/publicdomain/zero/1.0/' },
  pricing: { amount: 0, currency: 'CNY' },
  publisher: { id: 'rurixforge', name: 'RurixForge 官方', url: 'https://github.com/rurixforge' },
  thumbnail: null,
};

const WOOD: PackageSummary = {
  ...STARTER,
  id: 'forge.wood-pbr',
  name: '木质 PBR 贴图组',
  description: '一套木质 PBR 贴图(albedo / normal / roughness)。',
  latestVersion: '1.1.0',
  tags: ['贴图', 'PBR', '木质'],
  license: { id: 'CC-BY-4.0', url: 'https://creativecommons.org/licenses/by/4.0/' },
};

const SKILL: PackageSummary = {
  ...STARTER,
  id: 'forge.skill-scene-audit',
  name: '技能:场景健康度审计',
  kind: 'skill',
  description: '交付前对场景做只读体检,产出 blocker/warn/info 三级问题清单。',
  tags: ['技能', '场景', '审计'],
};

const PREMIUM: PackageSummary = {
  ...STARTER,
  id: 'forge.premium-pack',
  name: '付费素材集',
  description: '用于验证付费闸门的样例包。',
  pricing: { amount: 30, currency: 'CNY' },
};

const STARTER_MANIFEST: PackageManifest = {
  id: STARTER.id,
  name: STARTER.name,
  kind: 'asset-pack',
  description: STARTER.description,
  version: '1.0.0',
  tags: STARTER.tags,
  license: STARTER.license,
  pricing: STARTER.pricing,
  publisher: STARTER.publisher,
  engineVersion: '>=0.1.0',
  dependencies: [{ id: 'forge.wood-pbr', version: '1.0.0' }],
  files: [
    { path: 'starter_chair.gltf', sha256: '625cfe10aa247f636c0ad394c771ecdbc5d4ed2d101429ceb3c3277df7aba4b1', size: 440 },
    { path: 'starter_dot.png', sha256: '1a304f04d6f5a13a83988b695f06917e3f45aee73564e3575c285ec6faf250fe', size: 129 },
    { path: 'starter_red.rxmat', sha256: '0ec5ad5dd40d89bfac31ff7554901b930095949ea02f45b4a2e0aa00f9763a72', size: 142 },
  ],
  preview: { thumbnail: null, screenshots: [] },
  createdAt: '2026-08-25T07:02:29Z',
  updatedAt: '2026-08-25T07:02:29Z',
};

const INSTALLED_STARTER: InstallRecord = {
  sourceId: 'official',
  packageId: 'forge.starter-props',
  version: '1.0.0',
  kind: 'asset-pack',
  assetPaths: ['Meshes/starter_chair.gltf', 'Textures/starter_dot.png', 'Materials/starter_red.rxmat'],
  skillNames: [],
  installedAt: '2026-08-25T07:10:00Z',
};

const LIB_ITEM: LibraryItem = {
  id: 'lib_1',
  name: 'chair.gltf',
  sha256: 'aa11bb22cc33dd44',
  size: 2048,
  ext: 'gltf',
  kind: 'mesh',
  tags: ['家具'],
  source: 'project',
  addedAt: '2026-08-25T06:00:00Z',
};

// ---------- 假后端 ----------

interface Call {
  url: string;
  method: string;
  body: string;
}

interface Backend {
  sources: StoreSource[];
  catalog: Array<{ sourceId: string; sourceName: string; package: PackageSummary }>;
  searchErrors: SourceError[];
  packages: Record<string, PackageDetail>;
  manifests: Record<string, PackageManifest>;
  installed: InstallRecord[];
  updates: UpdateInfo[];
  library: LibraryItem[];
  installTaskId: string;
  /** 每次 GET /tasks/{id} 依次取一个;取尽后停在最后一个 */
  taskQueue: StoreTask[];
  /** 首次卸载是否回 409 GOV_PROPOSAL_REQUIRED */
  requireProposal: boolean;
  proposalId: string;
}

function defaultBackend(over: Partial<Backend> = {}): Backend {
  return {
    sources: [OFFICIAL, COMMUNITY],
    catalog: [
      { sourceId: 'official', sourceName: OFFICIAL.name, package: STARTER },
      { sourceId: 'official', sourceName: OFFICIAL.name, package: WOOD },
      { sourceId: 'official', sourceName: OFFICIAL.name, package: SKILL },
      { sourceId: 'community', sourceName: COMMUNITY.name, package: PREMIUM },
    ],
    searchErrors: [],
    packages: {
      'official/forge.starter-props': { sourceId: 'official', summary: STARTER, versions: ['1.0.0'] },
      'official/forge.wood-pbr': { sourceId: 'official', summary: WOOD, versions: ['1.1.0', '1.0.0'] },
    },
    manifests: { 'official/forge.starter-props/1.0.0': STARTER_MANIFEST },
    installed: [],
    updates: [],
    library: [],
    installTaskId: 'task_1',
    taskQueue: [],
    requireProposal: false,
    proposalId: 'prop_1',
    ...over,
  };
}

function resp(status: number, payload: unknown): Response {
  const text = payload === undefined ? '' : JSON.stringify(payload);
  return {
    ok: status >= 200 && status < 300,
    status,
    text: async () => text,
    json: async () => payload,
  } as Response;
}

function stubFetch(b: Backend): Call[] {
  const calls: Call[] = [];
  vi.stubGlobal(
    'fetch',
    vi.fn(async (input: unknown, init?: RequestInit) => {
      const url = String(input);
      const method = (init?.method ?? 'GET').toUpperCase();
      const body = typeof init?.body === 'string' ? init.body : '';
      calls.push({ url, method, body });
      const [path, qs] = url.split('?');
      const q = new URLSearchParams(qs ?? '');
      const json = <T,>(): T => JSON.parse(body === '' ? '{}' : body) as T;

      // 源
      if (path === '/api/forge/store/sources' && method === 'GET') return resp(200, { sources: b.sources });
      if (path === '/api/forge/store/sources' && method === 'POST') {
        const p = json<{ id: string; name: string; baseUrl: string; token?: string }>();
        const s: StoreSource = { id: p.id, name: p.name, baseUrl: p.baseUrl, enabled: true, hasToken: !!p.token };
        b.sources = [...b.sources, s];
        return resp(200, { source: s });
      }
      if (path.startsWith('/api/forge/store/sources/') && method === 'PATCH') {
        const id = path.slice('/api/forge/store/sources/'.length);
        const p = json<{ enabled?: boolean; name?: string; token?: string }>();
        b.sources = b.sources.map((s) =>
          s.id === id
            ? { ...s, ...(p.enabled === undefined ? {} : { enabled: p.enabled }), ...(p.name ? { name: p.name } : {}), ...(p.token ? { hasToken: true } : {}) }
            : s,
        );
        return resp(200, { source: b.sources.find((s) => s.id === id) });
      }
      if (path.startsWith('/api/forge/store/sources/') && method === 'DELETE') {
        b.sources = b.sources.filter((s) => s.id !== path.slice('/api/forge/store/sources/'.length));
        return resp(200, undefined);
      }

      // 搜索
      if (path === '/api/forge/store/search') {
        const kw = (q.get('q') ?? '').toLowerCase();
        const kind = q.get('kind');
        const sourceId = q.get('sourceId');
        const page = Number(q.get('page') ?? '1');
        const pageSize = Number(q.get('pageSize') ?? '12');
        const hits = b.catalog.filter(
          (h) =>
            (kw === '' || h.package.name.toLowerCase().includes(kw) || h.package.id.toLowerCase().includes(kw)) &&
            (!kind || h.package.kind === kind) &&
            (!sourceId || h.sourceId === sourceId),
        );
        return resp(200, {
          total: hits.length,
          page,
          pageSize,
          items: hits.slice((page - 1) * pageSize, page * pageSize),
          errors: b.searchErrors,
        });
      }

      // 包详情 / 版本
      if (path.startsWith('/api/forge/store/packages/') && method === 'GET') {
        const rest = path.slice('/api/forge/store/packages/'.length).split('/');
        if (rest.length === 2) {
          const d = b.packages[`${rest[0]}/${rest[1]}`];
          return d ? resp(200, d) : resp(404, { error: { code: 'STORE_PACKAGE_NOT_FOUND', message: '包不存在' } });
        }
        const m = b.manifests[`${rest[0]}/${rest[1]}/${rest[2]}`];
        return m
          ? resp(200, { sourceId: rest[0], manifest: m })
          : resp(404, { error: { code: 'STORE_VERSION_NOT_FOUND', message: '版本不存在' } });
      }

      // 安装 / 长任务
      if (path === '/api/forge/store/install' && method === 'POST') return resp(200, { taskId: b.installTaskId });
      if (path.startsWith('/api/forge/store/tasks/') && method === 'GET') {
        if (b.taskQueue.length === 0) return resp(404, { error: { code: 'STORE_TASK_NOT_FOUND', message: '任务不存在' } });
        const t = b.taskQueue.length > 1 ? (b.taskQueue.shift() as StoreTask) : b.taskQueue[0];
        return resp(200, t);
      }

      // 卸载(治理两阶段)
      if (path === '/api/forge/store/uninstall' && method === 'POST') {
        if (b.requireProposal) {
          return resp(409, {
            error: { code: 'GOV_PROPOSAL_REQUIRED', message: '卸载须经提案批准', proposalId: b.proposalId },
          });
        }
        return resp(200, { taskId: 'task_uninstall' });
      }
      if (path.startsWith('/api/forge/proposals/') && method === 'PATCH') {
        b.requireProposal = false;
        return resp(200, { id: b.proposalId, status: 'approved' });
      }

      if (path === '/api/forge/store/installed') return resp(200, { installed: b.installed });
      if (path === '/api/forge/store/updates') return resp(200, { updates: b.updates });

      // 个人资产库
      if (path.endsWith(':install') && method === 'POST') return resp(200, { assetPath: 'Meshes/chair.gltf' });
      if (path === '/api/forge/store/library' && method === 'GET') return resp(200, { items: b.library });
      if (path === '/api/forge/store/library' && method === 'POST') {
        const p = json<{ assetPath: string }>();
        const item: LibraryItem = { ...LIB_ITEM, id: `lib_${b.library.length + 1}`, name: p.assetPath };
        b.library = [...b.library, item];
        return resp(200, { item });
      }
      if (path.startsWith('/api/forge/store/library/') && method === 'DELETE') {
        b.library = b.library.filter((i) => i.id !== path.slice('/api/forge/store/library/'.length));
        return resp(200, { removed: true });
      }

      throw new Error(`未 mock 的商店端点: ${method} ${url}`);
    }),
  );
  return calls;
}

// ---------- 夹具 ----------

const initialStore = useStoreStore.getState();
const initialAssets = useAssetStore.getState();

/** 冲干净微任务队列(fake timers 下不推进时钟,只让 promise 链跑完)。 */
async function flush(rounds = 6): Promise<void> {
  for (let i = 0; i < rounds; i++) {
    await act(async () => {
      await Promise.resolve();
    });
  }
}

/** 推进 ms 毫秒并冲干净由此触发的异步链。 */
async function tick(ms = 1000): Promise<void> {
  await act(async () => {
    vi.advanceTimersByTime(ms);
  });
  await flush();
}

async function mount() {
  const utils = render(<StoreTab />);
  await flush();
  return utils;
}

beforeEach(() => {
  useStoreStore.getState().stopPolling();
  useStoreStore.setState(initialStore, true);
  useAssetStore.setState(initialAssets, true);
  useToastStore.getState().clear();
  vi.useFakeTimers();
});

afterEach(() => {
  cleanup();
  useStoreStore.getState().stopPolling();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

const toastTitles = () => useToastStore.getState().items.map((t) => t.title);

// ---------- 用例 ----------

describe('商店壳:三子页切换', () => {
  it('默认发现页;点「已安装」「我的资产库」各自换页并拉对应清单', async () => {
    const b = defaultBackend({ installed: [INSTALLED_STARTER], library: [LIB_ITEM] });
    const calls = stubFetch(b);
    await mount();

    expect(screen.getByTestId('store-tab')).toBeInTheDocument();
    expect(screen.getByTestId('store-discover')).toBeInTheDocument();
    expect(screen.queryByTestId('store-installed')).not.toBeInTheDocument();

    fireEvent.click(screen.getByTestId('store-subtab-installed'));
    await flush();
    expect(screen.getByTestId('store-installed')).toBeInTheDocument();
    expect(screen.queryByTestId('store-discover')).not.toBeInTheDocument();

    fireEvent.click(screen.getByTestId('store-subtab-library'));
    await flush();
    expect(screen.getByTestId('store-library')).toBeInTheDocument();
    expect(calls.some((c) => c.url === '/api/forge/store/library' && c.method === 'GET')).toBe(true);

    fireEvent.click(screen.getByTestId('store-subtab-discover'));
    await flush();
    expect(screen.getByTestId('store-discover')).toBeInTheDocument();
  });
});

describe('发现页', () => {
  it('首屏卡片渲染:名称 / 版本 / 许可证 / 价格 / 类型徽标 + 分页条', async () => {
    stubFetch(defaultBackend());
    await mount();

    const card = screen.getByTestId('store-card-forge.starter-props');
    expect(card).toHaveTextContent('入门示例道具包');
    expect(card).toHaveTextContent('v1.0.0');
    expect(card).toHaveTextContent('CC0-1.0');
    expect(card).toHaveTextContent('资产包');
    expect(card).toHaveTextContent('RurixForge 官方');
    expect(screen.getByTestId('store-card-price-forge.starter-props')).toHaveTextContent('免费');
    // 技能包类型徽标
    expect(screen.getByTestId('store-card-forge.skill-scene-audit')).toHaveTextContent('技能');
    // 四个包一页装下
    expect(screen.getByTestId('store-page-indicator')).toHaveTextContent('第 1 / 1 页 · 共 4 个包');
    expect(screen.getByTestId('store-page-prev')).toBeDisabled();
    expect(screen.getByTestId('store-page-next')).toBeDisabled();
  });

  it('搜索框回车检索:query 进 wire,结果收窄', async () => {
    const calls = stubFetch(defaultBackend());
    await mount();

    fireEvent.change(screen.getByTestId('store-search-input'), { target: { value: '木质' } });
    fireEvent.keyDown(screen.getByTestId('store-search-input'), { key: 'Enter' });
    await flush();

    const searched = calls.filter((c) => c.url.startsWith('/api/forge/store/search'));
    expect(searched[searched.length - 1].url).toContain('q=');
    expect(screen.getByTestId('store-card-forge.wood-pbr')).toBeInTheDocument();
    expect(screen.queryByTestId('store-card-forge.starter-props')).not.toBeInTheDocument();
  });

  it('类型 chips:切「技能」→ kind 进 query,只剩技能包', async () => {
    const calls = stubFetch(defaultBackend());
    await mount();

    fireEvent.click(screen.getByTestId('store-kind-skill'));
    await flush();

    const searched = calls.filter((c) => c.url.startsWith('/api/forge/store/search'));
    expect(searched[searched.length - 1].url).toContain('kind=skill');
    expect(screen.getByTestId('store-card-forge.skill-scene-audit')).toBeInTheDocument();
    expect(screen.queryByTestId('store-card-forge.starter-props')).not.toBeInTheDocument();
  });

  it('errors[] 非空:黄色横幅报「N 个源不可达」,展开列出源 id + 错误码 + 消息', async () => {
    stubFetch(
      defaultBackend({
        searchErrors: [
          { sourceId: 'community', code: 'STORE_SOURCE_UNREACHABLE', message: 'connect ECONNREFUSED 127.0.0.1:9' },
        ],
      }),
    );
    await mount();

    const banner = screen.getByTestId('store-source-errors');
    expect(banner).toHaveTextContent('1 个源不可达');
    fireEvent.click(screen.getByTestId('store-source-errors-toggle'));
    const row = screen.getByTestId('store-source-error-community');
    expect(row).toHaveTextContent('community');
    expect(row).toHaveTextContent('STORE_SOURCE_UNREACHABLE');
    expect(row).toHaveTextContent('connect ECONNREFUSED 127.0.0.1:9');
  });

  it('搜索无结果:空态给出建议,不伪装成加载中', async () => {
    stubFetch(defaultBackend());
    await mount();
    fireEvent.change(screen.getByTestId('store-search-input'), { target: { value: '不存在的东西' } });
    fireEvent.keyDown(screen.getByTestId('store-search-input'), { key: 'Enter' });
    await flush();
    expect(screen.getByTestId('store-empty')).toHaveTextContent('未找到匹配的包');
    expect(screen.queryByTestId('store-grid')).not.toBeInTheDocument();
  });

  it('付费包:安装钮禁用 + 「暂不支持付费获取」如实标注', async () => {
    stubFetch(defaultBackend());
    await mount();
    const card = screen.getByTestId('store-card-forge.premium-pack');
    expect(screen.getByTestId('store-card-price-forge.premium-pack')).toHaveTextContent('¥30');
    expect(card).toHaveTextContent('暂不支持付费获取');
    expect(screen.getByTestId('store-card-install-forge.premium-pack')).toBeDisabled();
  });

  it('已安装的包:卡片安装钮转「已安装」灰态;有新版时转「更新到 x.y.z」', async () => {
    stubFetch(
      defaultBackend({
        installed: [INSTALLED_STARTER, { ...INSTALLED_STARTER, packageId: 'forge.wood-pbr', version: '1.0.0' }],
      }),
    );
    await mount();
    expect(screen.getByTestId('store-card-install-forge.starter-props')).toBeDisabled();
    expect(screen.getByTestId('store-card-install-forge.starter-props')).toHaveTextContent('已安装');
    // wood 已装 1.0.0,源上最新 1.1.0 → 更新按钮
    expect(screen.getByTestId('store-card-install-forge.wood-pbr')).toHaveTextContent('更新到 1.1.0');
    expect(screen.getByTestId('store-card-install-forge.wood-pbr')).toBeEnabled();
  });
});

describe('详情分栏', () => {
  it('点卡片 → 右侧 360px 分栏(非弹窗):版本下拉 / 许可证 / 文件清单 / 依赖', async () => {
    stubFetch(defaultBackend());
    await mount();

    fireEvent.click(screen.getByTestId('store-card-forge.starter-props'));
    await flush();

    const detail = screen.getByTestId('store-detail');
    expect(detail).toHaveTextContent('入门示例道具包');
    expect(detail.className).toContain('w-[360px]');
    expect(screen.getByTestId('store-detail-license')).toHaveTextContent('CC0-1.0');
    expect(screen.getByTestId('store-detail-version')).toHaveValue('1.0.0');

    // 文件清单:三行 path / 人类可读大小 / sha256 前 8 位
    const files = screen.getByTestId('store-detail-files');
    expect(within(files).getByTestId('store-detail-file-starter_chair.gltf')).toHaveTextContent('starter_chair.gltf');
    expect(within(files).getByTestId('store-detail-file-starter_chair.gltf')).toHaveTextContent('440 B');
    expect(within(files).getByTestId('store-detail-file-starter_chair.gltf')).toHaveTextContent('625cfe10');
    expect(within(files).getAllByTestId(/store-detail-file-/)).toHaveLength(3);
    // 依赖
    expect(screen.getByTestId('store-detail-deps')).toHaveTextContent('forge.wood-pbr @ 1.0.0');
    // 目标文件夹占位说明
    expect(screen.getByTestId('store-detail-dest')).toHaveAttribute(
      'placeholder',
      '安装目标文件夹（留空则按类型自动分流）',
    );

    fireEvent.click(screen.getByTestId('store-detail-close'));
    expect(screen.queryByTestId('store-detail')).not.toBeInTheDocument();
  });
});

describe('安装长任务', () => {
  const RUNNING_QUEUE: StoreTask[] = [
    { taskId: 'task_1', status: 'running', phase: 'resolve', done: 0, total: 3 },
    { taskId: 'task_1', status: 'running', phase: 'download', done: 2, total: 3 },
    { taskId: 'task_1', status: 'completed', phase: 'record', done: 3, total: 3, record: INSTALLED_STARTER },
  ];

  async function openStarterDetail(b: Backend) {
    const calls = stubFetch(b);
    await mount();
    fireEvent.click(screen.getByTestId('store-card-forge.starter-props'));
    await flush();
    return calls;
  }

  it('点安装 → POST 拿 taskId → 1s 轮询推进 phase 文案 → completed 出 toast 并刷新已安装', async () => {
    const b = defaultBackend({ taskQueue: [...RUNNING_QUEUE] });
    const calls = await openStarterDetail(b);

    fireEvent.change(screen.getByTestId('store-detail-dest'), { target: { value: 'Props' } });
    fireEvent.click(screen.getByTestId('store-detail-install'));
    await flush();

    const post = calls.find((c) => c.url === '/api/forge/store/install' && c.method === 'POST');
    expect(post).toBeDefined();
    expect(JSON.parse(post!.body)).toEqual({
      sourceId: 'official',
      pkgId: 'forge.starter-props',
      version: '1.0.0',
      destFolder: 'Props',
    });
    // 立即一拍:解析清单
    expect(screen.getByTestId('store-task-phase')).toHaveTextContent('解析清单');

    // 装完后已安装清单要能真的多一条
    b.installed = [INSTALLED_STARTER];

    await tick(1000);
    expect(screen.getByTestId('store-task-phase')).toHaveTextContent('下载中 2/3');

    await tick(1000);
    expect(toastTitles()).toContain('安装完成');
    expect(useStoreStore.getState().pollingTaskId).toBeNull();
    expect(useStoreStore.getState().installed).toHaveLength(1);
    // 轮询自停:再推 3 秒不再发 /tasks/ 请求
    const before = calls.filter((c) => c.url.startsWith('/api/forge/store/tasks/')).length;
    await tick(3000);
    expect(calls.filter((c) => c.url.startsWith('/api/forge/store/tasks/')).length).toBe(before);
  });

  it('卡上快装:不捎带详情栏里看不见的 destFolder,按类型自动分流', async () => {
    const b = defaultBackend({ taskQueue: [...RUNNING_QUEUE] });
    const calls = await openStarterDetail(b);
    // 详情栏填了目标文件夹,但点的是网格里另一个包的安装钮
    fireEvent.change(screen.getByTestId('store-detail-dest'), { target: { value: 'Props' } });
    fireEvent.click(screen.getByTestId('store-card-install-forge.wood-pbr'));
    await flush();
    const post = calls.find((c) => c.url === '/api/forge/store/install' && c.method === 'POST');
    expect(JSON.parse(post!.body)).toEqual({
      sourceId: 'official',
      pkgId: 'forge.wood-pbr',
      version: '1.1.0',
    });
  });

  it('任务 failed:可读文案 + 错误码 + 原始 message 三者都在,不化简', async () => {
    const b = defaultBackend({
      taskQueue: [
        { taskId: 'task_1', status: 'running', phase: 'verify', done: 1, total: 3 },
        {
          taskId: 'task_1',
          status: 'failed',
          phase: 'verify',
          done: 1,
          total: 3,
          error: { code: 'STORE_CHECKSUM_MISMATCH', message: 'starter_dot.png 摘要不符' },
        },
      ],
    });
    await openStarterDetail(b);

    fireEvent.click(screen.getByTestId('store-detail-install'));
    await flush();
    expect(screen.getByTestId('store-task-phase')).toHaveTextContent('校验完整性');

    await tick(1000);
    const err = screen.getByTestId('store-task-error');
    expect(err).toHaveTextContent('文件校验失败，已中止安装');
    expect(screen.getByTestId('store-task-error-code')).toHaveTextContent('STORE_CHECKSUM_MISMATCH');
    expect(err).toHaveTextContent('starter_dot.png 摘要不符');
    expect(useStoreStore.getState().pollingTaskId).toBeNull();
  });

  it('组件卸载:轮询定时器被清理,不再发 /tasks/ 请求', async () => {
    const b = defaultBackend({
      taskQueue: [{ taskId: 'task_1', status: 'running', phase: 'download', done: 1, total: 9 }],
    });
    const calls = stubFetch(b);
    const { unmount } = render(<StoreTab />);
    await flush();
    fireEvent.click(screen.getByTestId('store-card-forge.starter-props'));
    await flush();
    fireEvent.click(screen.getByTestId('store-detail-install'));
    await flush();

    await tick(1000);
    const polled = calls.filter((c) => c.url.startsWith('/api/forge/store/tasks/')).length;
    expect(polled).toBeGreaterThanOrEqual(2);
    expect(useStoreStore.getState().pollingTaskId).toBe('task_1');

    unmount();
    expect(useStoreStore.getState().pollingTaskId).toBeNull();
    await tick(5000);
    expect(calls.filter((c) => c.url.startsWith('/api/forge/store/tasks/')).length).toBe(polled);
  });
});

describe('已安装页', () => {
  it('行渲染 + 检查更新 → 「更新到 x.y.z」按钮出现', async () => {
    const b = defaultBackend({
      installed: [INSTALLED_STARTER],
      updates: [
        { sourceId: 'official', packageId: 'forge.starter-props', current: '1.0.0', latest: '1.2.0', hasUpdate: true },
      ],
    });
    stubFetch(b);
    await mount();
    fireEvent.click(screen.getByTestId('store-subtab-installed'));
    await flush();

    const row = screen.getByTestId('store-installed-row-official/forge.starter-props');
    expect(row).toHaveTextContent('forge.starter-props');
    expect(row).toHaveTextContent('v1.0.0');
    expect(row).toHaveTextContent('来源 official');
    expect(row).toHaveTextContent('资产 3');

    expect(screen.queryByTestId('store-update-official/forge.starter-props')).not.toBeInTheDocument();
    fireEvent.click(screen.getByTestId('store-check-updates'));
    await flush();
    expect(screen.getByTestId('store-update-official/forge.starter-props')).toHaveTextContent('更新到 1.2.0');
  });

  it('卸载三阶段:行内确认条 → 409 拿 proposalId → 批准后自动重发(全程无模态)', async () => {
    const b = defaultBackend({
      installed: [INSTALLED_STARTER],
      requireProposal: true,
      taskQueue: [{ taskId: 'task_uninstall', status: 'completed', phase: 'record', done: 1, total: 1 }],
    });
    const calls = stubFetch(b);
    await mount();
    fireEvent.click(screen.getByTestId('store-subtab-installed'));
    await flush();

    // ① 确认条(非模态,行内展开)
    fireEvent.click(screen.getByTestId('store-uninstall-official/forge.starter-props'));
    const bar = screen.getByTestId('store-uninstall-bar');
    expect(bar).toHaveTextContent('卸载 forge.starter-props');
    expect(screen.getByTestId('store-uninstall-confirm')).toBeInTheDocument();
    expect(document.querySelector('dialog')).toBeNull();

    // ② 确认 → 409 GOV_PROPOSAL_REQUIRED,条目转「已创建提案 + 在此批准」
    fireEvent.click(screen.getByTestId('store-uninstall-confirm'));
    await flush();
    expect(screen.getByTestId('store-uninstall-proposal')).toHaveTextContent('已创建提案 prop_1');
    expect(screen.getByTestId('store-uninstall-approve')).toBeInTheDocument();

    // ③ 批准 → PATCH proposals 后自动重发 uninstall
    b.installed = [];
    fireEvent.click(screen.getByTestId('store-uninstall-approve'));
    await flush();

    const patch = calls.find((c) => c.url.startsWith('/api/forge/proposals/') && c.method === 'PATCH');
    expect(patch).toBeDefined();
    expect(patch!.url).toBe('/api/forge/proposals/prop_1');
    expect(JSON.parse(patch!.body)).toEqual({ action: 'approve' });
    expect(calls.filter((c) => c.url === '/api/forge/store/uninstall').length).toBe(2);
    expect(screen.queryByTestId('store-uninstall-bar')).not.toBeInTheDocument();
    expect(useStoreStore.getState().installed).toHaveLength(0);
    // 卸载任务的进度文案不冒充安装
    expect(toastTitles()).toContain('卸载完成');
  });

  it('空态:如实提示尚未安装,而非留白', async () => {
    stubFetch(defaultBackend());
    await mount();
    fireEvent.click(screen.getByTestId('store-subtab-installed'));
    await flush();
    expect(screen.getByTestId('store-installed-empty')).toHaveTextContent('尚未安装任何包');
  });
});

describe('源管理面板', () => {
  it('展开列表:token 只显「已配置」徽标;启停走 PATCH;新增源 token 走 password 且提交后不回显', async () => {
    const b = defaultBackend();
    const calls = stubFetch(b);
    await mount();

    fireEvent.click(screen.getByTestId('store-sources-toggle'));
    const panel = screen.getByTestId('store-sources-panel');
    expect(within(panel).getByTestId('store-source-row-official')).toHaveTextContent('RurixForge 官方源');
    expect(screen.getByTestId('store-source-token-official')).toHaveTextContent('无 token');
    expect(screen.getByTestId('store-source-token-community')).toHaveTextContent('已配置');

    // 启停开关
    const toggle = screen.getByTestId('store-source-toggle-community');
    expect(toggle).toHaveAttribute('aria-checked', 'true');
    fireEvent.click(toggle);
    await flush();
    const patch = calls.find((c) => c.url === '/api/forge/store/sources/community' && c.method === 'PATCH');
    expect(patch).toBeDefined();
    expect(JSON.parse(patch!.body)).toEqual({ enabled: false });
    expect(screen.getByTestId('store-source-toggle-community')).toHaveAttribute('aria-checked', 'false');

    // 新增源:token 输入是 password,提交后清空且全页不出现明文
    const SECRET = 'sk-secret-store-token';
    const tokenInput = screen.getByTestId('store-source-new-token') as HTMLInputElement;
    expect(tokenInput).toHaveAttribute('type', 'password');
    fireEvent.change(screen.getByTestId('store-source-new-id'), { target: { value: 'mirror' } });
    fireEvent.change(screen.getByTestId('store-source-new-name'), { target: { value: '镜像源' } });
    fireEvent.change(screen.getByTestId('store-source-new-baseurl'), { target: { value: 'https://mirror.test/registry' } });
    fireEvent.change(tokenInput, { target: { value: SECRET } });
    fireEvent.click(screen.getByTestId('store-source-add'));
    await flush();

    const post = calls.find((c) => c.url === '/api/forge/store/sources' && c.method === 'POST');
    expect(post).toBeDefined();
    expect(JSON.parse(post!.body)).toEqual({
      id: 'mirror',
      name: '镜像源',
      baseUrl: 'https://mirror.test/registry',
      token: SECRET,
    });
    // 新行只有徽标,没有明文;输入框已清空
    expect(screen.getByTestId('store-source-token-mirror')).toHaveTextContent('已配置');
    expect((screen.getByTestId('store-source-new-token') as HTMLInputElement).value).toBe('');
    expect(document.body.textContent ?? '').not.toContain(SECRET);
    expect(document.body.innerHTML).not.toContain(SECRET);
    // store 里也不留 token(只有 hasToken 布尔)
    expect(JSON.stringify(useStoreStore.getState().sources)).not.toContain(SECRET);
  });

  it('删除源:DELETE 落到具体源', async () => {
    const calls = stubFetch(defaultBackend());
    await mount();
    fireEvent.click(screen.getByTestId('store-sources-toggle'));
    fireEvent.click(screen.getByTestId('store-source-delete-community'));
    await flush();
    expect(calls.some((c) => c.url === '/api/forge/store/sources/community' && c.method === 'DELETE')).toBe(true);
    expect(screen.queryByTestId('store-source-row-community')).not.toBeInTheDocument();
  });
});

describe('我的资产库', () => {
  it('列表 + 装进项目 + 移出库', async () => {
    const b = defaultBackend({ library: [LIB_ITEM] });
    const calls = stubFetch(b);
    await mount();
    fireEvent.click(screen.getByTestId('store-subtab-library'));
    await flush();

    const item = screen.getByTestId('store-library-item-lib_1');
    expect(item).toHaveTextContent('chair.gltf');
    expect(item).toHaveTextContent('mesh');
    expect(item).toHaveTextContent('2.0 KB');
    expect(item).toHaveTextContent('project');

    // 装进项目(可指定落地文件夹,空则按类型分流)
    fireEvent.change(screen.getByTestId('store-library-dest'), { target: { value: 'Misc' } });
    fireEvent.click(screen.getByTestId('store-library-install-lib_1'));
    await flush();
    const install = calls.find((c) => c.url === '/api/forge/store/library/lib_1:install' && c.method === 'POST');
    expect(install).toBeDefined();
    expect(JSON.parse(install!.body)).toEqual({ destFolder: 'Misc' });
    expect(toastTitles().some((t) => t.includes('已装进项目'))).toBe(true);

    // 移出库
    fireEvent.click(screen.getByTestId('store-library-remove-lib_1'));
    await flush();
    expect(calls.some((c) => c.url === '/api/forge/store/library/lib_1' && c.method === 'DELETE')).toBe(true);
    expect(screen.queryByTestId('store-library-item-lib_1')).not.toBeInTheDocument();
    expect(screen.getByTestId('store-library-empty')).toBeInTheDocument();
  });

  it('从当前项目收藏:手输路径入库;「收藏选中资产」在无选中时禁用,有选中时带上其路径', async () => {
    const b = defaultBackend();
    const calls = stubFetch(b);
    await mount();
    fireEvent.click(screen.getByTestId('store-subtab-library'));
    await flush();

    expect(screen.getByTestId('store-library-from-selection')).toBeDisabled();

    fireEvent.change(screen.getByTestId('store-library-path-input'), {
      target: { value: 'Meshes/chair.gltf' },
    });
    fireEvent.click(screen.getByTestId('store-library-add'));
    await flush();
    const post = calls.find((c) => c.url === '/api/forge/store/library' && c.method === 'POST');
    expect(post).toBeDefined();
    expect(JSON.parse(post!.body)).toEqual({ assetPath: 'Meshes/chair.gltf' });

    // assetStore 只读引用:选中一件素材后快捷收藏按钮可用
    act(() => {
      useAssetStore.setState({
        items: [{ path: 'Textures/wood.png', guid: 'g1', type: 'texture', size: 10 }],
        selectedGuid: 'g1',
      });
    });
    const quick = screen.getByTestId('store-library-from-selection');
    expect(quick).toBeEnabled();
    fireEvent.click(quick);
    await flush();
    const posts = calls.filter((c) => c.url === '/api/forge/store/library' && c.method === 'POST');
    expect(JSON.parse(posts[posts.length - 1].body)).toEqual({ assetPath: 'Textures/wood.png' });
  });
});

describe('非模态纪律', () => {
  it('整棵子树没有 <dialog>;卸载确认与源管理都是行内展开', async () => {
    stubFetch(defaultBackend({ installed: [INSTALLED_STARTER] }));
    const { container } = await mount();
    fireEvent.click(screen.getByTestId('store-sources-toggle'));
    fireEvent.click(screen.getByTestId('store-subtab-installed'));
    await flush();
    fireEvent.click(screen.getByTestId('store-uninstall-official/forge.starter-props'));
    expect(container.querySelector('dialog')).toBeNull();
    expect(document.querySelectorAll('dialog')).toHaveLength(0);
    // 源管理面板与主体同处一棵 tab 子树内(非 portal 弹窗)
    expect(container.querySelector('[data-testid="store-sources-panel"]')).not.toBeNull();
  });
});
