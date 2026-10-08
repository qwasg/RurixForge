import { useId, useState, type FormEvent } from 'react';
import { Button } from '@/components/ui/button';
import { Dialog, DialogContent } from '@/components/ui/dialog';
import { Checkbox, CheckboxGroup, Field, Input, Select } from '@/components/ui/form';
import { FormError } from '@/components/ui/misc';
import { adminApi } from '@/lib/api/admin';
import { errorMessage } from '@/lib/api/client';
import type { AdminModel, ModelInput, ModelPool, Platform } from '@/lib/api/types';
import { modelFromForm, modelToForm, POOL_LABEL, POOLS, PRICE_FIELDS, REASONING_EFFORTS, type ModelForm } from '@/lib/models';
import { useSettings } from '@/lib/settings';
import { useToast } from '@/lib/toast';

interface Props {
  /** null = 关闭；'new' = 新建；AdminModel = 编辑。 */
  model: AdminModel | 'new' | null;
  onOpenChange: (open: boolean) => void;
  onSaved: () => void;
}

export function ModelDialog({ model, onOpenChange, onSaved }: Props) {
  return (
    <Dialog open={model !== null} onOpenChange={onOpenChange}>
      {model !== null ? (
        <ModelFormBody model={model === 'new' ? null : model} onSaved={onSaved} onClose={() => onOpenChange(false)} />
      ) : null}
    </Dialog>
  );
}

export function ModelFormBody({ model, onSaved, onClose }: { model: AdminModel | null; onSaved: () => void; onClose: () => void }) {
  const formId = useId();
  const toast = useToast();
  const { currency } = useSettings();
  const [form, setForm] = useState<ModelForm>(() => modelToForm(model));
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const set = (patch: Partial<ModelForm>) => setForm((f) => ({ ...f, ...patch }));

  const onSubmit = async (e: FormEvent) => {
    e.preventDefault();
    const parsed = modelFromForm(form);
    if (!parsed.ok) return setError(parsed.error);
    setError(null);
    setBusy(true);
    try {
      if (model) {
        const patch: Partial<ModelInput> = { ...parsed.value };
        delete patch.id;
        await adminApi.models.update(model.id, patch);
      } else {
        await adminApi.models.create(parsed.value);
      }
      toast.success(model ? '模型已保存' : `已创建模型 ${parsed.value.id}`);
      onSaved();
      onClose();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <DialogContent
      title={model ? `编辑模型「${model.id}」` : '新增模型'}
      size="lg"
      footer={
        <>
          <Button onClick={onClose} disabled={busy}>
            取消
          </Button>
          <Button type="submit" form={formId} variant="primary" loading={busy}>
            保存
          </Button>
        </>
      }
    >
      <form id={formId} onSubmit={onSubmit} className="flex flex-col gap-4" noValidate>
        <div className="grid grid-cols-2 gap-3">
          <Field label="模型 ID" required hint={model ? '创建后不可修改' : '客户端请求里的 model，如 gpt-5.5、deepseek-chat'}>
            <Input
              value={form.id}
              onChange={(e) => set({ id: e.target.value })}
              disabled={!!model}
              className="font-mono text-xs"
              spellCheck={false}
              autoFocus={!model}
            />
          </Field>
          <Field label="显示名" hint="留空则同模型 ID">
            <Input value={form.displayName} onChange={(e) => set({ displayName: e.target.value })} />
          </Field>
          <Field label="平台">
            <Select value={form.platform} onChange={(e) => set({ platform: e.target.value as Platform })}>
              <option value="openai">OpenAI（及兼容）</option>
              <option value="anthropic">Anthropic</option>
            </Select>
          </Field>
          <Field label="上游模型名" hint="留空则同模型 ID；账号级映射优先">
            <Input
              value={form.upstreamModel}
              onChange={(e) => set({ upstreamModel: e.target.value })}
              className="font-mono text-xs"
              spellCheck={false}
            />
          </Field>
          <Field
            label="用量池"
            hint={
              form.pool === 'forge'
                ? '平台模型：计入套餐的平台模型额度（forge 池）'
                : '第三方前沿模型：按 API 价计入套餐的第三方模型额度（api 池）'
            }
          >
            <Select value={form.pool} onChange={(e) => set({ pool: e.target.value as ModelPool })}>
              {POOLS.map((p) => (
                <option key={p} value={p}>
                  {POOL_LABEL[p]}
                </option>
              ))}
            </Select>
          </Field>
        </div>

        <fieldset className="rounded-md border p-3">
          <legend className="px-1 text-xs font-medium text-foreground/80">能力</legend>
          <div className="grid grid-cols-2 gap-3">
            <div className="col-span-2 flex flex-wrap gap-x-6 gap-y-2">
              <Checkbox label="视觉输入" checked={form.vision} onChange={(e) => set({ vision: e.target.checked })} />
              <Checkbox label="工具调用" checked={form.tools} onChange={(e) => set({ tools: e.target.checked })} />
              <Checkbox
                label="Responses API"
                hint="可经 /v1/responses 调用（Codex 引擎只列这类模型）"
                checked={form.responses}
                onChange={(e) => set({ responses: e.target.checked })}
              />
            </div>
            <Field label="思考档位" className="col-span-2">
              <CheckboxGroup
                aria-label="思考档位"
                inline
                options={REASONING_EFFORTS.map((e) => ({ value: e, label: e }))}
                value={form.reasoningEfforts}
                onChange={(reasoningEfforts) => set({ reasoningEfforts })}
              />
            </Field>
            <Field label="上下文窗口（tokens）">
              <Input inputMode="numeric" value={form.contextWindow} onChange={(e) => set({ contextWindow: e.target.value })} />
            </Field>
            <Field label="最大输出（tokens）">
              <Input inputMode="numeric" value={form.maxOutput} onChange={(e) => set({ maxOutput: e.target.value })} />
            </Field>
          </div>
        </fieldset>

        <fieldset className="rounded-md border p-3">
          <legend className="px-1 text-xs font-medium text-foreground/80">单价（{currency} / 1M tokens）</legend>
          <div className="grid grid-cols-4 gap-3">
            {PRICE_FIELDS.map(({ key, label }) => (
              <Field key={key} label={`${label}单价`}>
                <Input
                  inputMode="decimal"
                  value={form.pricing[key]}
                  onChange={(e) => set({ pricing: { ...form.pricing, [key]: e.target.value } })}
                  placeholder="0"
                  className="tabular"
                />
              </Field>
            ))}
          </div>
          <p className="mt-2 text-xs text-muted-foreground">按每百万 token 的额度单位填写，保存时换算为 micros（×1,000,000）；实际扣费再乘分组倍率。</p>
        </fieldset>

        <div className="grid grid-cols-3 items-end gap-3">
          <Checkbox label="启用" checked={form.enabled} onChange={(e) => set({ enabled: e.target.checked })} />
          <Checkbox label="设为默认模型" checked={form.isDefault} onChange={(e) => set({ isDefault: e.target.checked })} />
          <Field label="排序" hint="数值小的靠前">
            <Input inputMode="numeric" value={form.sort} onChange={(e) => set({ sort: e.target.value })} />
          </Field>
        </div>
        <FormError>{error}</FormError>
      </form>
    </DialogContent>
  );
}
