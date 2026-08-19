import type { ReactNode } from 'react';
import { useOverlayStore, type OverlayKey } from '@/lib/overlayStore';
import { Kbd } from '../primitives';

/**
 * F7 wave.3 小 modal 群(参考 ui/overlays.rs about/shortcuts;scrim rgba(20,18,15,0.32)):
 * About / Shortcuts;wave.5:Settings 占位由全屏 SettingsOverlay 取代(components/settings)。
 */

function ModalShell({
  testId,
  onClose,
  children,
  width,
}: {
  testId: string;
  onClose: () => void;
  children: ReactNode;
  width: number;
}) {
  return (
    <div
      data-testid={`${testId}-scrim`}
      className="absolute inset-0 z-50 flex items-center justify-center"
      style={{ background: 'var(--scrim-modal)' }}
      onMouseDown={onClose}
    >
      <div
        role="dialog"
        className="flex flex-col gap-3 rounded-xl border border-edge-strong bg-shell-float p-[22px] shadow-float"
        style={{ width }}
        onMouseDown={(e) => e.stopPropagation()}
      >
        {children}
      </div>
    </div>
  );
}

function AboutModal() {
  const close = useOverlayStore((st) => st.close);
  return (
    <ModalShell testId="about-modal" width={420} onClose={() => close('about')}>
      <div className="flex items-center gap-3">
        <span className="flex h-14 w-14 shrink-0 items-center justify-center rounded-xl bg-fg font-serif text-[26px] text-fg-inv">
          铸
        </span>
        <span className="flex flex-col">
          <span className="font-serif text-[16px] font-semibold text-fg">RurixForge · 游戏引擎工作台</span>
          <span className="text-[11px] text-fg-4">v0.1.0 · F7 wave.3 壳</span>
        </span>
      </div>
      <div className="flex flex-col gap-1 text-[12px] text-fg-3">
        <span>后端：forge-agentd(经 host 127.0.0.1:3080)</span>
        <span>前端：React 18 + Tailwind + zustand + vite</span>
        <span>外观参考：Moonlit Agent IDE（主题派色逐行移植）</span>
      </div>
    </ModalShell>
  );
}

const SHORTCUT_GROUPS: Array<{ title: string; rows: Array<[string, string]> }> = [
  {
    title: '通用',
    rows: [
      ['命令面板', 'Ctrl+K'],
      ['新建会话', 'Ctrl+Shift+N'],
      ['关闭浮层', 'Esc'],
    ],
  },
  { title: '视图', rows: [['切换会话栏 / 对话栏 / Inspector', 'View 菜单']] },
  {
    title: '编辑',
    rows: [
      ['复制 / 粘贴', 'Ctrl+C / V'],
      ['全选', 'Ctrl+A'],
    ],
  },
  { title: 'Composer', rows: [['发送', 'Enter（wave.4 落地）']] },
];

function ShortcutsModal() {
  const close = useOverlayStore((st) => st.close);
  return (
    <ModalShell testId="shortcuts-modal" width={580} onClose={() => close('shortcuts')}>
      <span className="font-serif text-[16px] font-semibold text-fg">键盘快捷键</span>
      <div className="grid grid-cols-2 gap-x-6 gap-y-3">
        {SHORTCUT_GROUPS.map((g) => (
          <div key={g.title} className="flex flex-col gap-1">
            <span className="text-[11px] font-semibold text-fg-4">{g.title}</span>
            {g.rows.map(([label, keys]) => (
              <div key={label} className="flex items-center gap-2">
                <span className="flex-1 text-[12px] text-fg-2">{label}</span>
                <Kbd label={keys} />
              </div>
            ))}
          </div>
        ))}
      </div>
    </ModalShell>
  );
}

export default function Modals() {
  const about = useOverlayStore((st) => st.about);
  const shortcuts = useOverlayStore((st) => st.shortcuts);
  return (
    <>
      {about && <AboutModal />}
      {shortcuts && <ShortcutsModal />}
    </>
  );
}
