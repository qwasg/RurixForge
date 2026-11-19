import { useCallback, useEffect, useMemo, useState, type ReactNode } from 'react';
import { ArrowUpRight, Check, Cpu, KeyRound, Loader2, Pencil, Plus, RefreshCw, Trash2, Zap } from 'lucide-react';
import { cn } from '@/lib/cn';
import {
  apiGet,
  apiPost,
  deleteCustomModel,
  getCustomModels,
  saveCustomModel,
  testCustomModel,
  type CustomApiFormat,
  type CustomModelDraft,
  type CustomModelInfo,
  type CustomModelTestResult,
} from '@/lib/forgeApi';
import {
  ANTHROPIC_DEFAULT_MAX_OUTPUT,
  API_FORMATS,
  CONTEXT_MAX,
  CONTEXT_MIN,
  CONTEXT_PRESETS,
  DEFAULT_CONTEXT_WINDOW,
  NEW_MODEL_CONTEXT,
  OUTPUT_MAX,
  OUTPUT_PRESETS,
  apiFormatTitle,
  customEndpointUrl,
  formatWindow,
  parseKiloTokens,
  toKiloInput,
  type TokenPreset,
} from '@/lib/customModels';
import { switchToLocalModel, useCurrentLocalModelId } from '@/lib/modelSwitch';
import { useChatStore } from '@/lib/chatStore';
import { useToastStore } from '@/lib/toastStore';
import { SetToggle } from './controls';
import './customModels.css';

/**
 * D-049 设置·模型页「高级:自定义模型」卡片页。
 *
 * 每个自定义模型一张卡:API 格式(OpenAI chat.completions / Anthropic Messages)、请求地址、
 * 上下文窗口、最大输出、图片输入一目了然;卡上可测试连接、编辑、删除、直接「使用」。
 * 新增/编辑在网格里展开整行表单。数据面全走 agentd /api/forge/llm/custom-models
 * (密钥只写 keystore,清单永不回显);保存/删除后刷新模型菜单(design-snapshot)。
 * DeepSeek 官方渠道作为内置卡片排在末尾(只配 API Key)。
 */

function errorText(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

const toast = (kind: 'success' | 'error', text: string) => useToastStore.getState().push(kind, text);

function refreshModelMenu() {
  void useChatStore.getState().ensureModels(true);
}

// ---------- 小部件 ----------

type StatusKind = 'ready' | 'warn' | 'idle';

function StatusPill({ kind, children, testId }: { kind: StatusKind; children: ReactNode; testId?: string }) {
  return (
    <span className={cn('cm-status', `is-${kind}`)} data-testid={testId}>
      <i aria-hidden />
      {children}
    </span>
  );
}

function modelStatus(m: CustomModelInfo): { kind: StatusKind; text: string } {
  if (m.configured) return { kind: 'ready', text: '可用' };
  if (!m.keyConfigured) return { kind: 'warn', text: '缺少密钥' };
  return { kind: 'warn', text: '未完成' };
}

function SpecRow({ label, children, mono = false, title }: { label: string; children: ReactNode; mono?: boolean; title?: string }) {
  return (
    <div className="cm-spec-row">
      <dt>{label}</dt>
      <dd className={mono ? 'cm-mono' : undefined} title={title}>
        {children}
      </dd>
    </div>
  );
}

function TestLine({ result, testId }: { result: CustomModelTestResult; testId?: string }) {
  return (
    <p className={cn('cm-test-line', result.ok ? 'is-ok' : 'is-fail')} data-testid={testId} role="status">
      {result.ok
        ? `连接正常 · ${result.latencyMs}ms${result.reply ? ` · 回复「${result.reply}」` : ''}`
        : `连接失败:${result.error ?? '未知错误'}`}
    </p>
  );
}

function maxOutputText(m: { apiFormat: CustomApiFormat; maxOutputTokens: number | null }): string {
  if (m.maxOutputTokens) return formatWindow(m.maxOutputTokens);
  return m.apiFormat === 'anthropic' ? `${formatWindow(ANTHROPIC_DEFAULT_MAX_OUTPUT)} · 默认` : '自动';
}

/** 预设档 + 自定义(单位 K)的单选条;allowAuto 时首项为「自动」(值 null)。 */
function TokenChips({
  presets,
  value,
  onChange,
  autoLabel,
  label,
  testId,
}: {
  presets: TokenPreset[];
  value: number | null;
  onChange: (v: number | null) => void;
  autoLabel?: string;
  label: string;
  testId: string;
}) {
  const isPreset = value !== null && presets.some((p) => p.tokens === value);
  const [customOn, setCustomOn] = useState(value !== null && !isPreset);
  const [custom, setCustom] = useState(value !== null && !isPreset ? toKiloInput(value) : '');
  const pick = (v: number | null) => {
    setCustomOn(false);
    onChange(v);
  };
  return (
    <div className="cm-chips" role="radiogroup" aria-label={label} data-testid={testId}>
      {autoLabel && (
        <button
          type="button"
          role="radio"
          aria-checked={!customOn && value === null}
          className={cn('cm-chip', !customOn && value === null && 'is-selected')}
          data-testid={`${testId}-auto`}
          onClick={() => pick(null)}
        >
          {autoLabel}
        </button>
      )}
      {presets.map((p) => {
        const on = !customOn && value === p.tokens;
        return (
          <button
            key={p.label}
            type="button"
            role="radio"
            aria-checked={on}
            className={cn('cm-chip', on && 'is-selected')}
            data-testid={`${testId}-${p.label}`}
            onClick={() => pick(p.tokens)}
          >
            {p.label}
          </button>
        );
      })}
      <label className={cn('cm-chip cm-chip-custom', customOn && 'is-selected')}>
        <span>自定义</span>
        <input
          inputMode="decimal"
          value={custom}
          placeholder="数值"
          aria-label={`${label}(自定义,单位 K)`}
          data-testid={`${testId}-custom`}
          onFocus={() => {
            setCustomOn(true);
            onChange(parseKiloTokens(custom));
          }}
          onChange={(e) => {
            setCustom(e.target.value);
            setCustomOn(true);
            onChange(parseKiloTokens(e.target.value));
          }}
        />
        <span>K</span>
      </label>
    </div>
  );
}

// ---------- 表单 ----------

interface FormState {
  name: string;
  apiFormat: CustomApiFormat;
  baseUrl: string;
  model: string;
  key: string;
  contextWindow: number | null;
  maxOutputTokens: number | null;
  vision: boolean;
}

function initialForm(m?: CustomModelInfo): FormState {
  if (!m) {
    return {
      name: '',
      apiFormat: 'openai',
      baseUrl: '',
      model: '',
      key: '',
      contextWindow: NEW_MODEL_CONTEXT,
      maxOutputTokens: null,
      vision: false,
    };
  }
  return {
    name: m.name,
    apiFormat: m.apiFormat,
    baseUrl: m.baseUrl,
    model: m.model,
    key: '',
    contextWindow: m.contextWindowSet ? m.contextWindow : DEFAULT_CONTEXT_WINDOW,
    maxOutputTokens: m.maxOutputTokens,
    vision: m.vision,
  };
}

/** 前置校验(与 agentd 400 防线同口径);返回第一条问题,全部通过为 null。 */
function validate(f: FormState, isNew: boolean): string | null {
  if (f.name.trim().length > 64) return '显示名称最多 64 个字符';
  if (!/^https?:\/\//i.test(f.baseUrl.trim())) return 'Base URL 须以 http:// 或 https:// 开头';
  if (f.model.trim() === '') return '请填写模型 ID';
  if (isNew && f.key.trim() === '') return '请填写 API Key(本地服务无需鉴权时可填任意值)';
  if (f.contextWindow === null || f.contextWindow < CONTEXT_MIN || f.contextWindow > CONTEXT_MAX) {
    return '上下文窗口须在 1K–16M 之间';
  }
  if (f.maxOutputTokens !== null) {
    if (f.maxOutputTokens <= 0 || f.maxOutputTokens > OUTPUT_MAX) return '最大输出须在 1K–1M 之间';
    if (f.maxOutputTokens > f.contextWindow) return '最大输出不能超过上下文窗口';
  }
  return null;
}

function toDraft(f: FormState, id?: string): CustomModelDraft {
  const draft: CustomModelDraft = {
    name: f.name.trim(),
    apiFormat: f.apiFormat,
    baseUrl: f.baseUrl.trim(),
    model: f.model.trim(),
    contextWindow: f.contextWindow,
    maxOutputTokens: f.maxOutputTokens,
    vision: f.vision,
  };
  if (id) draft.id = id;
  if (f.key.trim() !== '') draft.key = f.key.trim();
  return draft;
}

function CustomModelForm({
  initial,
  onSaved,
  onCancel,
}: {
  initial?: CustomModelInfo;
  onSaved: (m: CustomModelInfo) => void;
  onCancel: () => void;
}) {
  const isNew = !initial;
  const [form, setForm] = useState<FormState>(() => initialForm(initial));
  const [saving, setSaving] = useState(false);
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<CustomModelTestResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [touched, setTouched] = useState(false);
  const set = <K extends keyof FormState>(key: K, value: FormState[K]) => {
    setForm((f) => ({ ...f, [key]: value }));
    setTestResult(null);
    setTouched(true);
  };
  const problem = validate(form, isNew);
  const endpoint = customEndpointUrl(form.baseUrl, form.apiFormat);
  const tid = initial ? initial.id : 'new';

  const test = async () => {
    if (testing) return;
    setTesting(true);
    setTestResult(null);
    try {
      setTestResult(await testCustomModel(toDraft(form, initial?.id)));
    } catch (err) {
      setTestResult({ ok: false, latencyMs: 0, endpoint, error: errorText(err) });
    } finally {
      setTesting(false);
    }
  };

  const save = async () => {
    if (saving || problem) return;
    setSaving(true);
    setError(null);
    try {
      const r = await saveCustomModel(toDraft(form, initial?.id));
      toast('success', isNew ? `已添加「${r.model.label}」` : `已保存「${r.model.label}」`);
      onSaved(r.model);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setSaving(false);
    }
  };

  return (
    <form
      className="cm-form"
      data-testid={`cm-form-${tid}`}
      onSubmit={(e) => {
        e.preventDefault();
        void save();
      }}
    >
      <div className="cm-form-head">
        <h3>{isNew ? '添加自定义模型' : `编辑 · ${initial.label}`}</h3>
        <span>密钥只保存在本机加密凭证库,不会回显</span>
      </div>

      <fieldset className="cm-group">
        <legend>API 格式</legend>
        <div className="cm-formats" role="radiogroup" aria-label="API 格式">
          {API_FORMATS.map((f) => {
            const on = form.apiFormat === f.id;
            return (
              <button
                key={f.id}
                type="button"
                role="radio"
                aria-checked={on}
                className={cn('cm-format', on && 'is-selected')}
                data-testid={`cm-format-${f.id}`}
                onClick={() => set('apiFormat', f.id)}
              >
                <span className="cm-radio" aria-hidden />
                <span className="cm-format-body">
                  <strong>{f.title}</strong>
                  <code>{f.path}</code>
                  <small>
                    {f.auth} · {f.hint}
                  </small>
                </span>
              </button>
            );
          })}
        </div>
      </fieldset>

      <div className="cm-fields">
        <label>
          <span className="cm-label">
            显示名称<em>可选</em>
          </span>
          <input
            value={form.name}
            maxLength={64}
            placeholder="例如:Claude 中转"
            data-testid="cm-name-input"
            onChange={(e) => set('name', e.target.value)}
          />
        </label>
        <label>
          <span className="cm-label">模型 ID</span>
          <input
            value={form.model}
            placeholder={form.apiFormat === 'anthropic' ? '例如:claude-sonnet-5-5' : '例如:qwen3-coder / gpt-4o-mini'}
            data-testid="cm-model-input"
            onChange={(e) => set('model', e.target.value)}
          />
        </label>
        <label>
          <span className="cm-label">Base URL</span>
          <input
            value={form.baseUrl}
            placeholder="https://api.example.com"
            data-testid="cm-baseurl-input"
            onChange={(e) => set('baseUrl', e.target.value)}
          />
          <small className="cm-hint">
            请求地址 <code data-testid="cm-endpoint-preview">{endpoint || '—'}</code>
          </small>
        </label>
        <label>
          <span className="cm-label">API Key</span>
          <input
            type="password"
            autoComplete="new-password"
            value={form.key}
            placeholder={initial?.keyConfigured ? '已配置,留空保留原密钥' : '输入服务密钥'}
            data-testid="cm-key-input"
            onChange={(e) => set('key', e.target.value)}
          />
          <small className="cm-hint">
            {form.apiFormat === 'anthropic' ? '以 x-api-key 发送(第三方端点同时带 Bearer)' : '以 Authorization: Bearer 发送'}
          </small>
        </label>
      </div>

      <fieldset className="cm-group">
        <legend>上下文窗口</legend>
        <TokenChips
          presets={CONTEXT_PRESETS}
          value={form.contextWindow}
          onChange={(v) => set('contextWindow', v)}
          label="上下文窗口"
          testId="cm-context"
        />
        <small className="cm-hint">
          决定会话历史的保留预算与上下文用量环的上限,按该模型实际支持的窗口填写;模型菜单里可再选更小的档。
        </small>
      </fieldset>

      <fieldset className="cm-group">
        <legend>最大输出</legend>
        <TokenChips
          presets={OUTPUT_PRESETS}
          value={form.maxOutputTokens}
          onChange={(v) => set('maxOutputTokens', v)}
          autoLabel="自动"
          label="最大输出"
          testId="cm-output"
        />
        <small className="cm-hint">
          {form.apiFormat === 'anthropic'
            ? `Anthropic 格式必须声明 max_tokens:选「自动」时按 ${formatWindow(ANTHROPIC_DEFAULT_MAX_OUTPUT)} 发送;开启思考时思考预算也计入其中。`
            : '「自动」= 请求不带 max_tokens,由服务端决定;声明后以 max_tokens 发送。'}
        </small>
      </fieldset>

      <div className="cm-toggle-row">
        <SetToggle on={form.vision} onChange={(v) => set('vision', v)} ariaLabel="支持图片输入" testId="cm-vision-toggle" />
        <div>
          <strong>支持图片输入</strong>
          <small>开启后,工具产出的图片(如 3D 生成的多视角预览)会随对话发给模型;端点不支持时请求会被拒,故默认关闭。</small>
        </div>
      </div>

      {error && (
        <p role="alert" className="cm-error" data-testid="cm-form-error">
          {error}
        </p>
      )}

      <div className="cm-form-foot">
        <div className="cm-form-test">
          <button
            type="button"
            className="cm-btn"
            disabled={problem !== null || testing}
            title={problem ?? undefined}
            data-testid="cm-test"
            onClick={() => void test()}
          >
            {testing ? <Loader2 size={13} className="animate-spin" /> : <Zap size={13} />}
            {testing ? '测试中…' : '测试连接'}
          </button>
          {testResult ? (
            <TestLine result={testResult} testId="cm-test-result" />
          ) : (
            touched &&
            problem && (
              <span className="cm-problem" data-testid="cm-problem">
                {problem}
              </span>
            )
          )}
        </div>
        <div className="cm-form-actions">
          <button type="button" className="cm-btn" disabled={saving} onClick={onCancel} data-testid="cm-cancel">
            取消
          </button>
          <button type="submit" className="cm-btn cm-primary" disabled={saving || problem !== null} data-testid="cm-save">
            {saving ? '保存中…' : isNew ? '添加模型' : '保存'}
          </button>
        </div>
      </div>
    </form>
  );
}

// ---------- 卡片 ----------

function CustomModelCard({
  model,
  current,
  onEdit,
  onChanged,
}: {
  model: CustomModelInfo;
  current: boolean;
  onEdit: () => void;
  onChanged: () => void;
}) {
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<CustomModelTestResult | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [busy, setBusy] = useState(false);
  const status = modelStatus(model);
  const id = model.id;

  const test = async () => {
    setTesting(true);
    setTestResult(null);
    try {
      setTestResult(
        await testCustomModel({
          id,
          name: model.name,
          apiFormat: model.apiFormat,
          baseUrl: model.baseUrl,
          model: model.model,
          contextWindow: model.contextWindowSet ? model.contextWindow : null,
          maxOutputTokens: model.maxOutputTokens,
          vision: model.vision,
        }),
      );
    } catch (err) {
      setTestResult({ ok: false, latencyMs: 0, endpoint: model.endpoint, error: errorText(err) });
    } finally {
      setTesting(false);
    }
  };

  const remove = async () => {
    setBusy(true);
    try {
      await deleteCustomModel(id);
      toast('success', `已删除「${model.label}」`);
      onChanged();
    } catch (err) {
      toast('error', `删除失败:${errorText(err)}`);
      setBusy(false);
    }
  };

  const use = async () => {
    setBusy(true);
    try {
      await switchToLocalModel(id);
      toast('success', `已切换到「${model.label}」`);
    } catch (err) {
      toast('error', errorText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <article className={cn('cm-card', current && 'is-current')} data-testid={`custom-model-${id}`}>
      <header className="cm-card-head">
        <span className="cm-card-icon" aria-hidden>
          <Cpu size={16} />
        </span>
        <div className="cm-card-title">
          <h3 title={model.label}>{model.label}</h3>
          <span className="cm-mono" title={model.model}>
            {model.model}
          </span>
        </div>
        <StatusPill kind={status.kind} testId={`custom-model-status-${id}`}>
          {status.text}
        </StatusPill>
      </header>

      <dl className="cm-spec">
        <SpecRow label="API 格式">{apiFormatTitle(model.apiFormat)}</SpecRow>
        <SpecRow label="请求地址" mono title={model.endpoint}>
          {model.endpoint}
        </SpecRow>
        <SpecRow label="上下文窗口">
          {formatWindow(model.contextWindow)}
          {!model.contextWindowSet && <span className="cm-dim"> · 默认</span>}
        </SpecRow>
        <SpecRow label="最大输出">{maxOutputText(model)}</SpecRow>
        <SpecRow label="图片输入">{model.vision ? '支持' : '不发送'}</SpecRow>
      </dl>

      {testResult && <TestLine result={testResult} testId={`custom-model-test-result-${id}`} />}

      {confirmDelete ? (
        <div className="cm-confirm" data-testid={`custom-model-confirm-${id}`}>
          <span>删除后密钥一并清除,正在使用它的会话需要改选其他模型。</span>
          <div>
            <button type="button" className="cm-btn" disabled={busy} onClick={() => setConfirmDelete(false)}>
              取消
            </button>
            <button
              type="button"
              className="cm-btn cm-danger"
              disabled={busy}
              data-testid={`custom-model-delete-confirm-${id}`}
              onClick={() => void remove()}
            >
              确认删除
            </button>
          </div>
        </div>
      ) : (
        <footer className="cm-card-foot">
          <div className="cm-badges">
            {model.isDefault && (
              <span className="cm-badge" title="未选择模型的会话默认使用它" data-testid={`custom-model-default-${id}`}>
                默认
              </span>
            )}
            {current && <span className="cm-badge is-accent">当前会话</span>}
          </div>
          <div className="cm-actions">
            <button
              type="button"
              className="cm-btn"
              disabled={testing || !model.keyConfigured}
              title={model.keyConfigured ? '用已保存的配置发一次最小请求' : '先配置 API Key'}
              data-testid={`custom-model-test-${id}`}
              onClick={() => void test()}
            >
              {testing ? <Loader2 size={13} className="animate-spin" /> : <Zap size={13} />}
              测试
            </button>
            <button type="button" className="cm-btn" data-testid={`custom-model-edit-${id}`} onClick={onEdit}>
              <Pencil size={13} />
              编辑
            </button>
            <button
              type="button"
              className="cm-icon-btn"
              aria-label={`删除 ${model.label}`}
              data-testid={`custom-model-delete-${id}`}
              onClick={() => setConfirmDelete(true)}
            >
              <Trash2 size={13} />
            </button>
            <button
              type="button"
              className={cn('cm-btn', current ? 'cm-current' : 'cm-use')}
              disabled={busy || !model.configured || current}
              title={model.configured ? undefined : '配置完成后才能使用'}
              data-testid={`custom-model-use-${id}`}
              onClick={() => void use()}
            >
              {current ? <Check size={13} /> : <ArrowUpRight size={13} />}
              {current ? '使用中' : '使用'}
            </button>
          </div>
        </footer>
      )}
    </article>
  );
}

// ---------- DeepSeek 官方(内置卡片) ----------

interface SnapshotModel {
  id: string;
  provider?: string;
  availability?: string;
}

function DeepseekCard({ current }: { current: boolean }) {
  const [availability, setAvailability] = useState<string>('needs-key');
  const [expanded, setExpanded] = useState(false);
  const [draft, setDraft] = useState('');
  const [saving, setSaving] = useState(false);
  const [busy, setBusy] = useState(false);

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
      toast('success', r.configured ? 'DeepSeek API Key 已保存' : '已保存,但渠道仍不可用');
      await refresh();
      refreshModelMenu();
    } catch (err) {
      toast('error', `保存失败:${errorText(err)}`);
    } finally {
      setSaving(false);
    }
  };

  const ready = availability === 'available';
  const use = async () => {
    setBusy(true);
    try {
      await switchToLocalModel('deepseek-chat');
      toast('success', '已切换到 DeepSeek');
    } catch (err) {
      toast('error', errorText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <article className={cn('cm-card', current && 'is-current')} data-testid="channel-deepseek">
      <header className="cm-card-head">
        <span className="cm-card-icon" aria-hidden>
          <KeyRound size={16} />
        </span>
        <div className="cm-card-title">
          <h3>DeepSeek 官方</h3>
          <span className="cm-mono">deepseek-chat</span>
        </div>
        <StatusPill kind={ready ? 'ready' : 'warn'} testId="deepseek-availability">
          {ready ? 'available' : 'needs-key'}
        </StatusPill>
      </header>
      <dl className="cm-spec">
        <SpecRow label="API 格式">OpenAI 兼容</SpecRow>
        <SpecRow label="请求地址" mono>
          https://api.deepseek.com/chat/completions
        </SpecRow>
        <SpecRow label="上下文窗口">64K</SpecRow>
        <SpecRow label="思考模式">开启思考时改用 deepseek-reasoner</SpecRow>
      </dl>
      {expanded && (
        <div className="cm-inline-form" data-testid="deepseek-key-form">
          <input
            type="password"
            autoComplete="new-password"
            value={draft}
            placeholder="sk-…(只写入本机凭证库,不回显)"
            data-testid="deepseek-key-input"
            onChange={(e) => setDraft(e.target.value)}
          />
          <button
            type="button"
            className="cm-btn cm-primary"
            disabled={draft.trim() === '' || saving}
            data-testid="deepseek-key-save"
            onClick={() => void save()}
          >
            {saving ? '保存中…' : '保存'}
          </button>
        </div>
      )}
      <footer className="cm-card-foot">
        <div className="cm-badges">
          <span className="cm-badge">内置渠道</span>
          {current && <span className="cm-badge is-accent">当前会话</span>}
        </div>
        <div className="cm-actions">
          <button type="button" className="cm-btn" data-testid="deepseek-key-toggle" onClick={() => setExpanded((v) => !v)}>
            <KeyRound size={13} />
            {expanded ? '收起' : '配置 API Key'}
          </button>
          <button
            type="button"
            className={cn('cm-btn', current ? 'cm-current' : 'cm-use')}
            disabled={busy || !ready || current}
            data-testid="deepseek-use"
            onClick={() => void use()}
          >
            {current ? <Check size={13} /> : <ArrowUpRight size={13} />}
            {current ? '使用中' : '使用'}
          </button>
        </div>
      </footer>
    </article>
  );
}

// ---------- 分区 ----------

export default function CustomModelsSection() {
  const [models, setModels] = useState<CustomModelInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [editing, setEditing] = useState<string | null>(null);
  const currentId = useCurrentLocalModelId();

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const r = await getCustomModels();
      setModels(r.models);
      setLoadError(null);
    } catch (err) {
      setLoadError(errorText(err));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const changed = useCallback(() => {
    setEditing(null);
    void load();
    refreshModelMenu();
  }, [load]);

  const ready = useMemo(() => models.filter((m) => m.configured).length, [models]);

  return (
    <section className="cm-section" aria-label="自定义模型" data-testid="custom-models">
      <div className="cm-section-head">
        <div>
          <h2>自定义模型</h2>
          <p>
            接入任何 OpenAI 或 Anthropic 兼容的服务。每个模型单独设置 API 格式、上下文窗口与最大输出,保存后出现在对话的模型菜单里。
          </p>
        </div>
        <div className="cm-section-tools">
          {!loading && !loadError && models.length > 0 && (
            <span className="cm-count" data-testid="custom-models-count">
              {ready}/{models.length} 可用
            </span>
          )}
          <button type="button" className="cm-icon-btn" aria-label="刷新自定义模型" disabled={loading} onClick={() => void load()}>
            <RefreshCw size={13} className={loading ? 'animate-spin' : undefined} />
          </button>
          <button
            type="button"
            className="cm-btn cm-primary"
            data-testid="custom-models-add"
            disabled={editing === 'new' || !!loadError}
            onClick={() => setEditing('new')}
          >
            <Plus size={13} />
            添加模型
          </button>
        </div>
      </div>

      {loadError && (
        <div className="cm-banner" role="alert" data-testid="custom-models-error">
          <span>自定义模型加载失败:{loadError}</span>
          <button type="button" className="cm-btn" disabled={loading} onClick={() => void load()}>
            {loading ? '重试中…' : '重试'}
          </button>
        </div>
      )}

      <div className="cm-grid">
        {editing === 'new' && <CustomModelForm onSaved={changed} onCancel={() => setEditing(null)} />}
        {!loadError && !loading && models.length === 0 && editing !== 'new' && (
          <div className="cm-empty" data-testid="custom-models-empty">
            <Cpu size={18} aria-hidden />
            <div>
              <strong>还没有自定义模型</strong>
              <span>填入服务地址、模型 ID 与密钥即可使用;两种 API 格式任选。</span>
            </div>
            <button type="button" className="cm-btn cm-primary" onClick={() => setEditing('new')}>
              <Plus size={13} />
              添加模型
            </button>
          </div>
        )}
        {models.map((m) =>
          editing === m.id ? (
            <CustomModelForm key={m.id} initial={m} onSaved={changed} onCancel={() => setEditing(null)} />
          ) : (
            <CustomModelCard
              key={m.id}
              model={m}
              current={currentId === m.id}
              onEdit={() => setEditing(m.id)}
              onChanged={changed}
            />
          ),
        )}
        <DeepseekCard current={currentId === 'deepseek-chat'} />
      </div>
    </section>
  );
}
