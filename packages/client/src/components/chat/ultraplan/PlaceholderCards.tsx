import type { ReactNode } from 'react';
import { CheckCircle2, FileText, ListChecks } from 'lucide-react';
import type { ChatBlock } from '@/lib/timeline';
import { useWorkbenchStore } from '@/lib/workbenchStore';

type Ultra = Extract<ChatBlock, { kind: 'ultraplan' }>;

/**
 * D-044:计划 / 验收 / 完成三张关口卡的**只读占位**(Demo 卡已由 DemoCard 接手)。
 *
 * 只把建卡事件的载荷摘要成几行,不提供任何流程动作;各自的正式卡片(计划就绪卡、验收清单、
 * 完成卡)落地后,在 UltraPlanBlock 的分发里替换对应分支即可。
 * 载荷来自事件原文,字段一律按「可能缺失 / 类型不对」读。
 */

function str(payload: Record<string, unknown>, key: string): string {
  const value = payload[key];
  return typeof value === 'string' ? value : '';
}

function num(payload: Record<string, unknown>, key: string): number | null {
  const value = payload[key];
  return typeof value === 'number' && Number.isFinite(value) ? value : null;
}

function record(raw: unknown): Record<string, unknown> | null {
  return raw !== null && typeof raw === 'object' && !Array.isArray(raw)
    ? (raw as Record<string, unknown>)
    : null;
}

function PlaceholderShell({
  icon,
  title,
  note,
  children,
}: {
  icon: ReactNode;
  title: string;
  note?: string;
  children?: ReactNode;
}) {
  return (
    <section className="my-1 rounded-lg border border-edge bg-shell-sunk px-3 py-2.5">
      <div className="flex items-center gap-1.5 text-[11px] font-medium text-fg-3">
        {icon}
        {title}
      </div>
      {children}
      {note && <div className="mt-1.5 text-[10.5px] leading-[15px] text-fg-4">{note}</div>}
    </section>
  );
}

const line = 'mt-1 break-words text-[12px] leading-[17px] text-fg-2';
const meta = 'mt-1 text-[11px] leading-[16px] text-fg-3';

/** ultraplan.plan.ready:{ rev, planPath, name, overview, gameMode, taskCount, roles[], automated, manual }。 */
export function PlanCardPlaceholder({ block }: { block: Ultra }) {
  const openPlan = useWorkbenchStore((state) => state.openPlan);
  const payload = block.payload;
  const name = str(payload, 'name');
  const overview = str(payload, 'overview');
  const planPath = str(payload, 'planPath');
  const gameMode = str(payload, 'gameMode');
  const taskCount = num(payload, 'taskCount');
  const automated = num(payload, 'automated');
  const manual = num(payload, 'manual');
  const roles = Array.isArray(payload.roles)
    ? payload.roles.filter((r): r is string => typeof r === 'string')
    : [];
  const facts = [
    taskCount !== null ? `${taskCount} 个任务` : '',
    gameMode !== '' ? gameMode.toUpperCase() : '',
    roles.length > 0 ? `角色:${roles.join('、')}` : '',
    automated !== null ? `自动化验证 ${automated} 项` : '',
    manual !== null ? `人工检查点 ${manual} 项` : '',
  ].filter((part) => part !== '');
  return (
    <PlaceholderShell
      icon={<FileText size={12} />}
      title={`制作计划${block.rev > 1 ? ` · 第 ${block.rev} 版` : ''}`}
      note="「确认并开始制作 / 要求修改」入口尚未接入,此处仅展示摘要。"
    >
      {name !== '' && <div className={line}>{name}</div>}
      {overview !== '' && <div className={`${meta} line-clamp-4 break-words`}>{overview}</div>}
      {facts.length > 0 && <div className={meta}>{facts.join(' · ')}</div>}
      {planPath !== '' && (
        <button
          type="button"
          data-testid="ultraplan-plan-open"
          onClick={() => openPlan(planPath)}
          className="mt-2 flex h-[24px] items-center rounded-md border border-edge bg-shell-panel px-2 text-[11px] text-fg-2 hover:bg-shell-hover"
        >
          在 Plan 页签打开
        </button>
      )}
    </PlaceholderShell>
  );
}

/** ultraplan.acceptance.ready:{ round, manual[{id,title,steps,expected}] };submitted = acceptance.recorded。 */
export function AcceptanceCardPlaceholder({ block }: { block: Ultra }) {
  const manual = (Array.isArray(block.payload.manual) ? block.payload.manual : [])
    .map(record)
    .filter((item): item is Record<string, unknown> => item !== null);
  const results = (block.submitted && Array.isArray(block.submitted.results) ? block.submitted.results : [])
    .map(record)
    .filter((item): item is Record<string, unknown> => item !== null);
  const count = (status: string) => results.filter((r) => r.status === status).length;
  return (
    <PlaceholderShell
      icon={<ListChecks size={12} />}
      title={`人工验收 · 第 ${block.rev} 轮`}
      note={block.submitted ? undefined : '逐项「通过 / 未通过 / 跳过」的验收清单尚未接入,此处仅展示检查点。'}
    >
      {manual.length > 0 && (
        <ol className="mt-1 flex flex-col gap-0.5">
          {manual.map((item, i) => (
            <li key={str(item, 'id') || i} className="break-words text-[11.5px] leading-[17px] text-fg-2">
              {i + 1}. {str(item, 'title') || str(item, 'id')}
            </li>
          ))}
        </ol>
      )}
      {block.submitted && (
        <div className={meta}>
          已提交 · 通过 {count('pass')} · 未通过 {count('fail')} · 跳过 {count('skip')}
        </div>
      )}
    </PlaceholderShell>
  );
}

/** ultraplan.done:{ id }。 */
export function DoneCardPlaceholder() {
  return (
    <PlaceholderShell icon={<CheckCircle2 size={12} />} title="UltraPlan 已完成">
      <div className={meta}>所有关口均已通过,产物保留在工作区的 .forge/ultraplan/ 下。</div>
    </PlaceholderShell>
  );
}
