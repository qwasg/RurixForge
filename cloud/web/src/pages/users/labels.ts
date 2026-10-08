import type { BadgeTone } from '@/components/ui/badge';
import type { Role, UserStatus } from '@/lib/api/types';

export const ROLE_LABEL: Record<Role, string> = { admin: '管理员', user: '用户' };
export const ROLE_TONE: Record<Role, BadgeTone> = { admin: 'info', user: 'neutral' };

export const USER_STATUS_LABEL: Record<UserStatus, string> = { active: '正常', disabled: '已禁用' };
export const USER_STATUS_TONE: Record<UserStatus, BadgeTone> = { active: 'success', disabled: 'danger' };

const LEDGER_KIND_LABEL: Record<string, string> = {
  signup_bonus: '注册赠送',
  usage: '消费',
  redeem: '兑换',
  admin: '管理员调整',
  admin_adjust: '管理员调整',
  adjust: '管理员调整',
  refund: '退款',
  payment: '充值',
  topup: '充值',
};

export function ledgerKindLabel(kind: string): string {
  return LEDGER_KIND_LABEL[kind] ?? kind;
}
