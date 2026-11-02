import { useCallback, useEffect, useState } from 'react';
import { cn } from '@/lib/cn';
import {
  getAntigravityStatus,
  postAntigravityConfig,
  postAntigravityProbe,
  type AntigravityAvailability,
  type AntigravityConfigReq,
  type AntigravityQuota,
  type AntigravityStatus,
} from '@/lib/forgeApi';
import { useToastStore } from '@/lib/toastStore';
import CloudModelsSection, { ByoAdvancedSection } from './CloudModelsSection';
import ChannelConnections from './ChannelConnections';
import CustomModelsSection from './CustomModelsSection';
import { SetCard, SetH1, SetInput, SetRow, SmBtn } from './controls';

/**
 * 设置·模型页:官方订阅渠道卡 → RurixForge 云(账户 + 云端模型列表)→「高级:自定义模型」
 * 卡片页(D-049:OpenAI / Anthropic 两种 API 格式 + 上下文窗口 / 最大输出,DeepSeek 官方为内置卡)。
 * 生成服务在左侧导航「生成服务」页管理;RAG 向量检索(embedding)配置面已从本页撤下,
 * 检索维持词法档(agentd 既有配置仍生效,接口未删)。
 *
 * 下方 Antigravity 反代卡与额度条解析保留导出(订阅渠道卡的「反代配置」与测试复用)。
 */

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

export default function ModelsPage() {
  return (
    <div data-testid="settings-page-models" className="flex flex-col">
      <SetH1>模型</SetH1>
      <ChannelConnections />
      <CloudModelsSection />
      <ByoAdvancedSection>
        <CustomModelsSection />
      </ByoAdvancedSection>
    </div>
  );
}
