import { describe, expect, it } from 'vitest';
import type { SnapshotModel } from '@/lib/chatStore';
import { codexQuota, describeEngine, describeModel, humanDuration } from '@/lib/systemStore';

const LOCAL = describeEngine('local', []);
const MODELS: SnapshotModel[] = [
  { id: 'deepseek-chat', label: 'deepseek-chat', provider: 'deepseek', availability: 'needs-key' },
  { id: 'mock', label: 'Mock provider', provider: 'mock', availability: 'available' },
  { id: 'openai-compat', label: 'qwen2.5-7b', provider: 'openai-compat', availability: 'available' },
  { id: 'codex:gpt-5', label: 'gpt-5', provider: 'codex', availability: 'available' },
];

describe('describeModel(与 Composer 模型选择器同口径)', () => {
  it('首轮探测前且无目录:检测中(不先亮写死结论)', () => {
    expect(describeModel({ checked: false, engine: LOCAL, models: [], wantedId: null, defaultModelId: null }).state).toBe(
      'checking',
    );
  });

  it('选中模型可用 → live;缺密钥 → 模型未配置并说明原因', () => {
    const live = describeModel({ checked: true, engine: LOCAL, models: MODELS, wantedId: 'openai-compat', defaultModelId: null });
    expect(live).toMatchObject({ state: 'live', label: 'qwen2.5-7b' });
    const needsKey = describeModel({ checked: true, engine: LOCAL, models: MODELS, wantedId: 'deepseek-chat', defaultModelId: null });
    expect(needsKey.state).toBe('unconfigured');
    expect(needsKey.label).toBe('模型未配置');
    expect(needsKey.title).toContain('缺少 API Key');
  });

  it('会话未选 → 回落默认模型;默认也缺 → 首个可用真模型,再退 mock', () => {
    expect(
      describeModel({ checked: true, engine: LOCAL, models: MODELS, wantedId: null, defaultModelId: 'deepseek-chat' }).state,
    ).toBe('unconfigured');
    expect(
      describeModel({ checked: true, engine: LOCAL, models: MODELS, wantedId: null, defaultModelId: null }).label,
    ).toBe('qwen2.5-7b');
    const onlyMock = MODELS.filter((m) => m.provider !== 'openai-compat');
    expect(
      describeModel({ checked: true, engine: LOCAL, models: onlyMock, wantedId: null, defaultModelId: null }),
    ).toMatchObject({ state: 'mock', label: 'Mock provider' });
  });

  it('选中的模型不在目录里 → 模型未配置', () => {
    const r = describeModel({ checked: true, engine: LOCAL, models: MODELS, wantedId: 'ghost', defaultModelId: null });
    expect(r.state).toBe('unconfigured');
    expect(r.title).toContain('ghost');
  });

  it('Codex:未登录 → login;未指定 → 默认模型;指定 → 目录 label', () => {
    const notReady = describeEngine('codex', [{ id: 'codex', installed: true, authMode: null }]);
    expect(describeModel({ checked: true, engine: notReady, models: MODELS, wantedId: null, defaultModelId: null }).state).toBe(
      'login',
    );
    const ready = describeEngine('codex', [{ id: 'codex', authMode: 'chatgpt', planType: 'Plus' }]);
    expect(describeModel({ checked: true, engine: ready, models: MODELS, wantedId: null, defaultModelId: 'mock' })).toMatchObject({
      state: 'auto',
      label: 'Codex 默认模型',
    });
    expect(
      describeModel({ checked: true, engine: ready, models: MODELS, wantedId: 'codex:gpt-5', defaultModelId: null }).label,
    ).toBe('gpt-5');
  });
});

describe('describeEngine / codexQuota', () => {
  it('本地恒就绪;Codex 按安装与登录态给文案', () => {
    expect(LOCAL).toMatchObject({ label: '本地', ready: true });
    expect(describeEngine('codex', [{ id: 'codex', installed: false }]).label).toBe('Codex · 未安装');
    expect(describeEngine('codex', [{ id: 'codex', installed: true }]).label).toBe('Codex · 未登录');
    const ready = describeEngine('codex', [
      { id: 'codex', authMode: 'chatgpt', planType: 'Plus', rateLimits: { primary: { usedPercent: 35, resetsAt: 1_800_000_000 } } },
    ]);
    expect(ready).toMatchObject({ label: 'Codex · Plus', ready: true, plan: 'Plus' });
    expect(ready.quota).toEqual({ remaining: 65, resetsAt: 1_800_000_000, windowMins: undefined });
  });

  it('额度多种形态:usedPercent / primary / rateLimitsByLimitId;无法解析 → 空', () => {
    expect(codexQuota({ usedPercent: 10 }).remaining).toBe(90);
    expect(codexQuota({ primary: { usedPercent: 120 } }).remaining).toBe(0);
    expect(codexQuota({ rateLimitsByLimitId: { codex: { primary: { usedPercent: 40 } } } }).remaining).toBe(60);
    expect(codexQuota(null)).toEqual({});
    expect(codexQuota({ foo: 'bar' })).toEqual({});
  });

  it('Pro status uses the Codex weekly allowance rather than a legacy five-hour summary or model reserve', () => {
    const limits = { primary: { usedPercent: 0, windowDurationMins: 300 }, rateLimitsByLimitId: {
      base_model_inference: { primary: { usedPercent: 0, windowDurationMins: 10080 } },
      codex: { primary: { usedPercent: 20, windowDurationMins: 10080, resetsAt: 1900000000 } },
    } };
    const engine = describeEngine('codex', [{ id: 'codex', authMode: 'chatgpt', planType: 'pro', rateLimits: limits }]);
    expect(engine.quota).toEqual({ remaining: 80, windowMins: 10080, resetsAt: 1900000000 });
    expect(codexQuota({ primary: { usedPercent: NaN } })).toEqual({});
  });

  it('humanDuration 中文时长', () => {
    expect(humanDuration(42)).toBe('42 秒');
    expect(humanDuration(125)).toBe('2 分钟');
    expect(humanDuration(3 * 3600 + 5 * 60)).toBe('3 小时 5 分钟');
    expect(humanDuration(3 * 86400)).toBe('3 天');
    expect(humanDuration(undefined)).toBe('');
  });
});
