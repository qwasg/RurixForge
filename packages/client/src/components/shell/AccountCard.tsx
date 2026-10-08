import { useEffect, useRef, useState } from 'react';
import { Bot, ChevronsUpDown, CircleUserRound, Info, Keyboard, Moon, Settings, Sun } from 'lucide-react';
import { cn } from '@/lib/cn';
import { useChatStore } from '@/lib/chatStore';
import { runCommand } from '@/lib/commands';
import { useOverlayStore } from '@/lib/overlayStore';
import { useSettingsStore, type SettingsPage } from '@/lib/settingsStore';
import { describeEngine, useSystemPolling, useSystemStore } from '@/lib/systemStore';
import { useThemeStore } from '@/lib/themeStore';
import { displayRoot, useWorkspaceStore } from '@/lib/workspaceStore';
import { MenuItem, MenuSep } from './primitives';

/**
 * 账户卡(D-040,侧栏底部与设置页左下共用):取代写死的「本 / 我的空间 / 本地用户」。
 * 名称 = 本机登录用户名(host 健康接口);副行 = Codex 套餐(已登录)或当前工作区路径;
 * 侧栏变体点开是账户菜单(设置 / 账户 / Codex 账户 / 切换主题 / 快捷键 / 关于)。
 * D-046:头像改折面底(呼应 logo 折带),侧栏卡悬停/展开露出 up-down 箭头;菜单补「账户」直达设置·账户页。
 */

export interface Identity {
  name: string;
  initial: string;
  subtitle: string;
  codexReady: boolean;
  hasUser: boolean;
}

export function useIdentity(): Identity {
  useSystemPolling();
  const checked = useSystemStore((st) => st.checked);
  const userName = useSystemStore((st) => st.health?.user?.name?.trim() ?? '');
  const engines = useSystemStore((st) => st.engines);
  const codexAccount = useChatStore((st) => st.codexAccount);
  const workspace = useWorkspaceStore((st) => st.workspaces.find((w) => w.id === st.activeWorkspaceId));
  const codex = describeEngine('codex', engines);
  const email = typeof codexAccount?.email === 'string' && codexAccount.email !== '' ? codexAccount.email : undefined;
  // 首轮探测完成前不下结论(不先亮「本地用户」再跳成真名)
  const initial = (userName.match(/[\p{L}\p{N}]/u)?.[0] ?? (checked ? '本' : '·')).toUpperCase();
  const subtitle = !checked
    ? '正在读取账户…'
    : codex.ready
      ? `Codex · ${email ?? codex.plan ?? '已登录'}`
      : workspace
        ? displayRoot(workspace.root)
        : '本机 · 未登录 Codex';
  const name = userName || (checked ? '本地用户' : '…');
  return { name, initial, subtitle, codexReady: codex.ready, hasUser: userName !== '' };
}

function openSettings(page: SettingsPage): void {
  useSettingsStore.getState().setPage(page);
  useOverlayStore.getState().open('settings');
}

function CardBody({ id }: { id: Identity }) {
  return (
    <>
      <span
        data-testid="account-avatar"
        className="forge-fold-avatar flex h-7 w-7 shrink-0 select-none items-center justify-center rounded-full text-[12px] font-medium"
      >
        {id.initial}
      </span>
      <span className="flex min-w-0 flex-1 flex-col text-left leading-tight">
        <span data-testid="account-name" className="truncate text-[12.5px] font-medium text-fg">
          {id.name}
        </span>
        <span data-testid="account-subtitle" title={id.subtitle} className="truncate text-[10.5px] text-fg-4">
          {id.subtitle}
        </span>
      </span>
    </>
  );
}

export default function AccountCard({ variant = 'sidebar' }: { variant?: 'sidebar' | 'settings' }) {
  const id = useIdentity();
  const isDark = useThemeStore((st) => st.isDark);
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setOpen(false);
    };
    document.addEventListener('mousedown', onDown);
    window.addEventListener('keydown', onKey);
    return () => {
      document.removeEventListener('mousedown', onDown);
      window.removeEventListener('keydown', onKey);
    };
  }, [open]);

  if (variant === 'settings') {
    return (
      <div data-testid="account-card" className="flex min-w-0 flex-1 items-center gap-2">
        <CardBody id={id} />
      </div>
    );
  }

  const pick = (fn: () => void) => () => {
    setOpen(false);
    fn();
  };

  return (
    <div ref={rootRef} className="relative min-w-0 flex-1">
      <button
        type="button"
        data-testid="account-card"
        aria-haspopup="menu"
        aria-expanded={open}
        title={id.hasUser ? `${id.name} · ${id.subtitle}` : id.subtitle}
        onClick={() => setOpen((v) => !v)}
        className={cn(
          'group flex w-full min-w-0 items-center gap-2 rounded-lg px-1 py-1 transition-colors hover:bg-shell-hover',
          open && 'bg-shell-hover',
        )}
      >
        <CardBody id={id} />
        <ChevronsUpDown
          size={12}
          aria-hidden
          className={cn(
            'mr-0.5 shrink-0 text-fg-4 transition-opacity group-hover:opacity-100',
            open ? 'opacity-100' : 'opacity-0',
          )}
        />
      </button>
      {open && (
        <div
          role="menu"
          data-testid="account-menu"
          className="forge-pop-in absolute bottom-full left-0 z-40 mb-1.5 w-[236px] rounded-xl border border-edge-strong bg-shell-float p-1.5 shadow-float"
        >
          <MenuItem icon={<Settings size={12} />} label="设置" onSelect={pick(() => useOverlayStore.getState().open('settings'))} />
          <MenuItem icon={<CircleUserRound size={12} />} label="账户" onSelect={pick(() => openSettings('account'))} />
          <MenuItem
            icon={<Bot size={12} />}
            label={id.codexReady ? 'Codex 账户' : '登录 Codex'}
            onSelect={pick(() => openSettings('codex'))}
          />
          <MenuItem
            icon={isDark ? <Sun size={12} /> : <Moon size={12} />}
            label={isDark ? '切换到浅色主题' : '切换到深色主题'}
            onSelect={pick(() => runCommand('theme.toggle'))}
          />
          <MenuSep />
          <MenuItem icon={<Keyboard size={12} />} label="键盘快捷键" onSelect={pick(() => runCommand('help.shortcuts'))} />
          <MenuItem icon={<Info size={12} />} label="关于 RurixForge" onSelect={pick(() => runCommand('help.about'))} />
        </div>
      )}
    </div>
  );
}
