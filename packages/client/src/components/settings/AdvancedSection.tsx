import { useState, type ReactNode } from 'react';
import { ChevronDown, ChevronRight } from 'lucide-react';

/** 设置页可折叠的「高级」分区(标题行与 SetSectionLabel 同排版)。 */
export default function AdvancedSection({
  title,
  testId,
  defaultOpen = false,
  children,
}: {
  title: string;
  testId: string;
  defaultOpen?: boolean;
  children: ReactNode;
}) {
  const [open, setOpen] = useState(defaultOpen);
  return (
    <div data-testid={testId} data-open={open ? '1' : undefined} className="flex flex-col">
      <button
        type="button"
        data-testid={`${testId}-toggle`}
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
        className="mb-2 mt-5 flex items-center gap-1 self-start pl-0.5 text-[11px] font-medium text-fg-3 transition-colors hover:text-fg-2"
      >
        {open ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
        {title}
      </button>
      {open && <div className="flex flex-col gap-3">{children}</div>}
    </div>
  );
}
