import { useMemo, useState } from 'react';
import { Check, FileWarning, ShieldQuestion, Terminal, X } from 'lucide-react';
import { useChatStore } from '@/lib/chatStore';
import type { ChatBlock } from '@/lib/timeline';
import { cn } from '@/lib/cn';

type Approval = Extract<ChatBlock, { kind: 'approval' }>;
type Decision = 'accept' | 'acceptForSession' | 'decline';
type Answers = Record<string, unknown>;

interface SchemaField {
  type?: unknown;
  title?: unknown;
  description?: unknown;
  default?: unknown;
  format?: unknown;
  enum?: unknown;
  enumNames?: unknown;
  oneOf?: unknown;
  items?: unknown;
  minimum?: unknown;
  maximum?: unknown;
  minLength?: unknown;
  maxLength?: unknown;
  minItems?: unknown;
  maxItems?: unknown;
}

function schemaProperties(schema: Record<string, unknown> | undefined): Array<[string, SchemaField]> {
  const properties = schema?.properties;
  if (!properties || typeof properties !== 'object' || Array.isArray(properties)) return [];
  return Object.entries(properties as Record<string, unknown>).flatMap(([id, value]) =>
    value && typeof value === 'object' && !Array.isArray(value)
      ? [[id, value as SchemaField]]
      : [],
  );
}

function schemaRequired(schema: Record<string, unknown> | undefined): string[] {
  return Array.isArray(schema?.required)
    ? schema.required.filter((value): value is string => typeof value === 'string')
    : [];
}

function schemaChoices(field: SchemaField): Array<{ value: string; label: string }> {
  if (Array.isArray(field.enum)) {
    const names = Array.isArray(field.enumNames) ? field.enumNames : [];
    return field.enum.flatMap((value, index) =>
      typeof value === 'string'
        ? [{ value, label: typeof names[index] === 'string' ? names[index] : value }]
        : [],
    );
  }
  if (Array.isArray(field.oneOf)) {
    return field.oneOf.flatMap((value) => {
      if (!value || typeof value !== 'object' || Array.isArray(value)) return [];
      const option = value as Record<string, unknown>;
      return typeof option.const === 'string'
        ? [{ value: option.const, label: typeof option.title === 'string' ? option.title : option.const }]
        : [];
    });
  }
  const items = field.items;
  if (!items || typeof items !== 'object' || Array.isArray(items)) return [];
  const item = items as SchemaField;
  if (Array.isArray(item.enum)) return schemaChoices(item);
  const anyOf = (items as Record<string, unknown>).anyOf;
  if (!Array.isArray(anyOf)) return [];
  return anyOf.flatMap((value) => {
    if (!value || typeof value !== 'object' || Array.isArray(value)) return [];
    const option = value as Record<string, unknown>;
    return typeof option.const === 'string'
      ? [{ value: option.const, label: typeof option.title === 'string' ? option.title : option.const }]
      : [];
  });
}

function initialAnswers(schema: Record<string, unknown> | undefined): Answers {
  return Object.fromEntries(
    schemaProperties(schema).flatMap(([id, field]) =>
      field.default !== undefined && field.default !== null ? [[id, field.default]] : [],
    ),
  );
}

function isAnswered(value: unknown): boolean {
  if (typeof value === 'string') return value.trim() !== '';
  if (Array.isArray(value)) return value.length > 0;
  return value !== undefined && value !== null;
}

function schemaValueValid(field: SchemaField, value: unknown): boolean {
  if (!isAnswered(value)) return false;
  if (field.type === 'number' || field.type === 'integer') {
    if (typeof value !== 'number' || !Number.isFinite(value)) return false;
    if (field.type === 'integer' && !Number.isInteger(value)) return false;
    if (typeof field.minimum === 'number' && value < field.minimum) return false;
    if (typeof field.maximum === 'number' && value > field.maximum) return false;
  }
  if (field.type === 'string' && typeof value === 'string') {
    if (typeof field.minLength === 'number' && value.length < field.minLength) return false;
    if (typeof field.maxLength === 'number' && value.length > field.maxLength) return false;
  }
  if (field.type === 'array' && Array.isArray(value)) {
    if (typeof field.minItems === 'number' && value.length < field.minItems) return false;
    if (typeof field.maxItems === 'number' && value.length > field.maxItems) return false;
  }
  return true;
}

function prettyJson(value: Record<string, unknown>): string {
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
}

function safeHttpUrl(value: unknown): string | null {
  if (typeof value !== 'string' || value.trim() === '') return null;
  try {
    const url = new URL(value);
    return url.protocol === 'http:' || url.protocol === 'https:' ? url.href : null;
  } catch {
    return null;
  }
}

function approvalTitle(block: Approval): string {
  if (block.approvalKind === 'command') return '允许执行命令？';
  if (block.approvalKind === 'fileChange') return '允许修改文件？';
  if (block.approvalKind === 'userInput') return '需要你的回答';
  if (block.approvalKind === 'elicitation') return '需要你的确认';
  return '需要授权';
}

function approvalIcon(block: Approval) {
  if (block.approvalKind === 'command') return Terminal;
  if (block.approvalKind === 'fileChange') return FileWarning;
  return ShieldQuestion;
}

function SchemaControl({
  id,
  field,
  value,
  required,
  onChange,
}: {
  id: string;
  field: SchemaField;
  value: unknown;
  required: boolean;
  onChange: (value: unknown) => void;
}) {
  const title = typeof field.title === 'string' && field.title !== '' ? field.title : id;
  const description = typeof field.description === 'string' ? field.description : undefined;
  const choices = schemaChoices(field);
  const commonClass = 'rounded-md border border-edge bg-shell-panel px-2 py-1.5 text-[11px] text-fg outline-none focus:border-acc-ring';
  let control;

  if (field.type === 'boolean') {
    control = (
      <select
        data-testid={`approval-schema-${id}`}
        value={value === true ? 'true' : value === false ? 'false' : ''}
        onChange={(event) => onChange(event.target.value === '' ? undefined : event.target.value === 'true')}
        className={commonClass}
      >
        <option value="">请选择</option>
        <option value="true">是</option>
        <option value="false">否</option>
      </select>
    );
  } else if (field.type === 'array') {
    const selected = Array.isArray(value) ? value.filter((item): item is string => typeof item === 'string') : [];
    control = choices.length > 0 ? (
      <span data-testid={`approval-schema-${id}`} className="flex flex-wrap gap-1.5">
        {choices.map((option) => {
          const checked = selected.includes(option.value);
          return (
            <label key={option.value} className={cn(
              'flex cursor-pointer items-center gap-1 rounded-md border px-2 py-1 text-[10.5px]',
              checked ? 'border-acc-ring bg-acc-bg text-acc' : 'border-edge bg-shell-panel text-fg-3',
            )}>
              <input
                type="checkbox"
                checked={checked}
                onChange={() => onChange(checked ? selected.filter((item) => item !== option.value) : [...selected, option.value])}
                className="sr-only"
              />
              {option.label}
            </label>
          );
        })}
      </span>
    ) : (
      <input
        data-testid={`approval-schema-${id}`}
        value={selected.join(', ')}
        onChange={(event) => onChange(event.target.value.split(',').map((item) => item.trim()).filter(Boolean))}
        placeholder="用逗号分隔多个值"
        className={commonClass}
      />
    );
  } else if (choices.length > 0) {
    control = (
      <select
        data-testid={`approval-schema-${id}`}
        value={typeof value === 'string' ? value : ''}
        onChange={(event) => onChange(event.target.value === '' ? undefined : event.target.value)}
        className={commonClass}
      >
        <option value="">请选择</option>
        {choices.map((option) => <option key={option.value} value={option.value}>{option.label}</option>)}
      </select>
    );
  } else if (field.type === 'number' || field.type === 'integer') {
    control = (
      <input
        data-testid={`approval-schema-${id}`}
        type="number"
        step={field.type === 'integer' ? 1 : 'any'}
        min={typeof field.minimum === 'number' ? field.minimum : undefined}
        max={typeof field.maximum === 'number' ? field.maximum : undefined}
        value={typeof value === 'number' && Number.isFinite(value) ? value : ''}
        onChange={(event) => {
          const parsed = Number(event.target.value);
          onChange(event.target.value !== '' && Number.isFinite(parsed) ? parsed : undefined);
        }}
        className={commonClass}
      />
    );
  } else {
    const htmlType = field.format === 'email'
      ? 'email'
      : field.format === 'uri'
        ? 'url'
        : field.format === 'date'
          ? 'date'
          : field.format === 'date-time'
            ? 'datetime-local'
            : 'text';
    control = (
      <input
        data-testid={`approval-schema-${id}`}
        type={htmlType}
        minLength={typeof field.minLength === 'number' ? field.minLength : undefined}
        maxLength={typeof field.maxLength === 'number' ? field.maxLength : undefined}
        value={typeof value === 'string' ? value : ''}
        onChange={(event) => onChange(event.target.value === '' ? undefined : event.target.value)}
        className={commonClass}
      />
    );
  }

  return (
    <label className="flex flex-col gap-1.5">
      <span className="text-[11.5px] text-fg-2">{title}{required ? ' *' : ''}</span>
      {description && <span className="text-[10px] leading-[15px] text-fg-4">{description}</span>}
      {control}
    </label>
  );
}

/** app-server requestApproval / requestUserInput 的内联卡；resolved 后保留决定作为审计线索。 */
export default function ApprovalCard({ block }: { block: Approval }) {
  const resolvePermission = useChatStore((state) => state.resolvePermission);
  const [answers, setAnswers] = useState<Answers>(() => initialAnswers(block.schema));
  const [otherQuestions, setOtherQuestions] = useState<Record<string, boolean>>({});
  const [externalFlowConfirmed, setExternalFlowConfirmed] = useState(false);
  const [sending, setSending] = useState(false);
  const Icon = approvalIcon(block);
  const resolved = block.decision !== undefined;
  const questions = block.questions ?? [];
  const decisions = block.availableDecisions ?? ['accept', 'acceptForSession', 'decline'];
  const fields = useMemo(() => schemaProperties(block.schema), [block.schema]);
  const requiredFields = useMemo(() => schemaRequired(block.schema), [block.schema]);
  const safeExternalUrl = safeHttpUrl(block.url);
  const policyDetails = useMemo(() => ({
    ...(block.networkApprovalContext ? { networkApprovalContext: block.networkApprovalContext } : {}),
    ...(block.proposedExecpolicyAmendment ? { proposedExecpolicyAmendment: block.proposedExecpolicyAmendment } : {}),
    ...(block.proposedNetworkPolicyAmendments
      ? { proposedNetworkPolicyAmendments: block.proposedNetworkPolicyAmendments }
      : {}),
  }), [block.networkApprovalContext, block.proposedExecpolicyAmendment, block.proposedNetworkPolicyAmendments]);
  const urlFlowIncomplete = block.mode === 'url' && (!safeExternalUrl || !externalFlowConfirmed);
  const missingRequired =
    questions.some((question) =>
      (question.required === true || (question.required === undefined && block.approvalKind === 'userInput')) &&
      !isAnswered(answers[question.id]),
    ) ||
    fields.some(([id, field]) =>
      (requiredFields.includes(id) || isAnswered(answers[id])) && !schemaValueValid(field, answers[id]),
    ) ||
    urlFlowIncomplete;

  const setAnswer = (id: string, value: unknown) => {
    setAnswers((current) => {
      if (value === undefined) {
        const next = { ...current };
        delete next[id];
        return next;
      }
      return { ...current, [id]: value };
    });
  };

  const decide = async (decision: Decision) => {
    if (resolved || sending || (decision !== 'decline' && missingRequired)) return;
    setSending(true);
    try {
      await resolvePermission(block.id, decision !== 'decline', { decision, answers });
    } finally {
      setSending(false);
    }
  };

  return (
    <section
      data-testid={`approval-card-${block.id}`}
      className="my-1 overflow-hidden rounded-[10px] border border-edge-strong bg-shell-sunk shadow-sh1"
    >
      <div className="flex items-start gap-2.5 px-3 py-2.5">
        <span className="mt-px flex h-6 w-6 shrink-0 items-center justify-center rounded-md bg-warn-bg text-warn">
          <Icon size={13} />
        </span>
        <div className="min-w-0 flex-1">
          <div className="text-[12.5px] font-medium text-fg">{approvalTitle(block)}</div>
          {(block.reason || block.message || block.tool) && (
            <div className="mt-0.5 text-[11px] leading-[16px] text-fg-3">
              {block.reason || block.message || block.tool}
            </div>
          )}
          {(block.serverName || block.elicitationId) && (
            <div className="mt-1 flex flex-wrap gap-x-2 font-code text-[9.5px] text-fg-4">
              {block.serverName && <span>server: {block.serverName}</span>}
              {block.elicitationId && <span>request: {block.elicitationId}</span>}
            </div>
          )}
          {block.command && (
            <pre className="mt-2 overflow-auto rounded-md border border-edge bg-shell-panel px-2 py-1.5 font-code text-[10.5px] text-fg-2">
              {block.command}
            </pre>
          )}
          {block.cwd && <div className="mt-1 truncate font-code text-[9.5px] text-fg-4">{block.cwd}</div>}
          {block.grantRoot && (
            <div data-testid="approval-grant-root" className="mt-2 rounded-md border border-warn/30 bg-warn-bg px-2 py-1.5 text-[10.5px] text-warn">
              本会话将允许写入：<span className="font-code">{block.grantRoot}</span>
            </div>
          )}
          {block.changes && block.changes.length > 0 && (
            <div className="mt-2 flex flex-col gap-1">
              {block.changes.map((change, index) => (
                <div key={`${change.path}:${index}`} className="flex items-center gap-2 font-code text-[10.5px] text-fg-3">
                  <span className="min-w-0 flex-1 truncate">{change.path}</span>
                  {change.kind && <span className="uppercase text-fg-4">{change.kind}</span>}
                </div>
              ))}
            </div>
          )}
          {block.permissions && Object.keys(block.permissions).length > 0 && (
            <div data-testid="approval-permissions" className="mt-2">
              <div className="mb-1 text-[10px] font-medium text-fg-3">请求的权限</div>
              <pre className="max-h-36 overflow-auto rounded-md border border-edge bg-shell-panel px-2 py-1.5 font-code text-[10px] whitespace-pre-wrap text-fg-3">
                {prettyJson(block.permissions)}
              </pre>
            </div>
          )}
          {Object.keys(policyDetails).length > 0 && (
            <div data-testid="approval-policy-amendments" className="mt-2">
              <div className="mb-1 text-[10px] font-medium text-warn">可能扩大后续命令或网络权限</div>
              <pre className="max-h-40 overflow-auto rounded-md border border-warn/30 bg-warn-bg px-2 py-1.5 font-code text-[10px] whitespace-pre-wrap text-fg-3">
                {prettyJson(policyDetails)}
              </pre>
            </div>
          )}
          {block.url && (
            <div data-testid="approval-external-flow" className="mt-2 rounded-md border border-edge bg-shell-panel px-2 py-2 text-[10.5px] text-fg-3">
              {safeExternalUrl ? (
                <a
                  data-testid="approval-external-link"
                  href={safeExternalUrl}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="text-acc hover:underline"
                >
                  打开外部授权页面
                </a>
              ) : (
                <div className="text-warn">外部地址不是安全的 HTTP(S) URL，已阻止打开。</div>
              )}
              {block.mode === 'url' && safeExternalUrl && (
                <label className="mt-2 flex cursor-pointer items-center gap-2">
                  <input
                    data-testid="approval-external-confirm"
                    type="checkbox"
                    checked={externalFlowConfirmed}
                    onChange={(event) => setExternalFlowConfirmed(event.target.checked)}
                  />
                  我已在外部页面完成操作
                </label>
              )}
            </div>
          )}
        </div>
      </div>

      {!resolved && questions.length > 0 && (
        <div className="flex flex-col gap-2 border-t border-edge px-3 py-2.5">
          {questions.map((question, index) => {
            const id = question.id || `question-${index}`;
            const required = question.required === true || (question.required === undefined && block.approvalKind === 'userInput');
            const choices = question.options ?? [];
            const hasOtherChoice = question.isOther === true || choices.some((option) => option.isOther === true);
            return (
              <label key={id} className="flex flex-col gap-1.5">
                <span className="text-[11.5px] text-fg-2">{question.question || question.header || id}{required ? ' *' : ''}</span>
                {question.header && question.question && (
                  <span className="text-[10px] text-fg-4">{question.header}</span>
                )}
                {choices.length > 0 ? (
                  <>
                    <span className="flex flex-wrap gap-1.5">
                    {choices.map((option) => {
                      const isOther = option.isOther === true;
                      return (
                      <button
                        key={option.label}
                        type="button"
                        title={option.description}
                        onClick={() => {
                          setOtherQuestions((current) => ({ ...current, [id]: isOther }));
                          setAnswer(id, isOther ? undefined : option.label);
                        }}
                        className={cn(
                          'rounded-md border px-2 py-1 text-[10.5px]',
                          (!isOther && answers[id] === option.label) || (isOther && otherQuestions[id])
                            ? 'border-acc-ring bg-acc-bg text-acc'
                            : 'border-edge bg-shell-panel text-fg-3 hover:bg-shell-hover',
                        )}
                      >
                        {option.label}
                      </button>
                      );
                    })}
                    {question.isOther === true && !choices.some((option) => option.isOther === true) && (
                      <button
                        type="button"
                        onClick={() => {
                          setOtherQuestions((current) => ({ ...current, [id]: true }));
                          setAnswer(id, undefined);
                        }}
                        className={cn(
                          'rounded-md border px-2 py-1 text-[10.5px]',
                          otherQuestions[id]
                            ? 'border-acc-ring bg-acc-bg text-acc'
                            : 'border-edge bg-shell-panel text-fg-3 hover:bg-shell-hover',
                        )}
                      >
                        其他
                      </button>
                    )}
                    </span>
                    {hasOtherChoice && otherQuestions[id] && (
                      <input
                        data-testid={`approval-question-${id}`}
                        type={question.isSecret || question.secret ? 'password' : 'text'}
                        value={typeof answers[id] === 'string' ? answers[id] : ''}
                        onChange={(event) => setAnswer(id, event.target.value)}
                        placeholder="请输入其他回答"
                        autoComplete={question.isSecret || question.secret ? 'off' : undefined}
                        className="rounded-md border border-edge bg-shell-panel px-2 py-1.5 text-[11px] text-fg outline-none focus:border-acc-ring"
                      />
                    )}
                  </>
                ) : (
                  <input
                    data-testid={`approval-question-${id}`}
                    type={question.isSecret || question.secret ? 'password' : 'text'}
                    value={typeof answers[id] === 'string' ? answers[id] : ''}
                    onChange={(event) => setAnswer(id, event.target.value)}
                    autoComplete={question.isSecret || question.secret ? 'off' : undefined}
                    className="rounded-md border border-edge bg-shell-panel px-2 py-1.5 text-[11px] text-fg outline-none focus:border-acc-ring"
                  />
                )}
              </label>
            );
          })}
        </div>
      )}

      {!resolved && fields.length > 0 && (
        <div data-testid="approval-schema-form" className="flex flex-col gap-2 border-t border-edge px-3 py-2.5">
          {fields.map(([id, field]) => (
            <SchemaControl
              key={id}
              id={id}
              field={field}
              value={answers[id]}
              required={requiredFields.includes(id)}
              onChange={(value) => setAnswer(id, value)}
            />
          ))}
        </div>
      )}

      {!resolved && block.schema && fields.length === 0 && (
        <details className="border-t border-edge px-3 py-2 text-[10px] text-fg-3">
          <summary className="cursor-pointer">查看请求表单定义</summary>
          <pre className="mt-1 max-h-36 overflow-auto whitespace-pre-wrap font-code text-fg-4">{prettyJson(block.schema)}</pre>
        </details>
      )}

      {resolved ? (
        <div
          data-testid="approval-decision"
          className="flex items-center gap-1.5 border-t border-edge px-3 py-2 text-[11px] text-fg-3"
        >
          {block.decision === 'decline' || block.decision === 'expired'
            ? <X size={11} />
            : <Check size={11} />}
          {block.decision === 'acceptForSession'
            ? '已允许（本会话）'
            : block.decision === 'decline'
              ? '已拒绝'
              : block.decision === 'expired'
                ? '已失效'
                : '已允许'}
        </div>
      ) : (
        <div className="flex flex-wrap items-center justify-end gap-1.5 border-t border-edge px-3 py-2">
          {decisions.includes('decline') && (
            <button
              type="button"
              data-testid="approval-decline"
              disabled={sending}
              onClick={() => void decide('decline')}
              className="h-[25px] rounded-md border border-edge px-2 text-[11px] text-fg-3 hover:bg-shell-hover disabled:opacity-50"
            >
              拒绝
            </button>
          )}
          {decisions.includes('acceptForSession') && (
            <button
              type="button"
              data-testid="approval-session"
              disabled={sending || missingRequired}
              title={missingRequired ? '请先完成必填项或外部流程' : undefined}
              onClick={() => void decide('acceptForSession')}
              className="h-[25px] rounded-md border border-edge bg-shell-panel px-2 text-[11px] text-fg-2 hover:bg-shell-hover disabled:opacity-50"
            >
              本会话允许
            </button>
          )}
          {decisions.includes('accept') && (
            <button
              type="button"
              data-testid="approval-accept"
              disabled={sending || missingRequired}
              title={missingRequired ? '请先完成必填项或外部流程' : undefined}
              onClick={() => void decide('accept')}
              className="h-[25px] rounded-md border border-acc bg-acc px-2.5 text-[11px] text-fg-inv hover:bg-acc-soft disabled:opacity-50"
            >
              {block.mode === 'url' ? '已完成并继续' : '允许'}
            </button>
          )}
        </div>
      )}
    </section>
  );
}
