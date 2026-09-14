import { useCallback, useEffect, useState } from 'react';
import { ChevronDown, ChevronRight, RefreshCw } from 'lucide-react';
import { cn } from '@/lib/cn';
import { apiGet, apiPatch } from '@/lib/forgeApi';
import { useToastStore } from '@/lib/toastStore';
import { StatusDot } from '@/components/shell/primitives';

/**
 * F7 wave.5 提案 tab(参考 render_diff_page 诚实适配):
 * 本仓 proposals 是 F2 治理确认单 {id,kind,summary,impact(json),status(pending/approved/rejected),
 * createdBy},无代码内容 diff(无 original/proposed 文本)→ 落地为提案列表:
 * 状态点 + kind + summary + impact pretty JSON 展开 + pending 行「批准/拒绝」按钮
 * (PATCH /api/forge/proposals/{id} {action})——F2 交付后首个真实 UI。
 * 双行号 gutter 代码 diff 渲染器不造无数据之源,留 RD-F7-004(agent 改文件链回填)。
 *
 * 差异留痕:wire 字段为 impact/createdBy(非任务书预估的 payload/createdAt)——按实测面渲染,
 * 无 createdAt 时间不伪造。
 */

export interface Proposal {
  id: string;
  kind: string;
  summary: string;
  impact?: unknown;
  status: string;
  createdBy?: unknown;
}

function statusDot(status: string): string {
  if (status === 'approved') return 'var(--dot-done)';
  if (status === 'rejected') return 'var(--dot-blocked)';
  return 'var(--dot-queued)';
}

function ProposalRow({ p, onAction }: { p: Proposal; onAction: (id: string, action: 'approve' | 'reject') => void }) {
  const [expanded, setExpanded] = useState(false);
  const pending = p.status === 'pending';
  return (
    <div
      data-testid={`proposal-row-${p.id}`}
      className="flex flex-col rounded-lg border border-edge bg-shell-panel shadow-sh1"
    >
      <div className="flex items-center gap-2 px-3 py-2">
        <button
          type="button"
          aria-label={expanded ? '收起' : '展开'}
          data-testid={`proposal-expand-${p.id}`}
          onClick={() => setExpanded((v) => !v)}
          className="flex h-4 w-4 shrink-0 items-center justify-center text-fg-4 hover:text-fg-2"
        >
          {expanded ? <ChevronDown size={11} /> : <ChevronRight size={11} />}
        </button>
        <StatusDot color={statusDot(p.status)} />
        <span className="flex h-[18px] shrink-0 items-center rounded-full border border-edge bg-shell-sunk px-1.5 font-code text-[10px] text-fg-3">
          {p.kind}
        </span>
        <span className="min-w-0 flex-1 truncate text-[12.5px] text-fg">{p.summary}</span>
        <span className="shrink-0 font-code text-[10px] text-fg-4">{p.id}</span>
        <span
          className={cn(
            'flex h-[18px] shrink-0 items-center rounded-full px-1.5 text-[10px]',
            p.status === 'approved' && 'bg-sage-bg text-sage',
            p.status === 'rejected' && 'bg-danger-bg text-danger',
            pending && 'bg-warn-bg text-warn',
          )}
          data-testid={`proposal-status-${p.id}`}
        >
          {p.status}
        </span>
        {pending && (
          <span className="flex shrink-0 items-center gap-1">
            <button
              type="button"
              data-testid={`proposal-approve-${p.id}`}
              onClick={() => onAction(p.id, 'approve')}
              className="flex h-[22px] items-center rounded-md border border-acc bg-acc px-2 text-[11px] text-fg-inv hover:bg-acc-soft"
            >
              批准
            </button>
            <button
              type="button"
              data-testid={`proposal-reject-${p.id}`}
              onClick={() => onAction(p.id, 'reject')}
              className="flex h-[22px] items-center rounded-md border border-edge bg-shell-panel px-2 text-[11px] text-fg-2 hover:bg-shell-hover"
            >
              拒绝
            </button>
          </span>
        )}
      </div>
      {expanded && (
        <pre
          data-testid={`proposal-payload-${p.id}`}
          className="mx-3 mb-2 max-h-[240px] overflow-auto rounded bg-shell-sunk p-2 font-code text-[11px] text-fg-3"
        >
          {JSON.stringify({ impact: p.impact ?? null, createdBy: p.createdBy ?? null }, null, 2)
            .split('\n')
            .slice(0, 40)
            .join('\n')}
        </pre>
      )}
    </div>
  );
}

export default function ProposalsTab() {
  const [proposals, setProposals] = useState<Proposal[] | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const r = await apiGet<{ proposals: Proposal[] }>('/api/forge/proposals');
      setProposals(r.proposals);
      setLoadError(null);
    } catch (err) {
      setLoadError(err instanceof Error ? err.message : String(err));
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const act = async (id: string, action: 'approve' | 'reject') => {
    try {
      await apiPatch(`/api/forge/proposals/${encodeURIComponent(id)}`, { action });
      useToastStore.getState().push('success', action === 'approve' ? '已批准' : '已拒绝');
      await load();
    } catch (err) {
      useToastStore.getState().push('error', `操作失败:${err instanceof Error ? err.message : String(err)}`);
    }
  };

  return (
    <div data-testid="proposals-tab" className="flex h-full min-h-0 flex-col gap-2.5 overflow-y-auto p-5">
      <div className="flex items-center gap-2">
        <span className="font-serif text-[24px] font-bold text-fg">提案</span>
        <span className="flex-1" />
        <button
          type="button"
          title="刷新"
          aria-label="刷新提案"
          data-testid="proposals-refresh"
          onClick={() => void load()}
          className="flex h-[22px] w-[22px] items-center justify-center rounded text-fg-3 hover:bg-shell-hover"
        >
          <RefreshCw size={12} />
        </button>
      </div>
      <span className="text-[11px] text-fg-4">
        F2 治理确认单(两阶段:dry-run 影响面 → pending → 批准/拒绝,终态不可逆)
      </span>
      {loadError && (
        <span className="text-[12px] text-fg-3" data-testid="proposals-error">
          提案清单加载失败:{loadError}
        </span>
      )}
      {proposals !== null && proposals.length === 0 && !loadError && (
        <span className="text-[12px] text-fg-4" data-testid="proposals-empty">
          暂无提案。
        </span>
      )}
      <div className="flex flex-col gap-2">
        {(proposals ?? []).map((p) => (
          <ProposalRow key={p.id} p={p} onAction={(id, a) => void act(id, a)} />
        ))}
      </div>
    </div>
  );
}
