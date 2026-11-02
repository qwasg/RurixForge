import { useState, type ReactNode } from 'react';
import { ChevronDown, ChevronRight } from 'lucide-react';

function readOpen(storageKey: string | undefined, fallback: boolean): boolean {
  if (!storageKey) return fallback;
  try {
    const v = localStorage.getItem(storageKey);
    return v === null ? fallback : v === '1';
  } catch {
    return fallback;
  }
}

/** 设置页可折叠的「高级」分区(标题行与 SetSectionLabel 同排版);给 storageKey 时展开状态记在本机。 */
export default function AdvancedSection({
  title,
  testId,
  defaultOpen = false,
  storageKey,
  children,
}: {
  title: string;
  testId: string;
  defaultOpen?: boolean;
  storageKey?: string;
  children: ReactNode;
}) {
  const [open, setOpen] = useState(() => readOpen(storageKey, defaultOpen));
  const toggle = () => {
    const next = !open;
    setOpen(next);
    if (!storageKey) return;
    try {
      localStorage.setItem(storageKey, next ? '1' : '0');
    } catch {
      // 偏好记不住不影响功能(浏览器禁用存储)
    }
  };
  return (
    <div data-testid={testId} data-open={open ? '1' : undefined} className="flex flex-col">
      <button
        type="button"
        data-testid={`${testId}-toggle`}
        aria-expanded={open}
        onClick={toggle}
        className="mb-2 mt-5 flex items-center gap-1 self-start pl-0.5 text-[11px] font-medium text-fg-3 transition-colors hover:text-fg-2"
      >
        {open ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
        {title}
      </button>
      {open && <div className="flex flex-col gap-3">{children}</div>}
    </div>
  );
}
