import type { ReactNode } from 'react';
import { useOverlayStore } from '@/lib/overlayStore';
import { useSettingsStore } from '@/lib/settingsStore';
import { shortcutGroups } from '@/lib/shortcuts';
import AboutPanel from '../AboutPanel';
import { Kbd } from '../primitives';

/**
 * 小 modal 群:关于(与设置·关于页共用 AboutPanel)/ 键盘快捷键(读 lib/shortcuts 统一键位表,
 * 发送键随设置·Agent 页「Ctrl+Enter 发送」变化)。
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
        data-testid={testId}
        className="flex max-w-[92vw] flex-col gap-3 rounded-xl border border-edge-strong bg-shell-float p-[22px] shadow-float"
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
    <ModalShell testId="about-modal" width={560} onClose={() => close('about')}>
      <AboutPanel compact />
    </ModalShell>
  );
}

function ShortcutsModal() {
  const close = useOverlayStore((st) => st.close);
  const submitCtrlEnter = useSettingsStore((st) => st.submitCtrlEnter);
  return (
    <ModalShell testId="shortcuts-modal" width={580} onClose={() => close('shortcuts')}>
      <span className="font-serif text-[16px] font-semibold text-fg">键盘快捷键</span>
      <div className="grid grid-cols-2 gap-x-6 gap-y-3">
        {shortcutGroups(submitCtrlEnter).map((g) => (
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
