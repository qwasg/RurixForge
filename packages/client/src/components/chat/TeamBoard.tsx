import { useState } from 'react';
import type { TeamAction } from '@forge/protocol';
import { useCollaborationStore } from '@/lib/collaborationStore';
import { useChatStore } from '@/lib/chatStore';
import AgentMailbox from './AgentMailbox';
import ApprovalCard from './ApprovalCard';

const statusLabels: Record<string, string> = {
  active: '执行中', paused: '已暂停', stopped: '已停止', completed: '已完成', recoveryRequired: '需要恢复',
  queued: '待执行', running: '进行中', failed: '失败', blocked: '受阻', idle: '空闲',
};

/** Team execution state is server-owned; this view never infers completion from text. */
export default function TeamBoard() {
  const team = useCollaborationStore((state) => state.team);
  const agents = useCollaborationStore((state) => state.agents);
  const messages = useCollaborationStore((state) => state.messages);
  const error = useCollaborationStore((state) => state.error);
  const permissions = useChatStore((state) => state.pendingPermissions);
  const [expanded, setExpanded] = useState(false);
  const [mailbox, setMailbox] = useState(false);
  const [busy, setBusy] = useState(false);
  const [controlError, setControlError] = useState<string | null>(null);
  const root = agents.find((agent) => agent.role === 'root');
  const members = agents.filter((agent) => agent.role !== 'root');
  const rootMessages = root ? messages.filter((message) => message.toAgentId === root.id && message.source === 'agent') : [];
  const childPermissions = permissions.filter((permission) => permission.agentId && permission.agentId !== root?.id);
  const controllable = team && ['active', 'paused', 'recoveryRequired', 'blocked'].includes(team.status);
  if (!team && members.length === 0 && rootMessages.length === 0 && childPermissions.length === 0) return null;
  const control = async (action: TeamAction) => {
    setBusy(true);
    setControlError(null);
    try { await useCollaborationStore.getState().control(action); }
    catch (reason) { setControlError(reason instanceof Error ? reason.message : '操作失败'); }
    finally { setBusy(false); }
  };
  return <section data-testid="team-board" className="rounded-lg border border-edge bg-shell-sunk text-[11px] text-fg-3">
    <div className="flex flex-wrap items-center gap-2 px-2 py-1.5">
      <button type="button" data-testid="team-toggle" aria-expanded={expanded} onClick={() => setExpanded((value) => !value)} className="min-w-0 flex-1 truncate text-left text-fg">
        {expanded ? '▾' : '▸'} {team?.name ?? 'Agent 协作'} · {team ? statusLabels[team.status] : `${members.length} 位成员`}
      </button>
      {root && <button type="button" data-testid="team-inbox-toggle" onClick={() => setMailbox((value) => !value)}>消息 {rootMessages.length}</button>}
      {team && <>
        {controllable && <button type="button" data-testid="team-pause-resume" disabled={busy} onClick={() => void control(team.status === 'active' ? 'pause' : 'resume')} className="rounded border border-edge px-1.5 py-0.5 disabled:opacity-40">{team.status === 'active' ? '暂停' : '恢复'}</button>}
        {controllable && <button type="button" data-testid="team-stop" disabled={busy} onClick={() => void control('stop')} className="rounded border border-edge px-1.5 py-0.5 disabled:opacity-40">停止</button>}
      </>}
    </div>
    {expanded && <div className="max-h-64 space-y-2 overflow-y-auto border-t border-edge p-2">
      {team && <p>并发上限 {team.maxParallel} · 修复 {team.fixRounds}/{team.maxFixRounds}{team.status === 'paused' ? ' · 暂停派工，当前任务可完成' : ''}</p>}
      <div className="flex flex-wrap gap-1">
        {members.map((agent) => <button key={agent.id} type="button" onClick={() => useChatStore.getState().openSubagent(agent.id)} className="rounded border border-edge px-2 py-1 text-fg hover:bg-shell-hover">{agent.name} · {statusLabels[agent.status] ?? agent.status}</button>)}
      </div>
      {team?.tasks.map((task) => <div key={task.id} data-testid="team-task" className="rounded-md border border-edge bg-shell-panel px-2 py-1.5">
        <div className="flex justify-between gap-2"><span className="text-fg">{task.title}</span><span>{statusLabels[task.status]}</span></div>
        <div className="mt-1 flex flex-wrap gap-x-3 gap-y-1">
          <span>负责人：{agents.find((agent) => agent.id === task.ownerAgentId)?.name ?? task.ownerAgentId ?? '待认领'}</span>
          {task.stage && <span>阶段：{task.stage}</span>}
          {task.deps.length > 0 && <span>依赖：{task.deps.map((id) => team.tasks.find((candidate) => candidate.id === id)?.title ?? id).join('、')}</span>}
        </div>
        {task.result && (task.status === 'failed' || task.status === 'blocked') && <p className="mt-1 whitespace-pre-wrap">{task.result}</p>}
      </div>)}
      {error && <p role="status">状态读取失败：{error}</p>}
    </div>}
    {mailbox && root && <div className="border-t border-edge p-2"><AgentMailbox key={root.id} agent={root} input={false} /></div>}
    {childPermissions.length > 0 && <div data-testid="team-pending-approvals" className="max-h-72 space-y-2 overflow-y-auto border-t border-edge p-2">
      {childPermissions.map((permission) => <ApprovalCard key={permission.id} block={{ ...permission, kind: 'approval' }} />)}
    </div>}
    {controlError && <p role="alert" className="p-2">{controlError}</p>}
  </section>;
}
