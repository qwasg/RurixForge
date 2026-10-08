import { useEffect, useRef, useState } from 'react';
import type { AgentParticipant } from '@forge/protocol';
import { MESSAGE_STATUS_LABEL, useCollaborationStore } from '@/lib/collaborationStore';
import { annotationDraftKey, decodeAnnotationDrop, EDITOR_REFERENCE_MIME, useEditorAnnotationStore } from '@/lib/editorReferences';
import { useWorkspaceStore } from '@/lib/workspaceStore';
import { useComposerTextDraft } from '@/lib/composerStore';
import { flushEditorDocuments } from '@/lib/editorDocuments';
import AnnotationChips from './AnnotationChips';

/** A mailbox belongs to an agent identity, so idle members remain addressable. */
export default function AgentMailbox({ agent, input = true }: { agent: AgentParticipant; input?: boolean }) {
  const messages = useCollaborationStore((state) => state.messages);
  const agents = useCollaborationStore((state) => state.agents);
  const workspaceId = useWorkspaceStore((s) => s.activeWorkspaceId);
  const key = `${annotationDraftKey(agent.sessionId, workspaceId)}:${agent.id}`;
  const [text, setText] = useComposerTextDraft(key);
  const annotationDrafts = useEditorAnnotationStore((s) => s.drafts);
  const annotations = annotationDrafts[key] ?? [];
  const [error, setError] = useState<string | null>(null);
  const [sending, setSending] = useState(false);
  const attempt = useRef<{ text: string; id: string } | null>(null);
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    let cancelled = false;
    void useCollaborationStore.getState().loadMessages(agent.id).catch((reason: unknown) => {
      if (!cancelled) setError(reason instanceof Error ? reason.message : '消息历史读取失败');
    });
    return () => { cancelled = true; mounted.current = false; };
  }, [agent.id]);

  const send = async () => {
    const rawText = text.trim();
    const body = rawText || '请处理附带的编辑器批注。';
    if ((!rawText && !annotations.length) || sending || agent.status === 'stopped') return;
    const sent = structuredClone(annotations);
    const identity = JSON.stringify([body, sent]);
    const id = attempt.current?.text === identity ? attempt.current.id : crypto.randomUUID();
    attempt.current = { text: identity, id };
    setSending(true);
    setError(null);
    try {
      if (sent.length) await flushEditorDocuments(sent);
      await useCollaborationStore.getState().send(agent.id, {
        text: body, clientMessageId: id,
        ...(sent.length ? { annotations: sent } : {}),
        ...(agent.activeRunId ? { expectedRunId: agent.activeRunId } : {}),
      });
      if (mounted.current) {
        setText((current) => current === text ? '' : current);
        useEditorAnnotationStore.getState().acknowledge(sent, key);
        attempt.current = null;
      }
    } catch (reason) {
      if (mounted.current) setError(reason instanceof Error ? reason.message : '发送失败，草稿已保留');
    } finally {
      if (mounted.current) setSending(false);
    }
  };
  const nameOf = (id: string | null | undefined) => agents.find((participant) => participant.id === id)?.name ?? id ?? '用户';
  const history = messages.filter((message) => message.toAgentId === agent.id || message.fromAgentId === agent.id);

  return <div data-testid="agent-mailbox" className="flex flex-col gap-2 text-[12px]" onDragOver={(e) => { if(input && e.dataTransfer.types.includes(EDITOR_REFERENCE_MIME))e.preventDefault(); }} onDrop={(e) => { if(input && e.dataTransfer.types.includes(EDITOR_REFERENCE_MIME)){e.preventDefault();useEditorAnnotationStore.getState().add(decodeAnnotationDrop(e.dataTransfer),key);} }}>
    <div className="max-h-48 space-y-2 overflow-y-auto" aria-live="polite">
      {history.length === 0 && <p className="text-fg-4">暂无消息</p>}
      {history.map((message) => <div key={message.id} data-testid="agent-mailbox-message" className="rounded-md border border-edge bg-shell-sunk px-2 py-1.5">
        <div className="flex flex-wrap justify-between gap-1 text-[10px] text-fg-3">
          <span>{message.source === 'user' ? '用户' : nameOf(message.fromAgentId)} → {nameOf(message.toAgentId)}{message.kind === 'receipt' ? ' · 系统回执' : ''}</span>
          <span>{MESSAGE_STATUS_LABEL[message.status] ?? message.status}</span>
        </div>
        <p className="whitespace-pre-wrap break-words text-fg">{message.text}</p>
        {!!message.annotations?.length && <AnnotationChips annotations={message.annotations} />}
        {message.error && <p className="text-fg-3">{message.error}</p>}
      </div>)}
    </div>
    {input && annotations.length > 0 && <AnnotationChips annotations={annotations} onRemove={(id)=>useEditorAnnotationStore.getState().remove(id,key)} onNote={(id,note)=>useEditorAnnotationStore.getState().update(id,note,key)} />}
    {input && <form onSubmit={(event) => { event.preventDefault(); void send(); }} className="flex items-end gap-2">
      <textarea aria-label={`发送消息给 ${agent.name}`} data-testid="agent-message-input" value={text} onChange={(event) => setText(event.target.value)} rows={2}
        placeholder={agent.status === 'stopped' ? '成员已关闭' : `给 ${agent.name} 补充要求…`}
        disabled={agent.status === 'stopped'} className="min-w-0 flex-1 resize-y rounded-md border border-edge bg-shell-input p-2 text-fg outline-none focus:border-acc-ring" />
      <button type="submit" data-testid="agent-message-send" disabled={(!text.trim() && !annotations.length) || sending || agent.status === 'stopped'} className="rounded-md bg-acc px-2 py-1.5 text-fg-inv disabled:opacity-40">{sending ? '发送中' : '发送'}</button>
    </form>}
    {error && <p role="alert" className="text-fg-3">{error}</p>}
  </div>;
}
