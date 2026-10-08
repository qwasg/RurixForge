import { RotateCcw, Save } from 'lucide-react';
import { useEffect, useState, type FormEvent, type ReactNode } from 'react';
import { Button } from '@/components/ui/button';
import { Checkbox, Field, Input, Select, Textarea } from '@/components/ui/form';
import { Card, CardHeader, ErrorBlock, FormError, LoadingBlock, Notice, PageHeader } from '@/components/ui/misc';
import { adminApi } from '@/lib/api/admin';
import { errorMessage } from '@/lib/api/client';
import type { RegistrationMode, Settings, SettingsInput } from '@/lib/api/types';
import { microsToUnits, parseIntInput, unitsToMicros } from '@/lib/format';
import { useAsync } from '@/lib/hooks';
import { useGroups, useModels } from '@/lib/queries';
import { useSettings } from '@/lib/settings';
import { useToast } from '@/lib/toast';

interface SettingsForm {
  siteName: string;
  currency: string;
  registrationMode: RegistrationMode;
  requireEmailVerify: boolean;
  signupBonus: string;
  defaultGroupId: string;
  defaultModel: string;
  maxFailoverRetries: string;
  stickyTtlSeconds: string;
  codexInstructions: string;
}

function toForm(s: Settings): SettingsForm {
  return {
    siteName: s.siteName ?? '',
    currency: s.currency ?? 'USD',
    registrationMode: s.registrationMode ?? 'open',
    requireEmailVerify: !!s.requireEmailVerify,
    signupBonus: microsToUnits(s.signupBonusMicros ?? 0),
    defaultGroupId: s.defaultGroupId ? String(s.defaultGroupId) : '',
    defaultModel: s.defaultModel ?? '',
    maxFailoverRetries: String(s.maxFailoverRetries ?? 3),
    stickyTtlSeconds: String(s.stickyTtlSeconds ?? 3600),
    codexInstructions: s.codexInstructions ?? '',
  };
}

function fromForm(f: SettingsForm, smtpEnabled: boolean): { ok: true; value: SettingsInput } | { ok: false; error: string } {
  if (!f.siteName.trim()) return { ok: false, error: '请填写站点名称' };
  if (!f.currency.trim()) return { ok: false, error: '请填写展示货币' };
  const signupBonusMicros = f.signupBonus.trim() === '' ? 0 : unitsToMicros(f.signupBonus);
  if (Number.isNaN(signupBonusMicros) || signupBonusMicros < 0) return { ok: false, error: '注册赠送额度应为非负数字' };
  const retries = parseIntInput(f.maxFailoverRetries);
  if (Number.isNaN(retries) || retries < 0 || retries > 20) return { ok: false, error: '换号重试次数应为 0–20 的整数' };
  const ttl = parseIntInput(f.stickyTtlSeconds);
  if (Number.isNaN(ttl) || ttl < 0) return { ok: false, error: '粘性会话 TTL 应为非负整数（秒）' };
  return {
    ok: true,
    value: {
      siteName: f.siteName.trim(),
      currency: f.currency.trim(),
      registrationMode: f.registrationMode,
      requireEmailVerify: smtpEnabled ? f.requireEmailVerify : false,
      signupBonusMicros,
      defaultGroupId: f.defaultGroupId ? Number(f.defaultGroupId) : null,
      defaultModel: f.defaultModel,
      maxFailoverRetries: retries,
      stickyTtlSeconds: ttl,
      codexInstructions: f.codexInstructions,
    },
  };
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <Card>
      <CardHeader title={title} />
      <div className="grid grid-cols-1 gap-4 p-4 md:grid-cols-2">{children}</div>
    </Card>
  );
}

const MODE_HINT: Record<RegistrationMode, string> = {
  open: '任何人可用邮箱注册',
  invite: '注册时必须填写邀请码（在「兑换码」中生成邀请类型）',
  closed: '关闭注册，只能由管理员创建用户',
};

export function SettingsPage() {
  const loaded = useAsync(() => adminApi.settings.get(), []);
  const groups = useGroups();
  const models = useModels();
  const ctx = useSettings();
  const toast = useToast();
  const [form, setForm] = useState<SettingsForm | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (loaded.data) setForm(toForm(loaded.data));
  }, [loaded.data]);

  if (!loaded.data || !form) {
    return (
      <div>
        <PageHeader title="系统设置" />
        <Card>{loaded.error ? <ErrorBlock error={loaded.error} onRetry={loaded.reload} /> : <LoadingBlock />}</Card>
      </div>
    );
  }

  const smtpEnabled = !!loaded.data.smtpEnabled;
  const set = (patch: Partial<SettingsForm>) => setForm((f) => (f ? { ...f, ...patch } : f));
  const modelOptions = [...new Set([...(models.data ?? []).filter((m) => m.enabled).map((m) => m.id), ...(form.defaultModel ? [form.defaultModel] : [])])];

  const onSubmit = async (e: FormEvent) => {
    e.preventDefault();
    const parsed = fromForm(form, smtpEnabled);
    if (!parsed.ok) return setError(parsed.error);
    setError(null);
    setBusy(true);
    try {
      const saved = await adminApi.settings.update(parsed.value);
      const next: Settings = saved && typeof saved === 'object' ? { ...loaded.data!, ...saved } : { ...loaded.data!, ...parsed.value };
      loaded.setData(next);
      ctx.replace(next);
      toast.success('设置已保存');
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <form onSubmit={onSubmit} noValidate>
      <PageHeader
        title="系统设置"
        description="保存后立即生效。"
        actions={
          <>
            <Button size="sm" onClick={() => setForm(toForm(loaded.data!))} disabled={busy}>
              <RotateCcw />
              还原
            </Button>
            <Button size="sm" type="submit" variant="primary" loading={busy}>
              {busy ? null : <Save />}
              保存设置
            </Button>
          </>
        }
      />
      <div className="flex max-w-4xl flex-col gap-4">
        <FormError>{error}</FormError>
        <Section title="站点">
          <Field label="站点名称" hint="显示在登录页、后台标题与客户端">
            <Input value={form.siteName} onChange={(e) => set({ siteName: e.target.value })} />
          </Field>
          <Field label="展示货币" hint="仅影响金额显示，如 USD、CNY、积分；不做汇率换算">
            <Input value={form.currency} onChange={(e) => set({ currency: e.target.value })} className="w-40" />
          </Field>
        </Section>

        <Section title="注册">
          <Field label="注册模式" hint={MODE_HINT[form.registrationMode]}>
            <Select value={form.registrationMode} onChange={(e) => set({ registrationMode: e.target.value as RegistrationMode })}>
              <option value="open">开放注册</option>
              <option value="invite">邀请制</option>
              <option value="closed">关闭注册</option>
            </Select>
          </Field>
          <Field label={`注册赠送额度（${form.currency || ctx.currency}）`} hint="新用户注册即得的余额，0 = 不赠送">
            <Input inputMode="decimal" value={form.signupBonus} onChange={(e) => set({ signupBonus: e.target.value })} />
          </Field>
          <div className="md:col-span-2">
            <Checkbox
              label="注册需验证邮箱"
              hint={smtpEnabled ? '注册时发送邮箱验证码' : '未配置 SMTP（FORGE_CLOUD_SMTP_*），无法启用邮箱验证'}
              checked={smtpEnabled && form.requireEmailVerify}
              disabled={!smtpEnabled}
              onChange={(e) => set({ requireEmailVerify: e.target.checked })}
            />
          </div>
        </Section>

        <Section title="默认值">
          <Field label="默认分组" hint="未分组的用户使用；留空则用标记为默认的分组">
            <Select value={form.defaultGroupId} onChange={(e) => set({ defaultGroupId: e.target.value })}>
              <option value="">使用标记为默认的分组</option>
              {(groups.data ?? []).map((g) => (
                <option key={g.id} value={g.id}>
                  {g.name}
                  {g.isDefault ? '（默认）' : ''}
                </option>
              ))}
            </Select>
          </Field>
          <Field label="默认模型" hint="客户端模型目录里的默认选项（/api/v1/models/catalog 的 defaultModel）">
            <Select value={form.defaultModel} onChange={(e) => set({ defaultModel: e.target.value })}>
              <option value="">未设置</option>
              {modelOptions.map((id) => (
                <option key={id} value={id}>
                  {id}
                </option>
              ))}
            </Select>
          </Field>
        </Section>

        <Section title="网关">
          <Field label="换号重试次数" hint="上游 429 / 5xx / 401 时冷却该账号并换号重试的最大次数（默认 3）">
            <Input inputMode="numeric" value={form.maxFailoverRetries} onChange={(e) => set({ maxFailoverRetries: e.target.value })} className="w-40" />
          </Field>
          <Field label="粘性会话 TTL（秒）" hint="同一用户同一会话在此时间内固定到同一上游账号（默认 3600）">
            <Input inputMode="numeric" value={form.stickyTtlSeconds} onChange={(e) => set({ stickyTtlSeconds: e.target.value })} className="w-40" />
          </Field>
        </Section>

        <Card>
          <CardHeader title="Codex 订阅" />
          <div className="flex flex-col gap-3 p-4">
            <Field label="Codex instructions（可选）">
              <Textarea
                value={form.codexInstructions}
                onChange={(e) => set({ codexInstructions: e.target.value })}
                className="min-h-56 font-mono text-xs"
                spellCheck={false}
                placeholder="留空即可"
              />
            </Field>
            <Notice>
              可选。Codex 订阅账号处理 <code className="font-mono">/v1/chat/completions</code> 请求（转换为 responses 格式）时使用：
              非空时作为 <code className="font-mono">instructions</code> 发送，客户端的 system 消息转为 developer 输入；
              留空时把客户端的 system 消息拼接作为 instructions。
            </Notice>
          </div>
        </Card>
      </div>
    </form>
  );
}
