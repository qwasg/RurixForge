import { useCallback, useEffect, useState } from 'react';
import {
  apiGet,
  apiPost,
  ForgeApiError,
  getEmbeddingStatus,
  getOpenAiCompatStatus,
  postEmbeddingConfig,
  postOpenAiCompatConfig,
  type EmbeddingStatus,
  type OpenAiCompatStatus,
} from '@/lib/forgeApi';
import { useToastStore } from '@/lib/toastStore';
import { SetCard, SetH1, SetInput, SetRow, SetSectionLabel, SetToggle, SmBtn } from './controls';

/**
 * F7 wave.5 模型页(参考 set_page_models 渠道卡语义适配本仓面):
 * - LLM 渠道:deepseek 卡(design-snapshot availability 实测)+「配置 API Key」展开
 *   (password 输入 + 保存 → POST /api/forge/llm/key;R-5:密钥永不回显,响应只 {ok,configured});
 *   Mock provider 信息行(恒 available,无 key 时恒绿 seam)。
 * - F8 wave.2:OpenAI-Compatible 通用渠道卡(GET status 实测 configured/baseUrl/model/keyConfigured,
 *   配置展开 baseUrl+model+key → POST /api/forge/llm/openai-compat/config;key 永不回显);
 *   模型菜单经 design-snapshot models 数据面自动纳入 openai-compat 条目(未配 needs-key 禁用)。
 * - 生成后端(F5 gen backends 平移):清单 + 配置表单(enabled toggle/endpoint/apiKey password,
 *   POST /api/forge/gen/backends/configure;密钥不回显,configured/endpointSet 布尔面)。
 *
 * 差异留痕:参考为 11 家渠道框架(自定义模型清单/编辑/删除);本仓 deepseek+mock 先行,
 * F8 落地 openai-compat 一家通用面,渠道 seam 留 RD-F7-003,不造多渠道空壳。
 */

interface SnapshotModel {
  id: string;
  label: string;
  provider?: string;
  availability?: string;
}

interface GenBackend {
  id: string;
  kind: string;
  configured: boolean;
  /** 条目 enabled 事实;缺失(旧 agentd)= 未知,表单退回不改动语义 */
  enabled?: boolean;
  endpointSet: boolean;
  /** keystore 是否有该后端密钥(只回布尔);与 configured 不同——停用时 configured=false 但 key 仍在 */
  keyConfigured?: boolean;
  /** 已配置的模型名(非密,可回显;未配为 null) */
  model?: string | null;
  /** 能力面(text2img / text2video / tts / music / text2mesh / image2mesh 等,如实显示) */
  capabilities?: {
    kinds?: string[];
    /** 供应商固定端点(如 meshy);有此值时 endpoint 留空即走官方地址 */
    defaultEndpoint?: string;
  };
}

/** 能力字符串 → 中文标注(未知能力原样显示,不臆造)。 */
const KIND_LABELS: Record<string, string> = {
  text2img: '文生图',
  'texture-set': '贴图组',
  variations: '变体',
  text2video: '文生视频',
  tts: '语音合成',
  music: '音乐生成',
  text2mesh: '文生3D',
  image2mesh: '图生3D',
};

function kindsDesc(b: GenBackend): string {
  const kinds = b.capabilities?.kinds;
  if (!Array.isArray(kinds) || kinds.length === 0) return '';
  return kinds.map((k) => KIND_LABELS[k] ?? k).join('/');
}

// ---------- LLM 渠道区 ----------

function DeepseekCard() {
  const [availability, setAvailability] = useState<string>('needs-key');
  const [expanded, setExpanded] = useState(false);
  const [draft, setDraft] = useState('');
  const [saving, setSaving] = useState(false);

  const refresh = useCallback(async () => {
    try {
      const snap = await apiGet<{ models?: { models?: SnapshotModel[] } }>('/api/forge/design-snapshot');
      const ds = snap.models?.models?.find((m) => m.provider === 'deepseek');
      setAvailability(ds?.availability ?? 'needs-key');
    } catch {
      // 快照失败保持现状(状态栏健康面已如实呈现离线)
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const save = async () => {
    const key = draft.trim();
    if (key === '' || saving) return;
    setSaving(true);
    try {
      const r = await apiPost<{ ok: boolean; configured: boolean }>('/api/forge/llm/key', { apiKey: key });
      setDraft('');
      setExpanded(false);
      useToastStore.getState().push('success', r.configured ? 'API Key 已保存' : '已保存,availability 未翻(如实)');
      await refresh();
    } catch (err) {
      useToastStore.getState().push('error', `保存失败:${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setSaving(false);
    }
  };

  return (
    <SetCard testId="channel-deepseek">
      <SetRow
        title="DeepSeek"
        desc="deepseek-chat · OpenAI 兼容端点(api.deepseek.com)"
        last={!expanded}
        testId="channel-deepseek-row"
        control={
          <span className="flex items-center gap-2">
            <span
              data-testid="deepseek-availability"
              className={
                availability === 'available'
                  ? 'flex h-[18px] items-center rounded-full bg-sage-bg px-1.5 text-[10px] text-sage'
                  : 'flex h-[18px] items-center rounded-full bg-warn-bg px-1.5 text-[10px] text-warn'
              }
            >
              {availability === 'available' ? 'available' : 'needs-key'}
            </span>
            <SmBtn
              label="配置 API Key"
              testId="deepseek-key-toggle"
              onClick={() => setExpanded((v) => !v)}
            />
          </span>
        }
      />
      {expanded && (
        <div className="flex items-center gap-2 border-t border-edge px-4 py-3" data-testid="deepseek-key-form">
          <SetInput
            type="password"
            value={draft}
            onChange={setDraft}
            placeholder="sk-…(只写 keystore,永不回显)"
            width={260}
            testId="deepseek-key-input"
          />
          <SmBtn
            label={saving ? '保存中…' : '保存'}
            accent
            disabled={draft.trim() === '' || saving}
            testId="deepseek-key-save"
            onClick={() => void save()}
          />
          <span className="text-[10.5px] text-fg-4">密钥仅写入本地 keystore,不回显</span>
        </div>
      )}
    </SetCard>
  );
}

// ---------- F8 wave.2:OpenAI-Compatible 通用渠道卡 ----------

const OAI_STATUS_EMPTY: OpenAiCompatStatus = {
  configured: false,
  baseUrl: '',
  model: '',
  keyConfigured: false,
};

function OpenAiCompatCard() {
  const [status, setStatus] = useState<OpenAiCompatStatus>(OAI_STATUS_EMPTY);
  const [expanded, setExpanded] = useState(false);
  const [baseUrl, setBaseUrl] = useState('');
  const [model, setModel] = useState('');
  const [key, setKey] = useState('');
  const [vision, setVision] = useState(false);
  const [saving, setSaving] = useState(false);

  const refresh = useCallback(async () => {
    try {
      setStatus(await getOpenAiCompatStatus());
    } catch {
      // status 拉取失败保持现状(状态栏健康面已如实呈现离线)
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const openForm = () => {
    // 展开时预填已配置 baseUrl/model/vision(key 永不回显,留空 = 保留既有)。
    setBaseUrl(status.baseUrl);
    setModel(status.model);
    setVision(status.vision ?? false);
    setKey('');
    setExpanded((v) => !v);
  };

  const save = async () => {
    const bu = baseUrl.trim();
    const m = model.trim();
    if (bu === '' || m === '' || saving) return;
    setSaving(true);
    try {
      const payload: { baseUrl: string; model: string; key?: string; vision: boolean } = {
        baseUrl: bu,
        model: m,
        vision,
      };
      if (key.trim() !== '') payload.key = key.trim();
      const r = await postOpenAiCompatConfig(payload);
      setKey('');
      setExpanded(false);
      useToastStore
        .getState()
        .push('success', r.configured ? 'OpenAI-Compatible 已配置' : '已保存,key 未配置(如实)');
      await refresh();
    } catch (err) {
      useToastStore.getState().push('error', `保存失败:${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setSaving(false);
    }
  };

  return (
    <SetCard testId="channel-openai-compat">
      <SetRow
        title="OpenAI-Compatible"
        desc="通用 OpenAI 兼容端点(openai/vllm/ollama 等)· chat-completions 同形态"
        last={!expanded}
        testId="channel-openai-compat-row"
        control={
          <span className="flex items-center gap-2">
            <span
              data-testid="oai-availability"
              className={
                status.configured
                  ? 'flex h-[18px] items-center rounded-full bg-sage-bg px-1.5 text-[10px] text-sage'
                  : 'flex h-[18px] items-center rounded-full bg-warn-bg px-1.5 text-[10px] text-warn'
              }
            >
              {status.configured ? 'available' : 'needs-key'}
            </span>
            <SmBtn label="配置" testId="oai-config-toggle" onClick={openForm} />
          </span>
        }
      />
      {!expanded && (status.baseUrl !== '' || status.model !== '' || status.keyConfigured) && (
        <SetRow
          title="当前配置"
          desc={`${status.baseUrl || '(未配置 baseUrl)'} · ${status.model || '(未配置 model)'} · key ${status.keyConfigured ? '已配置' : '未配置'} · 图片输入${status.vision ? '已开' : '关'}`}
          last
          testId="oai-status-line"
        />
      )}
      {expanded && (
        <div className="flex flex-col gap-2 border-t border-edge px-4 py-3" data-testid="oai-config-form">
          <SetInput
            value={baseUrl}
            onChange={setBaseUrl}
            placeholder="baseUrl(如 http://127.0.0.1:8000;调用时拼 /v1/chat/completions)"
            width={360}
            testId="oai-baseurl-input"
          />
          <SetInput
            value={model}
            onChange={setModel}
            placeholder="model(如 qwen2.5-7b / gpt-4o-mini)"
            width={360}
            testId="oai-model-input"
          />
          <SetInput
            type="password"
            value={key}
            onChange={setKey}
            placeholder="apiKey(留空保留既有;只写 keystore,永不回显)"
            width={360}
            testId="oai-key-input"
          />
          <label className="flex items-center gap-2 text-[12px] text-fg-2">
            <SetToggle on={vision} onChange={setVision} testId="oai-vision-toggle" />
            该模型支持图片输入
          </label>
          <span className="text-[10.5px] text-fg-4">
            开启后,工具产出的图片(如 3D 生成的多视角预览)会随对话回注给模型;端点探不出模型是否支持,
            填错只会让请求被拒,故默认关闭
          </span>
          <div className="flex items-center gap-2">
            <SmBtn
              label={saving ? '保存中…' : '保存'}
              accent
              disabled={baseUrl.trim() === '' || model.trim() === '' || saving}
              testId="oai-config-save"
              onClick={() => void save()}
            />
            <span className="text-[10.5px] text-fg-4">baseUrl/model 落本地配置;密钥仅写入本地 keystore,不回显</span>
          </div>
        </div>
      )}
    </SetCard>
  );
}

// ---------- F10:Embedding 渠道卡(RAG 向量档;照 OpenAiCompatCard) ----------

const EMBED_STATUS_EMPTY: EmbeddingStatus = {
  configured: false,
  baseUrl: '',
  model: '',
  keyConfigured: false,
};

function EmbeddingCard() {
  const [status, setStatus] = useState<EmbeddingStatus>(EMBED_STATUS_EMPTY);
  const [expanded, setExpanded] = useState(false);
  const [baseUrl, setBaseUrl] = useState('');
  const [model, setModel] = useState('');
  const [key, setKey] = useState('');
  const [saving, setSaving] = useState(false);

  const refresh = useCallback(async () => {
    try {
      setStatus(await getEmbeddingStatus());
    } catch {
      // status 拉取失败保持现状(状态栏健康面已如实呈现离线)
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const openForm = () => {
    setBaseUrl(status.baseUrl);
    setModel(status.model);
    setKey('');
    setExpanded((v) => !v);
  };

  const save = async () => {
    const bu = baseUrl.trim();
    const m = model.trim();
    if (bu === '' || m === '' || saving) return;
    setSaving(true);
    try {
      const payload: { baseUrl: string; model: string; key?: string } = { baseUrl: bu, model: m };
      if (key.trim() !== '') payload.key = key.trim();
      const r = await postEmbeddingConfig(payload);
      setKey('');
      setExpanded(false);
      useToastStore
        .getState()
        .push('success', r.configured ? 'Embedding 渠道已配置(检索升级混合档)' : '已保存,key 未配置(如实)');
      await refresh();
    } catch (err) {
      useToastStore.getState().push('error', `保存失败:${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setSaving(false);
    }
  };

  return (
    <SetCard testId="channel-embedding">
      <SetRow
        title="Embedding(RAG 检索)"
        desc="OpenAI 兼容 /v1/embeddings(BGE-M3/text-embedding-3 等)· 未配置时检索走词法档"
        last={!expanded}
        testId="channel-embedding-row"
        control={
          <span className="flex items-center gap-2">
            <span
              data-testid="embed-availability"
              className={
                status.configured
                  ? 'flex h-[18px] items-center rounded-full bg-sage-bg px-1.5 text-[10px] text-sage'
                  : 'flex h-[18px] items-center rounded-full bg-warn-bg px-1.5 text-[10px] text-warn'
              }
            >
              {status.configured ? 'hybrid' : 'lexical'}
            </span>
            <SmBtn label="配置" testId="embed-config-toggle" onClick={openForm} />
          </span>
        }
      />
      {!expanded && (status.baseUrl !== '' || status.model !== '' || status.keyConfigured) && (
        <SetRow
          title="当前配置"
          desc={`${status.baseUrl || '(未配置 baseUrl)'} · ${status.model || '(未配置 model)'} · key ${status.keyConfigured ? '已配置' : '未配置'}`}
          last
          testId="embed-status-line"
        />
      )}
      {expanded && (
        <div className="flex flex-col gap-2 border-t border-edge px-4 py-3" data-testid="embed-config-form">
          <SetInput
            value={baseUrl}
            onChange={setBaseUrl}
            placeholder="baseUrl(如 https://api.siliconflow.cn;调用时拼 /v1/embeddings)"
            width={360}
            testId="embed-baseurl-input"
          />
          <SetInput
            value={model}
            onChange={setModel}
            placeholder="model(如 BAAI/bge-m3 / text-embedding-3-small)"
            width={360}
            testId="embed-model-input"
          />
          <SetInput
            type="password"
            value={key}
            onChange={setKey}
            placeholder="apiKey(留空保留既有;只写 keystore,永不回显)"
            width={360}
            testId="embed-key-input"
          />
          <div className="flex items-center gap-2">
            <SmBtn
              label={saving ? '保存中…' : '保存'}
              accent
              disabled={baseUrl.trim() === '' || model.trim() === '' || saving}
              testId="embed-config-save"
              onClick={() => void save()}
            />
            <span className="text-[10.5px] text-fg-4">配置后 RAG 检索自动升级「向量+词法」混合档;密钥仅写入本地 keystore</span>
          </div>
        </div>
      )}
    </SetCard>
  );
}

// ---------- 生成后端区(F5 平移) ----------

function GenBackendCard({ backend, onSaved }: { backend: GenBackend; onSaved: () => void }) {
  // 旧 agentd 不回 enabled:此时按「已配置即启用」推断,避免把未知当成 false 误停后端。
  const backendEnabled = backend.enabled ?? backend.configured;
  const [expanded, setExpanded] = useState(false);
  const [enabled, setEnabled] = useState(backendEnabled);
  const [endpoint, setEndpoint] = useState('');
  const [model, setModel] = useState(backend.model ?? '');
  const [apiKey, setApiKey] = useState('');
  const [saving, setSaving] = useState(false);

  // configure 的 enabled 是无条件覆盖,故展开时必须按清单事实重置,
  // 否则「只改 key」的保存会把停用的后端悄悄改回启用。
  // endpoint/apiKey 按契约不回显,恒空 = 保留既有。
  const toggleForm = () => {
    if (!expanded) {
      setEnabled(backendEnabled);
      setModel(backend.model ?? '');
      setEndpoint('');
      setApiKey('');
    }
    setExpanded((v) => !v);
  };

  const save = async () => {
    if (saving) return;
    setSaving(true);
    try {
      const payload: Record<string, unknown> = { id: backend.id, kind: backend.kind, enabled };
      if (endpoint.trim() !== '') payload.endpoint = endpoint.trim();
      if (model.trim() !== '') payload.model = model.trim();
      if (apiKey.trim() !== '') payload.apiKey = apiKey.trim();
      await apiPost('/api/forge/gen/backends/configure', payload);
      setApiKey('');
      setExpanded(false);
      useToastStore.getState().push('success', `生成后端 ${backend.id} 已保存`);
      onSaved();
    } catch (err) {
      useToastStore.getState().push('error', `保存失败:${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setSaving(false);
    }
  };

  const abilities = kindsDesc(backend);
  const descParts = [`kind=${backend.kind}`];
  if (abilities !== '') descParts.push(abilities);
  descParts.push(backendEnabled ? '已启用' : '已停用');
  const defaultEndpoint = backend.capabilities?.defaultEndpoint;
  if (backend.kind === 'remote') {
    // 有官方缺省端点的供应商,endpoint 空不等于「未配置」——别把可用状态说成缺件。
    descParts.push(
      backend.endpointSet
        ? 'endpoint 已配置'
        : defaultEndpoint
          ? `endpoint ${defaultEndpoint}(缺省)`
          : 'endpoint 未配置',
    );
    descParts.push(`key ${backend.keyConfigured ? '已配置' : '未配置'}`);
    if (backend.model) descParts.push(`model=${backend.model}`);
  }
  return (
    <SetCard testId={`gen-backend-${backend.id}`}>
      <SetRow
        title={backend.id}
        desc={descParts.join(' · ')}
        last={!expanded}
        testId={`gen-backend-row-${backend.id}`}
        control={
          <span className="flex items-center gap-2">
            <span
              data-testid={`gen-configured-${backend.id}`}
              className={
                backend.configured
                  ? 'flex h-[18px] items-center rounded-full bg-sage-bg px-1.5 text-[10px] text-sage'
                  : 'flex h-[18px] items-center rounded-full bg-shell-active px-1.5 text-[10px] text-fg-3'
              }
            >
              {backend.configured ? 'configured' : '未配置'}
            </span>
            <SmBtn label="配置" testId={`gen-configure-${backend.id}`} onClick={toggleForm} />
          </span>
        }
      />
      {expanded && (
        <div className="flex flex-col gap-2 border-t border-edge px-4 py-3" data-testid={`gen-form-${backend.id}`}>
          <label className="flex items-center gap-2 text-[12px] text-fg-2">
            <SetToggle on={enabled} onChange={setEnabled} testId={`gen-enabled-${backend.id}`} />
            启用该后端
          </label>
          {backend.kind === 'remote' && (
            <>
              <SetInput
                value={endpoint}
                onChange={setEndpoint}
                placeholder={
                  backend.endpointSet
                    ? 'endpoint(已配置,不回显;留空保留既有)'
                    : defaultEndpoint
                      ? `endpoint(留空即走官方 ${defaultEndpoint})`
                      : 'endpoint(如 https://api.example.com)'
                }
                width={320}
                testId={`gen-endpoint-${backend.id}`}
              />
              <SetInput
                value={model}
                onChange={setModel}
                placeholder="model(可选;留空保留既有,请求 body.model 透传)"
                width={320}
                testId={`gen-model-${backend.id}`}
              />
              <SetInput
                type="password"
                value={apiKey}
                onChange={setApiKey}
                placeholder={backend.keyConfigured ? 'apiKey(已配置,不回显;留空保留既有)' : 'apiKey(远程后端必填)'}
                width={320}
                testId={`gen-apikey-${backend.id}`}
              />
            </>
          )}
          <div>
            <SmBtn
              label={saving ? '保存中…' : '保存'}
              accent
              disabled={saving}
              testId={`gen-save-${backend.id}`}
              onClick={() => void save()}
            />
          </div>
        </div>
      )}
    </SetCard>
  );
}

export default function ModelsPage() {
  const [backends, setBackends] = useState<GenBackend[]>([]);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<{ message: string; offline: boolean } | null>(null);

  const loadBackends = useCallback(async () => {
    setLoading(true);
    try {
      const r = await apiGet<{ backends: GenBackend[] }>('/api/forge/gen/backends');
      setBackends(r.backends);
      setLoadError(null);
    } catch (err) {
      // 502 UPSTREAM_UNREACHABLE = agentd 没起,与「清单本身出错」是两码事,分开说。
      const offline = err instanceof ForgeApiError && (err.code === 'UPSTREAM_UNREACHABLE' || err.status === 502);
      setLoadError({ message: err instanceof Error ? err.message : String(err), offline });
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void loadBackends();
  }, [loadBackends]);

  return (
    <div data-testid="settings-page-models" className="flex flex-col">
      <SetH1>模型</SetH1>
      <SetSectionLabel>LLM 渠道</SetSectionLabel>
      <div className="flex flex-col gap-3">
        <DeepseekCard />
        <OpenAiCompatCard />
        <EmbeddingCard />
        <SetCard>
          <SetRow
            title="Mock provider"
            desc="无密钥恒绿 seam(不触网不触 MCP)"
            last
            control={
              <span className="flex h-[18px] items-center rounded-full bg-sage-bg px-1.5 text-[10px] text-sage">
                available
              </span>
            }
          />
        </SetCard>
      </div>
      <SetSectionLabel>生成后端</SetSectionLabel>
      <div className="flex flex-col gap-3">
        {loadError && (
          <SetCard testId="gen-backends-error">
            <SetRow
              title={loadError.offline ? '生成后端服务未连接' : '生成后端清单加载失败'}
              desc={
                loadError.offline
                  ? `agentd 未运行或不可达,配置面暂不可用。启动 agentd 后点重试。(${loadError.message})`
                  : loadError.message
              }
              last
              control={
                <SmBtn
                  label={loading ? '重试中…' : '重试'}
                  disabled={loading}
                  testId="gen-backends-retry"
                  onClick={() => void loadBackends()}
                />
              }
            />
          </SetCard>
        )}
        {!loadError && loading && backends.length === 0 && (
          <div className="text-[12px] text-fg-4" data-testid="gen-backends-loading">
            加载中…
          </div>
        )}
        {!loadError && !loading && backends.length === 0 && (
          <div className="text-[12px] text-fg-4" data-testid="gen-backends-empty">
            后端注册表为空
          </div>
        )}
        {backends.map((b) => (
          <GenBackendCard key={b.id} backend={b} onSaved={() => void loadBackends()} />
        ))}
      </div>
    </div>
  );
}
