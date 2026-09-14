import { FileText } from 'lucide-react';
import type { ChatBlock } from '@/lib/timeline';
import { useWorkbenchStore } from '@/lib/workbenchStore';
import MarkdownFlat from './MarkdownFlat';

type Plan = Extract<ChatBlock, { kind: 'plan' }>;

/** 计划 item 的流式正文；落盘后直接衔接既有 Plan 页签。 */
export default function PlanBlock({ block }: { block: Plan }) {
  const openPlan = useWorkbenchStore((state) => state.openPlan);
  return (
    <section
      data-testid="plan-block"
      className="my-1 rounded-lg border border-edge bg-shell-sunk px-3 py-2.5"
    >
      <div className="mb-2 flex items-center gap-1.5 text-[11px] font-medium text-fg-3">
        <FileText size={12} />
        Plan
        {!block.final && <span className="forge-thinking ml-1">Writing</span>}
      </div>
      {block.text.trim() !== '' && <MarkdownFlat text={block.text} />}
      {block.planPath && (
        <button
          type="button"
          data-testid="plan-block-open"
          onClick={() => openPlan(block.planPath!)}
          className="mt-2 flex h-[24px] items-center rounded-md border border-edge bg-shell-panel px-2 text-[11px] text-fg-2 hover:bg-shell-hover"
        >
          在 Plan 页签打开
        </button>
      )}
    </section>
  );
}
