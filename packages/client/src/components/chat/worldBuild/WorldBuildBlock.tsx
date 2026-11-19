import React from 'react';
import type { ChatBlock } from '@/lib/timeline';
import BuildPreviewCard from './BuildPreviewCard';
import { Box, CheckCircle2, Wrench, ShieldCheck } from 'lucide-react';

type WorldBuildBlockData = Extract<ChatBlock, { kind: 'world_build' }>;

function renderStepCard(block: WorldBuildBlockData) {
  switch (block.step) {
    case 'preview':
      return <BuildPreviewCard block={block} />;
    case 'survey':
      return (
        <div
          data-testid="world-build-survey-card"
          className="my-2 flex w-full max-w-2xl items-center gap-2.5 rounded-lg border border-edge bg-shell-panel p-3 text-[13px] text-fg"
        >
          <Box className="h-4 w-4 text-acc" />
          <span className="font-medium">项目地形与资产勘测已完成</span>
          <span className="text-[12px] text-fg-3">准备生成 3D 预览场景...</span>
        </div>
      );
    case 'construction':
      return (
        <div
          data-testid="world-build-construction-card"
          className="my-2 flex w-full max-w-2xl items-center gap-2.5 rounded-lg border border-edge bg-shell-panel p-3 text-[13px] text-fg"
        >
          <Wrench className="h-4 w-4 text-amber-500" />
          <span className="font-medium">多智能体场景施工进行中</span>
          <span className="text-[12px] text-fg-3">正在装配几何体与内部道具...</span>
        </div>
      );
    case 'coherence_verify':
      return (
        <div
          data-testid="world-build-verify-card"
          className="my-2 flex w-full max-w-2xl items-center gap-2.5 rounded-lg border border-edge bg-shell-panel p-3 text-[13px] text-fg"
        >
          <ShieldCheck className="h-4 w-4 text-sky-500" />
          <span className="font-medium">场景全局一致性验收</span>
          <span className="text-[12px] text-fg-3">正在检测碰撞与孤立道具...</span>
        </div>
      );
    case 'done':
      return (
        <div
          data-testid="world-build-done-card"
          className="my-2 flex w-full max-w-2xl items-center gap-2.5 rounded-lg border border-edge bg-shell-panel p-3 text-[13px] text-fg"
        >
          <CheckCircle2 className="h-4 w-4 text-emerald-500" />
          <span className="font-medium">场景搭建已完成并保存至工程</span>
        </div>
      );
    default:
      return null;
  }
}

export default function WorldBuildBlock({ block }: { block: WorldBuildBlockData }) {
  const card = renderStepCard(block);
  if (!card) return null;

  return (
    <div
      data-testid="world-build-block"
      data-step={block.step}
      data-slug={block.slug}
      data-rev={block.rev}
      data-submitted={block.submitted ? '1' : undefined}
    >
      {card}
    </div>
  );
}
