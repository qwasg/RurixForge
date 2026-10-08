import {
  Boxes,
  ChartColumn,
  KeyRound,
  Layers,
  LayoutDashboard,
  LogOut,
  Package,
  Receipt,
  ScrollText,
  Settings,
  Ticket,
  Users,
  type LucideIcon,
} from 'lucide-react';
import { useState } from 'react';
import { NavLink, Outlet } from 'react-router-dom';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { useAuth } from '@/lib/auth';
import { useSettings } from '@/lib/settings';
import { THEME_ICON, THEME_LABEL, useTheme } from '@/lib/theme';
import { cn } from '@/lib/utils';

export interface NavItem {
  to: string;
  label: string;
  icon: LucideIcon;
  end?: boolean;
}

export const NAV_ITEMS: NavItem[] = [
  { to: '/', label: '仪表盘', icon: LayoutDashboard, end: true },
  { to: '/users', label: '用户', icon: Users },
  { to: '/groups', label: '分组', icon: Layers },
  { to: '/plans', label: '套餐', icon: Package },
  { to: '/orders', label: '订单', icon: Receipt },
  { to: '/accounts', label: '上游账号', icon: KeyRound },
  { to: '/models', label: '模型与定价', icon: Boxes },
  { to: '/redeem-codes', label: '兑换码', icon: Ticket },
  { to: '/usage', label: '用量', icon: ChartColumn },
  { to: '/settings', label: '系统设置', icon: Settings },
  { to: '/audit-logs', label: '审计日志', icon: ScrollText },
];

function Logo() {
  return (
    <svg viewBox="0 0 32 32" className="size-6 shrink-0" aria-hidden>
      <rect width="32" height="32" rx="7" className="fill-foreground" />
      <path d="M10 9h12v3.2h-8.4v3.4h7.2v3.2h-7.2V23H10z" className="fill-background" />
    </svg>
  );
}

function ThemeToggle() {
  const { mode, cycle } = useTheme();
  const Icon = THEME_ICON[mode];
  return (
    <Button variant="ghost" size="icon-sm" onClick={cycle} aria-label={`主题：${THEME_LABEL[mode]}`} title={`主题：${THEME_LABEL[mode]}（点击切换）`}>
      <Icon />
    </Button>
  );
}

export function AppLayout() {
  const { user, logout } = useAuth();
  const { siteName } = useSettings();
  const [leaving, setLeaving] = useState(false);

  const onLogout = async () => {
    setLeaving(true);
    try {
      await logout();
    } finally {
      setLeaving(false);
    }
  };

  return (
    <div className="flex h-full flex-col">
      <header className="flex h-12 shrink-0 items-center gap-3 border-b bg-card px-4">
        <div className="flex min-w-0 items-center gap-2">
          <Logo />
          <span className="truncate font-semibold">{siteName}</span>
          <Badge tone="outline">管理后台</Badge>
        </div>
        <div className="ml-auto flex items-center gap-1.5">
          <ThemeToggle />
          <span className="hidden max-w-56 truncate px-1 text-xs text-muted-foreground sm:inline" title={user?.email}>
            {user?.email}
          </span>
          <Button variant="ghost" size="sm" onClick={onLogout} loading={leaving}>
            {leaving ? null : <LogOut />}
            退出
          </Button>
        </div>
      </header>
      <div className="flex min-h-0 flex-1">
        <nav aria-label="主导航" className="w-44 shrink-0 overflow-y-auto border-r bg-card/50 p-2">
          <ul className="flex flex-col gap-0.5">
            {NAV_ITEMS.map(({ to, label, icon: Icon, end }) => (
              <li key={to}>
                <NavLink
                  to={to}
                  end={end}
                  className={({ isActive }) =>
                    cn(
                      'flex items-center gap-2.5 rounded-md px-2.5 py-1.5 text-sm text-muted-foreground transition-colors hover:bg-accent hover:text-foreground',
                      isActive && 'bg-accent font-medium text-foreground',
                    )
                  }
                >
                  <Icon className="size-4 shrink-0" />
                  {label}
                </NavLink>
              </li>
            ))}
          </ul>
        </nav>
        <main className="min-w-0 flex-1 overflow-auto">
          <div className="mx-auto max-w-[1600px] p-5">
            <Outlet />
          </div>
        </main>
      </div>
    </div>
  );
}
