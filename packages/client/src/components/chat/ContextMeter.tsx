import {
  BookOpen,
  Brain,
  Cpu,
  FileText,
  MessageSquare,
  PencilLine,
  Wrench,
  X,
} from 'lucide-react';
import { cn } from '@/lib/cn';
import { formatTokens, type ContextRowKind, type ContextUsage } from '@/lib/contextUsage';

/**
 * Composer 上下文窗口计量(2026-08-24 用户拍板):
 * 胶囊下方工具行「选择模型」右侧 = 灰色空心圆环 + 内圈按占有率扇形填充 + 右侧百分比数字;
 * 点击在胶囊上方展开明细表(文件 / 技能 / 工具结果 / 对话 / 思考 / 系统基线),
 * 按 token 降序,每行带占比条,底注标明估算口径。
 *
 * 配色:环恒灰(edge-strong);扇形与数字 < 75% 走 text-3 灰,75–90% 转 warn,≥ 90% 转 danger
 * ——「灰环」形态不变,仅在逼近窗口时提醒。
 */

/** 扇形半径(viewBox 24 单位);用 r = R/2 + strokeWidth = R 的描边法画实心扇。 */
const PIE_R = 6.5;
const PIE_C = Math.PI * PIE_R;

const KIND_META: Record<ContextRowKind, { label: string; icon: typeof FileText }> = {
  base: { label: '系统', icon: Cpu },
  file: { label: '文件', icon: FileText },
  tool: { label: '工具', icon: Wrench },
  message: { label: '对话', icon: MessageSquare },
  reasoning: { label: '思考', icon: Brain },
  skill: { label: '技能', icon: BookOpen },
  draft: { label: '草稿', icon: PencilLine },
};

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
      title={`上下文 ${formatTokens(usage.used)} / ${formatTokens(usage.window)}（${usage.percent}%）`}
      onClick={onToggle}
      className={cn(
        'flex h-[22px] items-center gap-1 rounded-md px-1.5 text-[11px] hover:bg-shell-hover',
        tone.text,
        open && 'bg-shell-active',
      )}
    >
      <Ring ratio={usage.ratio} stroke={tone.stroke} />
      <span className="font-code" data-testid="composer-context-percent">
        {usage.percent}%
      </span>
    </button>
  );
}

/** 胶囊上方明细表。 */
export function ContextMeterPanel({
  usage,
  onClose,
}: {
  usage: ContextUsage;
  onClose: () => void;
}) {
  const tone = toneOf(usage.ratio);
  return (
    <div
      data-testid="composer-context-panel"
      className="flex flex-col overflow-hidden rounded-xl border border-edge bg-shell-panel"
    >
      <div className="flex items-center gap-2 border-b border-edge px-3 py-2">
        <span className="text-[11.5px] font-semibold text-fg-2">上下文窗口</span>
        <span className="font-code text-[10.5px] text-fg-3">
          {formatTokens(usage.used)} / {formatTokens(usage.window)}
        </span>
        <span className={cn('font-code text-[10.5px]', tone.text)}>{usage.percent}%</span>
        <span className="flex-1" />
        <button
          type="button"
          aria-label="关闭上下文明细"
          data-testid="composer-context-close"
          onClick={onClose}
          className="flex h-5 w-5 items-center justify-center rounded text-fg-3 hover:bg-shell-hover"
        >
          <X size={11} />
        </button>
      </div>

      {usage.rows.length === 0 ? (
        <div className="px-3 py-3 text-[10.5px] text-fg-4">暂无上下文占用</div>
      ) : (
        <div className="max-h-[220px] overflow-y-auto">
          {/* table-fixed:名称列吃满剩余宽,数值列定宽,窄侧栏下不被挤成两三个字 */}
          <table className="w-full table-fixed border-collapse text-left">
            <colgroup>
              <col />
              <col className="w-[46px]" />
              <col className="w-[88px]" />
            </colgroup>
            <thead className="sticky top-0 bg-shell-panel">
              <tr className="text-[9.5px] text-fg-4">
                <th className="py-1 pl-3 pr-2 font-normal">条目</th>
                <th className="py-1 pr-2 text-right font-normal">Tokens</th>
                <th className="py-1 pl-2 pr-3 text-right font-normal">占比</th>
              </tr>
            </thead>
            <tbody>
              {usage.rows.map((r) => {
                const meta = KIND_META[r.kind];
                const share = usage.used === 0 ? 0 : (r.tokens / usage.used) * 100;
                return (
                  <tr
                    key={r.key}
                    data-testid={`context-row-${r.kind}`}
                    className="border-t border-edge align-middle"
                    title={`${meta.label} · ${r.detail}`}
                  >
                    <td className="py-1 pl-3 pr-2">
                      <span className="flex items-center gap-1">
                        <meta.icon size={10} className="shrink-0 text-fg-3" />
                        <span className="truncate text-[11px] text-fg-2">{r.label}</span>
                      </span>
                      <span className="block truncate pl-[14px] text-[9.5px] text-fg-4">
                        {meta.label} · {r.detail}
                      </span>
                    </td>
                    <td className="py-1 pr-2 text-right font-code text-[10.5px] text-fg-2">
                      {formatTokens(r.tokens)}
                    </td>
                    <td className="py-1 pl-2 pr-3">
                      <span className="flex items-center justify-end gap-1.5">
                        <span className="h-1 w-9 shrink-0 rounded-full bg-shell-active">
                          <span
                            className="block h-full rounded-full bg-fg-4"
                            style={{ width: `${Math.round(share)}%` }}
                          />
                        </span>
                        <span className="w-7 shrink-0 text-right font-code text-[9.5px] text-fg-3">
                          {share < 1 ? '<1%' : `${Math.round(share)}%`}
                        </span>
                      </span>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}

      {/* 表区超高可滚,滚动条不总可见 → 底注如实报总项数,避免误以为只有可视几行 */}
      <div className="border-t border-edge px-3 py-1.5 text-[9.5px] leading-[14px] text-fg-4">
        共 {usage.rows.length} 项 ·{' '}
        {usage.calibrated
          ? `系统行由最近一次实测 prompt ${usage.measured} tokens 倒算，其余为字符估算`
          : '本会话尚无实测用量，全部为字符估算（中文 0.6 / 其余 0.3 token）'}
      </div>
    </div>
  );
}
