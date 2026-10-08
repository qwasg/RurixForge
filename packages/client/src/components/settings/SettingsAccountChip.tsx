import { useEffect } from 'react';
import { ChevronRight } from 'lucide-react';
import { formatMoney } from '@/lib/accountApi';
import { useAccountStore } from '@/lib/accountStore';
import { useSettingsStore } from '@/lib/settingsStore';
import CloudAvatar from '../account/CloudAvatar';
import AccountCard from '../shell/AccountCard';

/**
 * 设置页左下账户卡:已登录显示云账号(头像 / 昵称或邮箱 / 余额,点开账户页);
 * 未登录沿用本机账户卡,旁边给「登录」入口(D-046:反色小胶囊,同登录门主按钮)。
 */
export default function SettingsAccountChip() {
  const status = useAccountStore((st) => st.status);
  const refreshStatus = useAccountStore((st) => st.refreshStatus);
  const openAuth = useAccountStore((st) => st.openAuth);
  const setPage = useSettingsStore((st) => st.setPage);

  useEffect(() => {
    if (useAccountStore.getState().status === null) void refreshStatus();
  }, [refreshStatus]);

  const user = status?.loggedIn ? status.user : null;
  if (user) {
    return (
      <button
        type="button"
        data-testid="settings-account-chip"
        title="账户"
        onClick={() => setPage('account')}
        className="group flex min-w-0 flex-1 items-center gap-2 rounded-lg px-1 py-1 text-left transition-colors hover:bg-shell-hover"
      >
        <CloudAvatar user={user} />
        <span className="flex min-w-0 flex-1 flex-col leading-tight">
          <span data-testid="settings-account-name" className="truncate text-[12.5px] font-medium text-fg">
            {user.nickname || user.email}
          </span>
          <span data-testid="settings-account-balance" className="truncate text-[10.5px] text-fg-4">
            余额 {formatMoney(status?.balanceMicros ?? user.balanceMicros, status?.currency)}
          </span>
        </span>
        <ChevronRight
          size={12}
          aria-hidden
          className="mr-0.5 shrink-0 text-fg-4 opacity-0 transition-opacity group-hover:opacity-100"
        />
      </button>
    );
  }
  return (
    <div data-testid="settings-account-chip" className="flex min-w-0 flex-1 items-center gap-2">
      <AccountCard variant="settings" />
      <button
        type="button"
        data-testid="settings-account-login"
        onClick={openAuth}
        className="flex h-[26px] shrink-0 items-center rounded-full bg-fg px-3 text-[12px] font-medium text-shell-bg transition-opacity hover:opacity-90"
      >
        登录
      </button>
    </div>
  );
}
