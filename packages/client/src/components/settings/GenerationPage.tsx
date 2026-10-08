import { useCallback, useEffect, useState } from 'react';
import { Plus, RefreshCw } from 'lucide-react';
import { apiGet, apiPost, ForgeApiError } from '@/lib/forgeApi';
import { useGenStore, type GenBackendInfo } from '@/lib/genStore';
import { useToastStore } from '@/lib/toastStore';
import { genBackendLabel, LOCAL_H3_BACKEND_ID } from '@/lib/videoBackend';
import { SetCard, SetH1, SetRow, SetToggle, SmBtn } from './controls';
import './generationPage.css';

const KINDS: Record<string, string> = {
  text2img: '文生图', img2img: '图生图', 'texture-set': '贴图组', variations: '变体',
  text2video: '文生视频', image2video: '图生视频', tts: '语音合成', music: '音乐生成',
  text2mesh: '文生 3D', image2mesh: '图生 3D',
};

function capabilities(backend: GenBackendInfo): string {
  const kinds = backend.capabilities?.kinds;
  return Array.isArray(kinds) ? kinds.map((k) => KINDS[String(k)] ?? String(k)).join(' / ') : '';
}

function defaultEndpoint(backend: GenBackendInfo): string {
  const value = backend.capabilities?.defaultEndpoint;
  return typeof value === 'string' ? value : '';
}

function ConnectionForm({ backend, isNew = false, onSaved, onCancel }: {
  backend: GenBackendInfo; isNew?: boolean; onSaved: () => void; onCancel: () => void;
}) {
  const baseId = backend.adapter ?? backend.id;
  const localH3 = baseId === LOCAL_H3_BACKEND_ID;
  const [label, setLabel] = useState(backend.label ?? '');
  const [enabled, setEnabled] = useState(isNew || (backend.enabled ?? backend.configured));
  const [endpoint, setEndpoint] = useState(localH3 && !backend.endpointSet ? defaultEndpoint(backend) || 'http://127.0.0.1:8188' : '');
  const [model, setModel] = useState(backend.model ?? '');
  const [apiKey, setApiKey] = useState('');
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState('');
  const id = backend.id;
  const custom = isNew || !!backend.adapter;

  const save = async () => {
    if (saving) return;
    if (custom && !label.trim()) { setError('请输入连接名称'); return; }
    const payload: Record<string, unknown> = { id, kind: backend.kind, enabled };
    if (custom) { payload.label = label.trim(); payload.adapter = baseId; }
    if (endpoint.trim()) payload.endpoint = endpoint.trim();
    if (backend.kind === 'remote' && model.trim()) payload.model = model.trim();
    if (backend.kind === 'remote' && apiKey.trim()) payload.apiKey = apiKey.trim();
    setSaving(true);
    setError('');
    try {
      await apiPost('/api/forge/gen/backends/configure', payload);
      setApiKey('');
      useToastStore.getState().push('success', '生成服务配置已保存');
      void useGenStore.getState().loadBackends();
      onSaved();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally { setSaving(false); }
  };

  return <form className="generation-form" data-testid={`gen-form-${id}`} onSubmit={(e) => { e.preventDefault(); void save(); }}>
    <div className="generation-form-heading"><strong>{isNew ? '新增连接' : '编辑配置'}</strong><span>{genBackendLabel(baseId)}</span></div>
    <div className="generation-fields">
      {custom && <label>连接名称<input maxLength={64} value={label} onChange={(e) => setLabel(e.target.value)} placeholder="例如：我的图像服务" data-testid={`gen-name-${id}`} /></label>}
      {(backend.kind === 'remote' || localH3) && <label>服务地址<input value={endpoint} onChange={(e) => setEndpoint(e.target.value)} placeholder={backend.endpointSet ? '已配置，留空保留既有地址' : defaultEndpoint(backend) || 'https://api.example.com'} data-testid={`gen-endpoint-${id}`} /></label>}
      {backend.kind === 'remote' && <>
        <label>模型<input value={model} onChange={(e) => setModel(e.target.value)} placeholder="模型 ID（可选）" data-testid={`gen-model-${id}`} /></label>
        <label>API Key<input type="password" autoComplete="new-password" value={apiKey} onChange={(e) => setApiKey(e.target.value)} placeholder={backend.keyConfigured ? '已配置，留空保留既有密钥' : '输入服务密钥'} data-testid={`gen-apikey-${id}`} /></label>
      </>}
    </div>
    <div className="generation-form-footer"><label><SetToggle on={enabled} onChange={setEnabled} ariaLabel="启用连接" testId={`gen-enabled-${id}`} />启用连接</label><div><SmBtn label="取消" disabled={saving} onClick={onCancel} /><button type="submit" className="generation-save" data-testid={`gen-save-${id}`} disabled={saving}>{saving ? '保存中…' : '保存'}</button></div></div>
    {error && <p role="alert" className="generation-error">{error}</p>}
    {backend.kind === 'remote' && <p className="generation-note">密钥保存在本机凭证库中。编辑时留空即可保留已有地址和密钥。</p>}
  </form>;
}

function ConnectionCard({ backend, onSaved }: { backend: GenBackendInfo; onSaved: () => void }) {
  const [editing, setEditing] = useState(false);
  const enabled = backend.enabled ?? backend.configured;
  const isLocalH3 = (backend.adapter ?? backend.id) === LOCAL_H3_BACKEND_ID;
  const desc = [capabilities(backend), backend.model, !enabled && '已停用', isLocalH3 && '无需 API Key · 默认 352p / 2 秒预览'].filter(Boolean).join(' · ');
  return <div className={editing ? 'generation-connection is-editing' : 'generation-connection'}><SetCard testId={`gen-backend-${backend.id}`}>
    <SetRow title={backend.label || genBackendLabel(backend.id)} desc={desc} testId={`gen-backend-row-${backend.id}`} last={!editing} control={<span className="generation-card-actions"><span className={backend.configured ? 'generation-status is-ready' : 'generation-status'} data-testid={`gen-configured-${backend.id}`}>{backend.configured ? '已就绪' : enabled ? '待配置' : '未启用'}</span><SmBtn label={editing ? '收起' : '配置'} testId={`gen-configure-${backend.id}`} onClick={() => setEditing((v) => !v)} /></span>} />
    {editing && <ConnectionForm backend={backend} onSaved={() => { setEditing(false); onSaved(); }} onCancel={() => setEditing(false)} />}
  </SetCard></div>;
}

export default function GenerationPage() {
  const [backends, setBackends] = useState<GenBackendInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<{ message: string; offline: boolean } | null>(null);
  const [adding, setAdding] = useState(false);
  const [adapter, setAdapter] = useState('remote-openai-compatible');
  const [newId, setNewId] = useState('');
  const templates = backends.filter((b) => !b.adapter);
  const selected = templates.find((b) => b.id === adapter) ?? templates[0];
  const load = useCallback(async () => {
    setLoading(true);
    try {
      const result = await apiGet<{ backends: GenBackendInfo[] }>('/api/forge/gen/backends');
      setBackends(result.backends);
      setLoadError(null);
    } catch (err) {
      setLoadError({ message: err instanceof Error ? err.message : String(err), offline: err instanceof ForgeApiError && (err.code === 'UPSTREAM_UNREACHABLE' || err.status === 502) });
    } finally { setLoading(false); }
  }, []);
  useEffect(() => { void load(); }, [load]);

  return <div data-testid="settings-page-generation" className="generation-page">
    <SetH1>生成服务</SetH1>
    <div className="generation-toolbar"><p>配置图像、视频、音频与 3D 服务。每个连接独立保存，可随时启用或修改。</p><div><button type="button" aria-label="刷新生成服务" onClick={() => void load()} disabled={loading}><RefreshCw size={14} /></button><button type="button" className="generation-add" data-testid="gen-add" disabled={loading || !!loadError || !selected} onClick={() => { setNewId(`custom-${crypto.randomUUID()}`); setAdding(true); }}><Plus size={14} />新增连接</button></div></div>
    {loadError && <SetCard testId="gen-backends-error"><SetRow title={loadError.offline ? '生成后端服务未连接' : '生成服务加载失败'} desc={loadError.message} last control={<SmBtn label={loading ? '重试中…' : '重试'} disabled={loading} testId="gen-backends-retry" onClick={() => void load()} />} /></SetCard>}
    {adding && selected && <SetCard testId="gen-new-connection"><label className="generation-adapter">接口类型<select data-testid="gen-adapter" value={selected.id} onChange={(e) => setAdapter(e.target.value)}>{templates.map((b) => <option key={b.id} value={b.id}>{genBackendLabel(b.id)} · {capabilities(b)}</option>)}</select></label><ConnectionForm key={`${newId}:${selected.id}`} backend={{ ...selected, id: newId, adapter: selected.id, label: '', enabled: true, configured: false, endpointSet: false, keyConfigured: false, model: null }} isNew onCancel={() => setAdding(false)} onSaved={() => { setAdding(false); void load(); }} /></SetCard>}
    {!loadError && loading && backends.length === 0 && <p data-testid="gen-backends-loading" className="generation-note">加载中…</p>}
    {!loadError && !loading && backends.length === 0 && <p data-testid="gen-backends-empty" className="generation-note">暂无可用接口</p>}
    {!loadError && <div className="generation-connections">{[...backends].sort((a, b) => Number(!!b.adapter) - Number(!!a.adapter)).map((backend) => <ConnectionCard key={backend.id} backend={backend} onSaved={() => void load()} />)}</div>}
  </div>;
}
