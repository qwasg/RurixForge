import { useCallback, useEffect, useState } from 'react';
import { apiGet, apiPost, getOpenAiCompatStatus, postOpenAiCompatConfig, type OpenAiCompatStatus } from '@/lib/forgeApi';
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
  endpointSet: boolean;
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
    // 展开时预填已配置 baseUrl/model(key 永不回显,留空 = 保留既有)。
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
          desc={`${status.baseUrl || '(未配置 baseUrl)'} · ${status.model || '(未配置 model)'} · key ${status.keyConfigured ? '已配置' : '未配置'}`}
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

// ---------- 生成后端区(F5 平移) ----------

function GenBackendCard({ backend, onSaved }: { backend: GenBackend; onSaved: () => void }) {
  const [expanded, setExpanded] = useState(false);
  const [enabled, setEnabled] = useState(true);
  const [endpoint, setEndpoint] = useState('');
  const [apiKey, setApiKey] = useState('');
  const [saving, setSaving] = useState(false);

  const save = async () => {
    if (saving) return;
    setSaving(true);
    try {
      const payload: Record<string, unknown> = { id: backend.id, kind: backend.kind, enabled };
      if (endpoint.trim() !== '') payload.endpoint = endpoint.trim();
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

  return (
    <SetCard testId={`gen-backend-${backend.id}`}>
      <SetRow
        title={backend.id}
        desc={`kind=${backend.kind} · endpoint ${backend.endpointSet ? '已配置' : '未配置'}`}
        last={!expanded}
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
            <SmBtn label="配置" testId={`gen-configure-${backend.id}`} onClick={() => setExpanded((v) => !v)} />
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
                placeholder="endpoint(留空保留既有)"
                width={320}
                testId={`gen-endpoint-${backend.id}`}
              />
              <SetInput
                type="password"
                value={apiKey}
                onChange={setApiKey}
                placeholder="apiKey(留空保留既有;不回显)"
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
  const [loadError, setLoadError] = useState<string | null>(null);

  const loadBackends = useCallback(async () => {
    try {
      const r = await apiGet<{ backends: GenBackend[] }>('/api/forge/gen/backends');
      setBackends(r.backends);
      setLoadError(null);
    } catch (err) {
      setLoadError(err instanceof Error ? err.message : String(err));
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
          <div className="text-[12px] text-fg-3" data-testid="gen-backends-error">
            生成后端清单加载失败:{loadError}
          </div>
        )}
        {!loadError && backends.length === 0 && (
          <div className="text-[12px] text-fg-4">加载中…</div>
        )}
        {backends.map((b) => (
          <GenBackendCard key={b.id} backend={b} onSaved={() => void loadBackends()} />
        ))}
      </div>
    </div>
  );
}
