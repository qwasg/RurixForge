import { useCallback, useEffect, useState } from 'react';
import { cn } from '@/lib/cn';
import {
  apiGet,
  apiPost,
  getAntigravityStatus,
  getEmbeddingStatus,
  getOpenAiCompatStatus,
  postAntigravityConfig,
  postAntigravityProbe,
  postEmbeddingConfig,
  postOpenAiCompatConfig,
  type AntigravityAvailability,
  type AntigravityConfigReq,
  type AntigravityProbeReq,
  type AntigravityQuota,
  type AntigravityStatus,
  type EmbeddingStatus,
  type OpenAiCompatStatus,
} from '@/lib/forgeApi';
import { useSystemPolling, useSystemStore } from '@/lib/systemStore';
import { useToastStore } from '@/lib/toastStore';
import { useSettingsStore } from '@/lib/settingsStore';
import CloudModelsSection, { ByoAdvancedSection } from './CloudModelsSection';
import ChannelConnections from './ChannelConnections';
import { SetCard, SetH1, SetInput, SetRow, SetToggle, SmBtn } from './controls';

/** Mock provider 行:状态来自快照模型目录(5s 轮询),不写死 available。 */
function MockProviderRow() {
  useSystemPolling();
  const checked = useSystemStore((st) => st.checked);
  const mock = useSystemStore((st) => st.catalog.find((m) => m.provider === 'mock'));
  const state = !checked && !mock ? 'checking' : mock ? (mock.availability ?? 'unknown') : 'missing';
  return (
    <SetRow
      title={mock?.label || 'Mock provider'}
      desc="模拟模型:不调用真实大模型,回复为固定内容,适合离线走通工作流"
      last
      testId="models-mock-row"
      control={
        <span
          data-testid="models-mock-state"
          className={cn(
            'flex h-[18px] items-center rounded-full px-1.5 text-[10px]',
            state === 'available' ? 'bg-sage-bg text-sage' : 'bg-shell-active text-fg-3',
          )}
        >
          {state === 'checking' ? '检测中' : state === 'missing' ? '未提供' : state}
        </span>
      }
    />
  );
}

/**
 * F7 wave.5 模型页(参考 set_page_models 渠道卡语义适配本仓面):
 * - LLM 渠道:deepseek 卡(design-snapshot availability 实测)+「配置 API Key」展开
 *   (password 输入 + 保存 → POST /api/forge/llm/key;R-5:密钥永不回显,响应只 {ok,configured});
 *   Mock provider 信息行(状态取 systemStore 轻量快照的模型目录,与状态栏同源)。
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

// ---------- Antigravity 订阅反代渠道卡 (R1 & R3) ----------

export interface AntigravityLimitBucket {
  id: string;
  label: string;
  remainingPercent: number;
  resetsAt?: number | string;
}

export function formatResetTime(value?: number | string): string {
  if (value === undefined || value === null || value === '') return '';
  const date = new Date(typeof value === 'number' ? (value < 1e11 ? value * 1000 : value) : value);
  if (Number.isNaN(date.getTime())) return '';
  return `重置 ${date.toLocaleString('zh-CN', { month: 'numeric', day: 'numeric', hour: '2-digit', minute: '2-digit' })}`;
}

export function parseAntigravityLimits(quota?: AntigravityQuota | null): AntigravityLimitBucket[] {
  if (!quota) return [];
  const buckets: AntigravityLimitBucket[] = [];
  if (quota.primary) {
    const p = quota.primary;
    const remaining = p.remainingPercent ?? (p.usedPercent !== undefined ? Math.max(0, 100 - p.usedPercent) : 100);
    buckets.push({
      id: 'primary',
      label: '主要额度',
      remainingPercent: Math.max(0, Math.min(100, Math.round(remaining))),
      resetsAt: p.resetsAt,
    });
  }
  if (quota.secondary) {
    const s = quota.secondary;
    const remaining = s.remainingPercent ?? (s.usedPercent !== undefined ? Math.max(0, 100 - s.usedPercent) : 100);
    buckets.push({
      id: 'secondary',
      label: '次要额度',
      remainingPercent: Math.max(0, Math.min(100, Math.round(remaining))),
      resetsAt: s.resetsAt,
    });
  }
  return buckets;
}

export function AntigravityRateLimitBar({ bucket }: { bucket: AntigravityLimitBucket }) {
  const remaining = bucket.remainingPercent;
  return (
    <div data-testid={`antigravity-limit-${bucket.id}`} className="flex flex-col gap-1.5 px-4 py-3">
      <div className="flex items-center text-[11.5px]">
        <span className="min-w-0 flex-1 text-fg-2">{bucket.label}</span>
        <span className="font-code text-fg-3">剩余 {remaining}%</span>
      </div>
      <div className="h-1.5 overflow-hidden rounded-full bg-shell-active">
        <div
          className={cn(
            'h-full rounded-full transition-all duration-300',
            remaining > 20 ? 'bg-sage' : remaining > 5 ? 'bg-warn' : 'bg-red-500',
          )}
          style={{ width: `${remaining}%` }}
        />
      </div>
      {formatResetTime(bucket.resetsAt) && (
        <div className="text-[10px] text-fg-4">{formatResetTime(bucket.resetsAt)}</div>
      )}
    </div>
  );
}

const ANTIGRAVITY_STATUS_EMPTY: AntigravityStatus = {
  configured: false,
  baseUrl: '',
  model: 'gemini-3.8-flash',
  keyConfigured: false,
  availability: 'needs-config',
};

const ANTIGRAVITY_PRESETS = ['gemini-3.8-flash', 'gemini-3.8-pro'];

export function AntigravityCard() {
  const [status, setStatus] = useState<AntigravityStatus>(ANTIGRAVITY_STATUS_EMPTY);
  const [expanded, setExpanded] = useState(false);
  const [baseUrl, setBaseUrl] = useState('');
  const [model, setModel] = useState('gemini-3.8-flash');
  const [key, setKey] = useState('');
  const [saving, setSaving] = useState(false);
  const [probing, setProbing] = useState(false);
  const [probeResult, setProbeResult] = useState<{ ok: boolean; latency?: number; error?: string } | null>(null);

  const refresh = useCallback(async () => {
    try {
      const s = await getAntigravityStatus();
      setStatus(s);
    } catch {
      // 保持现状
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const openForm = () => {
    setBaseUrl(status.baseUrl);
    setModel(status.model || 'gemini-3.8-flash');
    setKey('');
    setProbeResult(null);
    setExpanded((v) => !v);
  };

  const handleProbe = async () => {
    setProbing(true);
    setProbeResult(null);
    try {
      const res = await postAntigravityProbe({
        baseUrl: baseUrl.trim() || undefined,
        model: model.trim() || undefined,
        key: key.trim() || undefined,
      });
      if (res.ok) {
        setProbeResult({ ok: true, latency: res.latencyMs });
        useToastStore.getState().push('success', `Antigravity 反代连接成功 (${res.latencyMs ?? 0}ms)`);
        await refresh();
      } else {
        setProbeResult({ ok: false, error: res.error || '探针未通过' });
        useToastStore.getState().push('error', `探针失败: ${res.error || '未知错误'}`);
      }
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setProbeResult({ ok: false, error: msg });
      useToastStore.getState().push('error', `探测失败: ${msg}`);
    } finally {
      setProbing(false);
    }
  };

  const handleSave = async () => {
    const bu = baseUrl.trim();
    const m = model.trim();
    if (bu === '' || m === '' || saving) return;
    setSaving(true);
    try {
      const payload: AntigravityConfigReq = { baseUrl: bu, model: m };
      if (key.trim() !== '') payload.key = key.trim();
      const r = await postAntigravityConfig(payload);
      setKey('');
      setExpanded(false);
      useToastStore.getState().push('success', r.configured ? 'Antigravity 反代已配置' : '配置已保存');
      await refresh();
    } catch (err) {
      useToastStore.getState().push('error', `保存失败: ${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setSaving(false);
    }
  };

  const limits = parseAntigravityLimits(status.rateLimits ?? status.quota);
  const currentAvailability: AntigravityAvailability =
    status.availability ?? (status.configured ? 'available' : 'needs-config');

  return (
    <SetCard testId="channel-antigravity">
      <SetRow
        title="Antigravity 订阅反代"
        desc="Google AI / Gemini 反代配额 · 原生 Agent LLM 供应商"
        last={!expanded && limits.length === 0}
        testId="channel-antigravity-row"
        control={
          <span className="flex items-center gap-2">
            {status.latencyMs != null && status.latencyMs > 0 && (
              <span
                data-testid="antigravity-latency"
                className="font-code text-[10.5px] text-fg-3"
              >
                {status.latencyMs}ms
              </span>
            )}
            <span
              data-testid="antigravity-availability"
              className={cn(
                'flex h-[18px] items-center rounded-full px-1.5 text-[10px]',
                currentAvailability === 'available' && 'bg-sage-bg text-sage',
                currentAvailability === 'needs-config' && 'bg-warn-bg text-warn',
                (currentAvailability === 'offline' || currentAvailability === 'disconnected') && 'bg-shell-active text-fg-3',
              )}
            >
              {currentAvailability}
            </span>
            <SmBtn label="配置" testId="antigravity-config-toggle" onClick={openForm} />
          </span>
        }
      />
      {/* 额度进度条 */}
      {limits.length > 0 && (
        <div className="divide-y divide-edge border-t border-edge" data-testid="antigravity-limits">
          {limits.map((b) => (
            <AntigravityRateLimitBar key={b.id} bucket={b} />
          ))}
        </div>
      )}
      {/* 当前配置状态摘要 */}
      {!expanded && (status.baseUrl !== '' || status.model !== '' || status.keyConfigured) && (
        <SetRow
          title="当前配置"
          desc={`${status.baseUrl || '(未配置 baseUrl)'} · ${status.model || '(未配置 model)'} · key ${status.keyConfigured ? '已配置' : '未配置'}`}
          last
          testId="antigravity-status-line"
        />
      )}
      {/* 展开配置表单 */}
      {expanded && (
        <div className="flex flex-col gap-2.5 border-t border-edge px-4 py-3" data-testid="antigravity-config-form">
          <SetInput
            value={baseUrl}
            onChange={setBaseUrl}
            placeholder="baseUrl (如 http://127.0.0.1:8080 或 https://antigravity.example.com)"
            width={360}
            testId="antigravity-baseurl-input"
          />
          <div className="flex flex-col gap-1.5">
            <SetInput
              value={model}
              onChange={setModel}
              placeholder="model (如 gemini-3.8-flash / gemini-3.8-pro)"
              width={360}
              testId="antigravity-model-input"
            />
            <div className="flex items-center gap-1.5 text-[11px] text-fg-4">
              <span>快捷预设:</span>
              {ANTIGRAVITY_PRESETS.map((preset) => (
                <button
                  key={preset}
                  type="button"
                  data-testid={`antigravity-preset-${preset}`}
                  onClick={() => setModel(preset)}
                  className={cn(
                    'rounded border px-1.5 py-0.5 text-[11px] transition-colors',
                    model === preset
                      ? 'border-acc bg-acc-bg text-acc'
                      : 'border-edge bg-shell-panel text-fg-2 hover:bg-shell-hover',
                  )}
                >
                  {preset}
                </button>
              ))}
            </div>
          </div>
          <SetInput
            type="password"
            value={key}
            onChange={setKey}
            placeholder={
              status.keyConfigured
                ? 'apiKey (已配置; 留空保留既有; 只写 keystore, 永不回显)'
                : 'apiKey (留空保留既有; 只写 keystore, 永不回显)'
            }
            width={360}
            testId="antigravity-key-input"
          />
          <div className="flex flex-wrap items-center gap-2 pt-1">
            <SmBtn
              label={probing ? '测试中…' : '测试连接'}
              disabled={probing}
              testId="antigravity-probe-btn"
              onClick={() => void handleProbe()}
            />
            <SmBtn
              label={saving ? '保存中…' : '保存'}
              accent
              disabled={baseUrl.trim() === '' || model.trim() === '' || saving}
              testId="antigravity-config-save"
              onClick={() => void handleSave()}
            />
            {probeResult && (
              <span
                data-testid="antigravity-probe-feedback"
                className={cn('text-[11px]', probeResult.ok ? 'text-sage' : 'text-warn')}
              >
                {probeResult.ok
                  ? `✓ 连接正常${probeResult.latency != null ? ` (${probeResult.latency}ms)` : ''}`
                  : `✕ ${probeResult.error}`}
              </span>
            )}
          </div>
          <span className="text-[10.5px] text-fg-4">
            端点与模型持久化至本地数据目录；密钥写入加密本地 keystore，绝不回显。
          </span>
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

export default function ModelsPage() {
  const setPage = useSettingsStore((st) => st.setPage);
  return (
    <div data-testid="settings-page-models" className="flex flex-col">
      <SetH1>模型</SetH1>
      <ChannelConnections />
      <CloudModelsSection />
      <ByoAdvancedSection>
        <DeepseekCard />
        <OpenAiCompatCard />
        <EmbeddingCard />
        <SetCard><MockProviderRow /></SetCard>
      </ByoAdvancedSection>
      <div className="mt-5">
        <SetCard><SetRow title="生成服务" desc="管理图像、视频、音频与 3D 连接" last control={<SmBtn label="管理配置" testId="models-open-generation" onClick={() => setPage('generation')} />} /></SetCard>
      </div>
    </div>
  );
}
