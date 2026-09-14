import { useToastStore, type ToastKind } from '@/lib/toastStore';
import { StatusDot } from '../primitives';

/**
 * F7 wave.3 toast 堆叠(参考 ui/overlays.rs render_toasts):
 * 右下 bottom 46 / right 20 / gap 8;3px 类型色竖条 + 6px 状态点 + 12px 标题;
 * bg_panel + line + sh_float 圆角 8;2.8s 自动消失(store 内计时)。
 * 类型色:success=sage / error=danger / warning=warn / info=accent。
 */

const TONE: Record<ToastKind, { bar: string; dot: string }> = {
  success: { bar: 'var(--sage)', dot: 'var(--dot-done)' },
  error: { bar: 'var(--danger)', dot: 'var(--dot-blocked)' },
  warning: { bar: 'var(--warn)', dot: 'var(--dot-queued)' },
  info: { bar: 'var(--accent)', dot: 'var(--dot-running)' },
};

export default function ToastStack() {
  const items = useToastStore((st) => st.items);
  return (
    <div
      data-testid="toast-stack"
      className="pointer-events-none absolute bottom-[46px] right-[20px] z-[60] flex flex-col gap-2"
    >
      {items.map((t) => (
        <div
          key={t.id}
          data-testid={`toast-${t.kind}`}
          className="pointer-events-auto flex items-center gap-2 overflow-hidden rounded-lg border border-edge bg-shell-panel py-2 pl-0 pr-3 text-[12px] text-fg shadow-float"
        >
          <span className="h-full w-[3px] shrink-0 self-stretch" style={{ background: TONE[t.kind].bar }} />
          <StatusDot color={TONE[t.kind].dot} />
          <span>{t.title}</span>
        </div>
      ))}
    </div>
  );
}
