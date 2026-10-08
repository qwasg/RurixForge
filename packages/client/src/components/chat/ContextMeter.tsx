import { LoaderCircle, Shrink, X } from 'lucide-react';
import { useChatStore } from '@/lib/chatStore';
import { cn } from '@/lib/cn';
import { formatTokens, percentLabel, type ContextUsage } from '@/lib/contextUsage';

/**
 * Composer 上下文窗口计量(2026-08-24 用户拍板):
 * 胶囊下方工具行「选择模型」右侧 = 灰色空心圆环 + 内圈按占有率扇形填充 + 右侧百分比数字;
 * 点击在胶囊上方展开面板。
 *
 * 2026-10-07 改版(用户:字看不清、明细太碎、要能压缩):面板只留占用条 + 三行
 * (系统提示与工具 / 对话历史 / 待发送),字号 12.5–13px、正文色;底部「压缩上下文」把较早的
 * 对话总结成摘要(POST /sessions/{id}/compact,本地 / Codex 两种引擎),压缩后时间线出现分隔线。
 *
 * 配色:环恒灰(edge-strong);扇形与数字 < 75% 走 text-3 灰,75–90% 转 warn,≥ 90% 转 danger
 * ——「灰环」形态不变,仅在逼近窗口时提醒。
 */

/** 扇形半径(viewBox 24 单位);用 r = R/2 + strokeWidth = R 的描边法画实心扇。 */
const PIE_R = 6.5;
const PIE_C = Math.PI * PIE_R;

/** 逼近窗口的两级提醒(ratio 已 clamp 0–1)。 */
function toneOf(ratio: number): { text: string; stroke: string } {
  if (ratio >= 0.9) return { text: 'text-danger', stroke: 'var(--danger)' };
  if (ratio >= 0.75) return { text: 'text-warn', stroke: 'var(--warn)' };
  return { text: 'text-fg-2', stroke: 'var(--text-3)' };
}

function Ring({ ratio, stroke }: { ratio: number; stroke: string }) {
  return (
    <svg width={12} height={12} viewBox="0 0 24 24" aria-hidden="true" className="shrink-0">
      <circle
        cx="12"
        cy="12"
        r="10"
        fill="none"
        strokeWidth="2"
        stroke="currentColor"
        className="text-edge-strong"
      />
      {ratio > 0 && (
        <circle
          data-testid="context-ring-fill"
          cx="12"
          cy="12"
          r={PIE_R / 2}
          fill="none"
          stroke={stroke}
          strokeWidth={PIE_R}
          strokeDasharray={`${ratio * PIE_C} ${PIE_C}`}
          transform="rotate(-90 12 12)"
        />
      )}
    </svg>
  );
}

/** 工具行按钮:灰环 + 百分比(与「选择模型」同规格 22px 胶囊)。 */
export function ContextMeterButton({
  usage,
  open,
  onToggle,
}: {
  usage: ContextUsage;
  open: boolean;
  onToggle: () => void;
}) {
  const tone = toneOf(usage.ratio);
  return (
    <button
      type="button"
      aria-label="上下文窗口占用"
      aria-expanded={open}
      data-testid="composer-context"
      title={`上下文 ${formatTokens(usage.used)} / ${formatTokens(usage.window)}（${percentLabel(usage)}）`}
      onClick={onToggle}
      className={cn(
        'flex h-[22px] items-center gap-1 rounded-md px-1.5 text-[11.5px] hover:bg-shell-hover',
        tone.text,
        open && 'bg-shell-active',
      )}
    >
      <Ring ratio={usage.ratio} stroke={tone.stroke} />
      <span className="tabular-nums" data-testid="composer-context-percent">
        {percentLabel(usage)}
      </span>
    </button>
  );
}

/** 胶囊上方面板:占用条 + 三行分项 + 压缩上下文。 */
export function ContextMeterPanel({
  usage,
  onClose,
}: {
  usage: ContextUsage;
  onClose: () => void;
}) {
  const tone = toneOf(usage.ratio);
  const hasSession = useChatStore((st) => st.currentSessionId !== null);
  const running = useChatStore((st) => st.activeRunId !== null);
  const compacting = useChatStore(
    (st) => st.compactingSessionId !== null && st.compactingSessionId === st.currentSessionId,
  );
  const compactContext = useChatStore((st) => st.compactContext);

  const blocked = !hasSession
    ? '还没有对话'
    : running
      ? 'Agent 正在运行，结束后再压缩'
      : usage.messageCount === 0
        ? usage.compacted
          ? '已压缩，之后的新对话可再次压缩'
          : '还没有可压缩的对话'
        : null;
  const hint = compacting ? '正在把较早的对话总结成摘要…' : (blocked ?? '把较早的对话总结成摘要，腾出上下文空间');

  return (
    <div
      data-testid="composer-context-panel"
      className="flex flex-col rounded-xl border border-edge bg-shell-panel px-4 pb-3 pt-2.5 shadow-sh1"
    >
      <div className="flex items-center gap-2">
        <span className="text-[13px] font-semibold text-fg">上下文</span>
        <span className="flex-1" />
        <span className={cn('text-[13px] font-semibold tabular-nums', tone.text)}>
          {percentLabel(usage)}
        </span>
        <button
          type="button"
          aria-label="关闭上下文面板"
          data-testid="composer-context-close"
          onClick={onClose}
          className="-mr-1.5 flex h-6 w-6 items-center justify-center rounded-md text-fg-3 hover:bg-shell-hover hover:text-fg"
        >
          <X size={14} />
        </button>
      </div>

      <div className="mt-2 h-1.5 overflow-hidden rounded-full bg-shell-active">
        <div
          data-testid="context-bar-fill"
          className="h-full rounded-full transition-[width] duration-300"
          style={{ width: `${usage.ratio * 100}%`, background: tone.stroke }}
        />
      </div>
      <div
        className="mt-1.5 text-[12px] text-fg-3"
        title={usage.calibrated ? '按最近一次请求的实测用量校准' : '按字符估算，仅供参考'}
      >
        约 {formatTokens(usage.used)} / {formatTokens(usage.window)} tokens
      </div>

      <ul className="mt-3 flex flex-col gap-1.5">
        {usage.rows.map((r) => (
          <li
            key={r.kind}
            data-testid={`context-row-${r.kind}`}
            className="flex items-center justify-between gap-3 text-[12.5px] leading-[18px]"
          >
            <span className="text-fg-2">{r.label}</span>
            <span className="tabular-nums text-fg">{formatTokens(r.tokens)}</span>
          </li>
        ))}
      </ul>

      <div className="mt-3 flex items-center gap-3 border-t border-edge pt-3">
        <button
          type="button"
          data-testid="composer-context-compact"
          disabled={compacting || blocked !== null}
          onClick={() => void compactContext()}
          className="inline-flex h-8 shrink-0 items-center gap-1.5 rounded-lg border border-edge-strong bg-shell-panel px-3 text-[12.5px] font-medium text-fg transition-colors hover:bg-shell-hover disabled:cursor-not-allowed disabled:opacity-50 disabled:hover:bg-shell-panel"
        >
          {compacting ? <LoaderCircle size={14} className="animate-spin" /> : <Shrink size={14} />}
          {compacting ? '压缩中…' : '压缩上下文'}
        </button>
        <span data-testid="composer-context-hint" className="min-w-0 text-[12px] leading-[18px] text-fg-3">
          {hint}
        </span>
      </div>
    </div>
  );
}
