import { useState } from 'react';
import { ArrowLeft, BookOpen, Boxes, Info, Palette, Search, Sparkles } from 'lucide-react';
import { cn } from '@/lib/cn';
import { useOverlayStore } from '@/lib/overlayStore';
import { SETTINGS_PAGES, useSettingsStore, type SettingsPage } from '@/lib/settingsStore';
import AboutPage from './AboutPage';
import AgentPage from './AgentPage';
import AppearancePage from './AppearancePage';
import ModelsPage from './ModelsPage';
import SkillsPage from './SkillsPage';

/**
 * F7 wave.5 设置全屏覆盖(参考 ui/settings.rs render_settings):
 * 盖在 36px titlebar 之下(absolute top-9 inset-x-0 bottom-0,bg 实体);
 * 左 240 导航(搜索框 + 返回 + 五页行[激活 accent_bg + accent 文] + 底部用户卡);
 * 内容区滚动居中,max-w 720(外观页 900)。当前页持久化 forge:settingsPage。
 * 开关:overlayStore.settings(Esc 全关互斥既有)。
 */

const PAGE_ICONS: Record<SettingsPage, typeof Palette> = {
  appearance: Palette,
  agent: Sparkles,
  models: Boxes,
  skills: BookOpen,
  about: Info,
};

function NavRow({
  id,
  label,
  active,
  onPick,
}: {
  id: SettingsPage;
  label: string;
  active: boolean;
  onPick: (p: SettingsPage) => void;
}) {
  const Icon = PAGE_ICONS[id];
  return (
    <button
      type="button"
      data-testid={`settings-nav-${id}`}
      onClick={() => onPick(id)}
      className={cn(
        'flex h-[28px] w-full items-center gap-2 rounded-md px-2 text-left text-[12.5px] transition-colors',
        active ? 'bg-acc-bg text-acc' : 'text-fg-2 hover:bg-shell-hover',
      )}
    >
      <Icon size={13} className="shrink-0" />
      <span className="min-w-0 flex-1 truncate">{label}</span>
    </button>
  );
}

export default function SettingsOverlay() {
  const open = useOverlayStore((st) => st.settings);
  const close = useOverlayStore((st) => st.close);
  const page = useSettingsStore((st) => st.page);
  const setPage = useSettingsStore((st) => st.setPage);
  const [query, setQuery] = useState('');

  if (!open) return null;

  const q = query.trim().toLowerCase();
  const pages = SETTINGS_PAGES.filter((p) => q === '' || p.label.toLowerCase().includes(q));

  return (
    <div
      data-testid="settings-overlay"
      className="absolute inset-x-0 bottom-0 top-[36px] z-40 flex bg-shell-bg text-fg"
    >
      {/* 左 240 导航 */}
      <div className="flex h-full w-[240px] shrink-0 flex-col border-r border-edge bg-shell-sunk px-2 py-2.5">
        <div className="mb-1.5 flex h-[30px] items-center gap-1.5 rounded-md border border-edge bg-shell-panel px-2">
          <Search size={12} className="shrink-0 text-fg-4" />
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="搜索设置…"
            data-testid="settings-search"
            className="h-full min-w-0 flex-1 bg-transparent text-[12px] text-fg outline-none placeholder:text-fg-4"
          />
        </div>
        <button
          type="button"
          data-testid="settings-back"
          onClick={() => close('settings')}
          className="mb-1.5 flex h-[26px] w-full items-center gap-1.5 rounded-md px-2 text-left text-[12px] text-fg-3 transition-colors hover:bg-shell-hover"
        >
          <ArrowLeft size={12} />
          返回
        </button>
        <div className="flex min-h-0 flex-1 flex-col gap-0.5 overflow-y-auto">
          {pages.map((p) => (
            <NavRow key={p.id} id={p.id} label={p.label} active={page === p.id} onPick={setPage} />
          ))}
          {pages.length === 0 && <div className="px-2 py-1 text-[11px] text-fg-4">无匹配设置页</div>}
        </div>
        <div className="mt-2 flex items-center gap-2 border-t border-edge pt-2">
          <span className="flex h-7 w-7 shrink-0 items-center justify-center rounded-full bg-acc text-[12px] text-fg-inv">
            本
          </span>
          <span className="flex min-w-0 flex-1 flex-col leading-tight">
            <span className="truncate text-[12px] text-fg">我的空间</span>
            <span className="truncate text-[10.5px] text-fg-4">本地用户</span>
          </span>
        </div>
      </div>
      {/* 内容区 */}
      <div className="relative min-w-0 flex-1">
        <div className="flex h-full flex-col items-center overflow-y-auto">
          <div
            className={cn('flex w-full flex-col px-8 pb-20 pt-8', page === 'appearance' ? 'max-w-[900px]' : 'max-w-[720px]')}
          >
            {page === 'appearance' && <AppearancePage />}
            {page === 'agent' && <AgentPage />}
            {page === 'models' && <ModelsPage />}
            {page === 'skills' && <SkillsPage />}
            {page === 'about' && <AboutPage />}
          </div>
        </div>
      </div>
    </div>
  );
}
