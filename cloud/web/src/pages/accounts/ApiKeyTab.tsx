import { Eye, EyeOff } from 'lucide-react';
import { useState, type FormEvent } from 'react';
import { Button } from '@/components/ui/button';
import { Checkbox, Field, Input, Select } from '@/components/ui/form';
import { KeyValueEditor, rowsToRecord, type KvRow } from '@/components/ui/kv-editor';
import { FormError } from '@/components/ui/misc';
import {
  BASE_URL_PRESETS,
  CUSTOM_PRESET,
  isHttpUrl,
  parsePool,
  PLATFORM_LABEL,
  presetForUrl,
  type PoolForm,
} from '@/lib/accounts';
import { adminApi } from '@/lib/api/admin';
import { errorMessage } from '@/lib/api/client';
import type { Group, Platform } from '@/lib/api/types';
import { useToast } from '@/lib/toast';
import { defaultPool, PoolFields } from './shared';

const DEFAULT_PRESET = BASE_URL_PRESETS[0];

export function ApiKeyTab({ groups, onAdded, onDone }: { groups: Group[]; onAdded: () => void; onDone: () => void }) {
  const toast = useToast();
  const [platform, setPlatform] = useState<Platform>(DEFAULT_PRESET.platform);
  const [preset, setPreset] = useState<string>(DEFAULT_PRESET.id);
  const [baseUrl, setBaseUrl] = useState(DEFAULT_PRESET.baseUrl);
  const [name, setName] = useState(DEFAULT_PRESET.label);
  const [nameTouched, setNameTouched] = useState(false);
  const [apiKey, setApiKey] = useState('');
  const [showKey, setShowKey] = useState(false);
  const [supportsResponses, setSupportsResponses] = useState(DEFAULT_PRESET.supportsResponses);
  const [pool, setPool] = useState<PoolForm>(() => defaultPool(10));
  const [mapping, setMapping] = useState<KvRow[]>([]);
  const [allowedModels, setAllowedModels] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const applyPreset = (id: string) => {
    setPreset(id);
    const p = BASE_URL_PRESETS.find((x) => x.id === id);
    if (!p) return;
    setBaseUrl(p.baseUrl);
    setPlatform(p.platform);
    setSupportsResponses(p.supportsResponses);
    if (!nameTouched) setName(p.label);
  };

  const onPlatformChange = (next: Platform) => {
    setPlatform(next);
    const current = BASE_URL_PRESETS.find((x) => x.id === preset);
    if (current && current.platform !== next) {
      applyPreset(BASE_URL_PRESETS.find((x) => x.platform === next)?.id ?? CUSTOM_PRESET);
    }
    if (next === 'anthropic') setSupportsResponses(false);
  };

  const onSubmit = async (e: FormEvent) => {
    e.preventDefault();
    if (!name.trim()) return setError('请填写账号名称');
    if (!isHttpUrl(baseUrl)) return setError('Base URL 需为 http(s) 地址，且包含版本段（如 /v1）');
    if (!apiKey.trim()) return setError('请填写 API Key');
    const parsed = parsePool(pool);
    if (!parsed.ok) return setError(parsed.error);
    setError(null);
    setBusy(true);
    try {
      const acc = await adminApi.accounts.create({
        name: name.trim(),
        platform,
        baseUrl: baseUrl.trim().replace(/\/+$/, ''),
        apiKey: apiKey.trim(),
        supportsResponses: platform === 'openai' && supportsResponses,
        priority: parsed.value.priority,
        weight: parsed.value.weight,
        concurrencyLimit: parsed.value.concurrencyLimit,
        groupIds: parsed.value.groupIds,
        modelMapping: rowsToRecord(mapping),
        allowedModels: [...new Set(allowedModels.split(/[\s,，]+/).filter(Boolean))],
        proxyUrl: parsed.value.proxyUrl,
      });
      toast.success(`已添加账号「${acc?.name ?? name.trim()}」`);
      onAdded();
      onDone();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <form onSubmit={onSubmit} className="flex flex-col gap-4" noValidate>
      <div className="grid grid-cols-3 gap-3">
        <Field label="平台">
          <Select value={platform} onChange={(e) => onPlatformChange(e.target.value as Platform)}>
            <option value="openai">{PLATFORM_LABEL.openai}（及兼容接口）</option>
            <option value="anthropic">{PLATFORM_LABEL.anthropic}</option>
          </Select>
        </Field>
        <Field label="服务商预设">
          <Select value={preset} onChange={(e) => applyPreset(e.target.value)}>
            {BASE_URL_PRESETS.map((p) => (
              <option key={p.id} value={p.id}>
                {p.label}
              </option>
            ))}
            <option value={CUSTOM_PRESET}>自定义</option>
          </Select>
        </Field>
        <Field label="账号名称" required>
          <Input
            value={name}
            onChange={(e) => {
              setName(e.target.value);
              setNameTouched(true);
            }}
          />
        </Field>
        <Field label="Base URL" required className="col-span-3" hint="包含版本段，网关只拼接 /chat/completions、/responses、/messages 等后缀">
          <Input
            value={baseUrl}
            onChange={(e) => {
              setBaseUrl(e.target.value);
              setPreset(presetForUrl(e.target.value));
            }}
            className="font-mono text-xs"
            spellCheck={false}
          />
        </Field>
        <Field label="API Key" required className="col-span-3" hint="加密保存，之后只显示末 4 位">
          <div className="flex items-center gap-1.5">
            <Input
              type={showKey ? 'text' : 'password'}
              value={apiKey}
              onChange={(e) => setApiKey(e.target.value)}
              autoComplete="off"
              className="font-mono text-xs"
              spellCheck={false}
              aria-label="API Key"
            />
            <Button size="icon" variant="ghost" aria-label={showKey ? '隐藏 Key' : '显示 Key'} onClick={() => setShowKey((s) => !s)}>
              {showKey ? <EyeOff /> : <Eye />}
            </Button>
          </div>
        </Field>
        {platform === 'openai' ? (
          <div className="col-span-3">
            <Checkbox
              label="支持 /v1/responses"
              hint="上游原生提供 Responses API 时勾选（OpenAI 官方支持；多数兼容服务商不支持）"
              checked={supportsResponses}
              onChange={(e) => setSupportsResponses(e.target.checked)}
            />
          </div>
        ) : null}
      </div>

      <PoolFields value={pool} onChange={setPool} groups={groups} showWeight />
      <Field label="可用模型范围（可选）" hint="填写模型 ID，以逗号或空格分隔；留空表示不限制">
        <Input value={allowedModels} onChange={(e) => setAllowedModels(e.target.value)} className="font-mono text-xs" spellCheck={false} />
      </Field>

      <Field label="模型映射（可选）" hint="对外模型 ID → 该账号实际请求的上游模型名；未映射时用模型目录的上游模型名">
        <KeyValueEditor rows={mapping} onChange={setMapping} />
      </Field>

      <FormError>{error}</FormError>
      <div className="flex justify-end">
        <Button type="submit" variant="primary" loading={busy}>
          添加账号
        </Button>
      </div>
    </form>
  );
}
