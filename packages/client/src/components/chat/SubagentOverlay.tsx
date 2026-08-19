import { GitFork, X } from 'lucide-react';
import { useChatStore } from '@/lib/chatStore';
import MarkdownFlat from './MarkdownFlat';

/**
 * F7 wave.4 子代理浮层(参考 render_subagent_overlay):浮于 composer 上方
 * (bottom 118 / left 14 / right 14,max-h 360,rounded 10 + 上飘阴影);
 * 头 = accent_bg 图标 + label + 状态徽(运行中 accent/已完成 sage/失败 danger)+ x;
 * 体 = PROMPT / SUMMARY 两节(摘要走 MarkdownFlat;运行中空摘要给占位文案)。
 * 本仓事件面不产生 subagent 块,组件就绪单测覆盖。
 */
export default function SubagentOverlay() {
  const overlayId = useChatStore((st) => st.subagentOverlayId);
  const messages = useChatStore((st) => st.messages);
  const openSubagent = useChatStore((st) => st.openSubagent);

  if (!overlayId) return null;
  let found: { label: string; status: 'running' | 'done' | 'error'; summary?: string } | null = null;
  for (let i = messages.length - 1; i >= 0 && !found; i -= 1) {
    for (const b of messages[i].blocks) {
      if (b.kind === 'subagent' && b.id === overlayId) {
        found = { label: b.label, status: b.status, summary: b.summary };
        break;
      }
    }
  }
  if (!found) return null;
  const badge =
    found.status === 'running'
      ? { text: '运行中', cls: 'bg-acc-bg text-acc' }
      : found.status === 'done'
        ? { text: '已完成', cls: 'bg-shell-sunk text-sage' }
        : { text: '失败', cls: 'bg-shell-sunk text-danger' };

  return (
    <div
      data-testid="subagent-overlay"
      className="absolute inset-x-[14px] bottom-[118px] z-30 flex max-h-[360px] flex-col overflow-hidden rounded-[10px] border border-edge bg-shell-float shadow-float"
    >
      <div className="flex items-center gap-2 border-b border-edge px-2.5 py-2">
        <span className="flex h-5 w-5 shrink-0 items-center justify-center rounded-md bg-acc-bg text-acc">
          <GitFork size={11} />
        </span>
        <span className="min-w-0 flex-1 truncate text-[12.5px] font-semibold text-fg">
          {found.label}
        </span>
        <span className={`rounded-[5px] px-2 py-[3px] text-[10.5px] ${badge.cls}`}>{badge.text}</span>
        <button
          type="button"
          aria-label="关闭子代理详情"
          data-testid="subagent-overlay-close"
          onClick={() => openSubagent(null)}
          className="flex h-[22px] w-[22px] items-center justify-center rounded-[5px] text-fg-3 hover:bg-shell-hover"
        >
          <X size={12} />
        </button>
      </div>
      <div className="flex max-h-[316px] flex-col gap-2.5 overflow-y-auto p-3">
        <div className="flex flex-col gap-1">
          <span className="text-[10px] font-semibold text-fg-4">SUMMARY</span>
          {found.summary && found.summary.trim() !== '' ? (
            <MarkdownFlat text={found.summary} />
          ) : (
            <span className="font-code text-[12px] text-fg-4">
              {found.status === 'running' ? '子 agent 正在工作，完成后会在这里显示摘要。' : '无摘要输出。'}
            </span>
          )}
        </div>
      </div>
    </div>
  );
}
