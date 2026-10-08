import { useAccountStore } from '@/lib/accountStore';
import { useOverlayStore } from '@/lib/overlayStore';
import { useSettingsStore, type SettingsPage } from '@/lib/settingsStore';

/**
 * agent.failed 的云模式失败码 → 可操作的引导(15 §8.5 / §11.4)。未知码返回 null,由调用方沿用原始错误行。
 */

export type GuidanceAction = 'recharge' | 'login' | 'pickModel' | 'billing';

export interface CloudGuidance {
  text: string;
  action?: { kind: GuidanceAction; label: string };
}

export function cloudGuidance(code: string | undefined | null): CloudGuidance | null {
  switch (code) {
    case 'INSUFFICIENT_BALANCE':
      return { text: '余额或套餐额度不足', action: { kind: 'recharge', label: '查看账户' } };
    case 'INCLUDED_USAGE_EXHAUSTED':
      return {
        text: '本周期套餐内额度已用尽',
        action: { kind: 'billing', label: '查看用量' },
      };
    case 'SPEND_LIMIT_REACHED':
      return {
        text: '已达到本周期用量上限',
        action: { kind: 'billing', label: '查看用量' },
      };
    case 'CLOUD_LOGIN_REQUIRED':
    case 'CLOUD_UNAUTHORIZED':
      return { text: '需要登录 RurixForge 云账号', action: { kind: 'login', label: '登录' } };
    case 'MODEL_NOT_ALLOWED':
    case 'MODEL_NOT_CONFIGURED':
      return { text: '当前模型不可用', action: { kind: 'pickModel', label: '选择模型' } };
    case 'RATE_LIMITED':
      return { text: '请求过于频繁，请稍后再试' };
    case 'NO_AVAILABLE_ACCOUNT':
    case 'UPSTREAM_ERROR':
    case 'CLOUD_UNREACHABLE':
      return { text: '云端暂时不可用，请稍后重试' };
    default:
      return null;
  }
}

function openSettings(page: SettingsPage): void {
  useSettingsStore.getState().setPage(page);
  useOverlayStore.getState().open('settings');
}

function runAction(kind: GuidanceAction): void {
  if (kind === 'login') useAccountStore.getState().openAuth();
  else if (kind === 'recharge') openSettings('account');
  else if (kind === 'billing') openSettings('billing');
  // 模型选择器没有外部打开钩子(本地 open 态),退到设置 → 模型页
  else openSettings('models');
}

export default function CloudErrorGuidance({
  code,
  error,
}: {
  code: string | undefined;
  error?: string;
}) {
  const guidance = cloudGuidance(code);
  if (!guidance) return null;
  const detail = error && error.trim() !== '' && error !== guidance.text ? error : null;
  return (
    <div
      data-testid="assistant-error-guidance"
      data-code={code}
      className="mt-0.5 flex items-center gap-2.5 rounded-lg border border-edge bg-shell-sunk px-3 py-2"
    >
      <div className="flex min-w-0 flex-1 flex-col gap-0.5">
        <span data-testid="assistant-error-guidance-text" className="text-[12.5px] text-fg">
          {guidance.text}
        </span>
        {detail && (
          <span data-testid="assistant-error-detail" className="truncate text-[11px] text-fg-4" title={detail}>
            {detail}
          </span>
        )}
      </div>
      {guidance.action && (
        <button
          type="button"
          data-testid={`assistant-error-action-${guidance.action.kind}`}
          onClick={() => runAction(guidance.action!.kind)}
          className="flex h-[26px] shrink-0 items-center rounded-md border border-acc bg-acc px-2.5 text-[12px] text-fg-inv transition-colors hover:bg-acc-soft"
        >
          {guidance.action.label}
        </button>
      )}
    </div>
  );
}
