import type { ChatBlock } from '@/lib/timeline';
import DemoCard from './DemoCard';
import { AcceptanceCard, DoneCard, PlanReviewCard } from './WorkflowCards';
import QuestionnaireCard from './QuestionnaireCard';

type Ultra = Extract<ChatBlock, { kind: 'ultraplan' }>;

function card(block: Ultra) {
  switch (block.step) {
    case 'questionnaire':
      // key:同一槽位换成另一版问卷时(upsert 换块)草稿态随之重置,不把上一版的勾选带过来。
      return <QuestionnaireCard key={`${block.upId}:${block.rev}`} block={block} />;
    case 'demo':
      return <DemoCard block={block} />;
    case 'plan':
      return <PlanReviewCard key={`${block.upId}:${block.rev}`} block={block} />;
    case 'acceptance':
      return <AcceptanceCard key={`${block.upId}:${block.rev}`} block={block} />;
    case 'done':
      return <DoneCard block={block} />;
    default:
      return null;
  }
}

/**
 * D-044:UltraPlan 关口卡的分发壳。
 *
 * 外层 div 只带定位用的标记(step / rev / submitted),卡片本体按 step 分发:
 * 问卷 = QuestionnaireCard(可填写);Demo = DemoCard(打开 Demo / 通过 / 提出修改);
 * 计划确认与修改、人工验收与自动修复、完成记录均为正式关口卡。是否可操作统一用
 * ultraPlanStore.cardInteractive(block, state, activeRunId) 判定(契约 §9)。
 */
export default function UltraPlanBlock({ block }: { block: Ultra }) {
  const body = card(block);
  if (body === null) return null;
  return (
    <div
      data-testid="ultraplan-block"
      data-step={block.step}
      data-rev={block.rev}
      data-submitted={block.submitted ? '1' : undefined}
    >
      {body}
    </div>
  );
}
