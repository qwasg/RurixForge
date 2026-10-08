import { useCallback, useEffect, useRef, useState } from 'react';
import { ExternalLink, Gamepad2, Loader2, RotateCcw, RotateCw, Square, TriangleAlert } from 'lucide-react';
import {
  DEMO_MANUAL_NOTE,
  DEMO_RESET_NOTE,
  DemoDecisionControls,
  ProbeErrors,
  VerifiedBadge,
  demoBlockReason,
  readProbe,
} from '@/components/chat/ultraplan/DemoReview';
import { useFlowContext } from '@/components/chat/ultraplan/flowContext';
import { useChatStore, type ChatMsg } from '@/lib/chatStore';
import { useSessionStore } from '@/lib/sessionStore';
import type { ChatBlock } from '@/lib/timeline';
import {
  actionErrorText,
  demoUrl,
  fetchDemoFace,
  isLoopbackHost,
  useUltraPlanStore,
  type DemoFace,
  type UltraPlanState,
} from '@/lib/ultraPlanStore';
import { demoTabTitle, useWorkbenchStore } from '@/lib/workbenchStore';

type Ultra = Extract<ChatBlock, { kind: 'ultraplan' }>;

/** iframe 沙箱(契约 §8):不给 allow-popups / allow-top-navigation / allow-forms / allow-modals。 */
export const DEMO_SANDBOX = 'allow-scripts allow-same-origin allow-pointer-lock';

/** 页签加载不出 Demo 的原因(I-5:如实给码 + 说明,warn 色)。 */
export interface DemoViewError {
  code: string;
  message: string;
}

/**
 * 页签该显示什么:加载中 / 某个错误 / 可加载的地址。纯函数,地址只经 ultraPlanStore.demoUrl()
 * 拼出并通过它的断言(http、回环、与应用不同源、路径在 /u/<token>/ 下),任何一条不成立都不给地址。
 */
export function resolveDemoView(input: {
  upId: string;
  /** 还没解析完过一次(首次解析在途)。 */
  loading: boolean;
  /** 取回的流程面;null = 取不到(离线 / 旧后端)。 */
  face: DemoFace | null;
  locationHostname: string;
  locationOrigin: string;
}): { kind: 'loading' } | { kind: 'error'; error: DemoViewError } | { kind: 'ready'; url: string; flow: UltraPlanState } {
  const { face } = input;
  // 首次解析回来之前:快照只回填了阶段、没有 Demo 坐标,此时不能报「没有 Demo」。
  if (input.loading && !face?.demo) return { kind: 'loading' };
  if (!isLoopbackHost(input.locationHostname)) {
    return {
      kind: 'error',
      error: {
        code: 'DEMO_HOST_LOCAL_ONLY',
        message: 'Demo 只在引擎所在的本机上提供(回环地址);当前页面不是经本机回环地址打开的,无法加载。',
      },
    };
  }
  if (!face) {
    return {
      kind: 'error',
      error: { code: 'DEMO_INFO_UNAVAILABLE', message: '取不到 Demo 信息(引擎离线或版本过旧),请稍后点「重新加载」。' },
    };
  }
  if (face.faceError) return { kind: 'error', error: face.faceError };
  const flow = face.state;
  if (!flow || flow.id !== input.upId) {
    return {
      kind: 'error',
      error: { code: 'ULTRAPLAN_FLOW_GONE', message: '该 Demo 所属的 UltraPlan 流程已不存在(可能已重新开始)。' },
    };
  }
  if (face.demoError) {
    return {
      kind: 'error',
      error: { code: face.demoError.code, message: face.demoError.message || 'Demo 服务没有启动。' },
    };
  }
  if (!face.demo || flow.demoIteration < 1) {
    return {
      kind: 'error',
      error: {
        code: 'DEMO_MISSING',
        message:
          flow.phase === 'running' && flow.running === 'spec_demo'
            ? 'Demo 正在构建,完成后会自动出现。'
            : '这个流程还没有可玩的 Demo。',
      },
    };
  }
  // token 与流程状态里的对不上(不该发生)= 不是这个流程的 Demo,宁可不加载。
  const tokenMismatch = flow.token !== '' && face.demo.token !== flow.token;
  const url = tokenMismatch
    ? null
    : demoUrl(face.demo, input.locationHostname, flow.demoIteration, input.locationOrigin);
  if (url === null) {
    return {
      kind: 'error',
      error: {
        code: 'DEMO_URL_REJECTED',
        message: 'Demo 地址没有通过安全校验(须为 http 回环地址、与应用不同源、路径在 /u/<token>/ 下),已拒绝加载。',
      },
    };
  }
  return { kind: 'ready', url, flow };
}

/** 当前会话里这一版的 Demo 卡(取探测报错用;从新往旧找)。 */
function findDemoBlock(messages: ChatMsg[], upId: string, iteration: number): Ultra | null {
  for (let i = messages.length - 1; i >= 0; i -= 1) {
    const blocks = messages[i].blocks;
    for (let j = blocks.length - 1; j >= 0; j -= 1) {
      const b = blocks[j];
      if (b.kind === 'ultraplan' && b.step === 'demo' && b.upId === upId && b.rev === iteration) return b;
    }
  }
  return null;
}

/**
 * D-044 Demo 页签:UltraPlan 流程的网页 Demo 试玩页(应用里第一个 iframe)。
 *
 * - 坐标:挂载时与 iframe 加载出错后重新解析——本会话走 ultraPlanStore.refresh(结果落 store),
 *   别的会话的页签走 fetchDemoFace(只读,不落 store)。URL 只由 demoUrl() 在前端拼(事件里
 *   从不带地址,端口每次重启都变),取与应用**不同**的回环主机名,让 Demo 跨站进独立渲染进程。
 * - iframe:沙箱 allow-scripts allow-same-origin allow-pointer-lock、allow 为空、不带 referrer;
 *   按迭代号(+ 重新加载计数)换 key,src 带 ?v=<迭代号>。
 * - 工具条:重新加载(重新解析 + 换 key)、停止(卸载 iframe)、在系统浏览器打开(Electron 把
 *   跨源 http 交给系统浏览器)、「第 N 版」、自动验证徽标、探测报错。
 * - 动作:通过 Demo / 提出修改 / 回到上一版(就地二次确认),条件见 demoBlockReason;
 *   页签属于别的会话时只能看不能点。
 */
export default function DemoTab({
  upId,
  sessionId,
  tabId,
  title,
}: {
  upId: string;
  sessionId: string;
  /** 所在页签(流程标题取回后回填 tabbar 文案)。 */
  tabId?: string;
  /** 页签标题(流程还没取回时的回落)。 */
  title?: string;
}) {
  const { activeSessionId, flow: activeFlow, activeRunId, unsupportedReason } = useFlowContext();
  const own = sessionId === activeSessionId;
  const storeSessionId = useUltraPlanStore((st) => st.sessionId);
  const storeState = useUltraPlanStore((st) => st.state);
  const storeDemo = useUltraPlanStore((st) => st.demo);
  const storeDemoError = useUltraPlanStore((st) => st.demoError);
  const storeFaceError = useUltraPlanStore((st) => st.faceError);
  const pending = useUltraPlanStore((st) => st.pending !== null);

  const [foreign, setForeign] = useState<DemoFace | null>(null);
  /** 首次解析完成于哪种来源(own / foreign);切换来源后回到加载态。 */
  const [resolvedFor, setResolvedFor] = useState<'own' | 'foreign' | null>(null);
  const [nonce, setNonce] = useState(0);
  const [stopped, setStopped] = useState(false);
  const [confirmRollback, setConfirmRollback] = useState(false);
  const [rollingBack, setRollingBack] = useState(false);
  const [rollbackError, setRollbackError] = useState<string | null>(null);
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  const resolve = useCallback(async () => {
    // 读「此刻」的当前会话,不闭包渲染时的值(切会话与解析可能交错)。
    const isOwn = sessionId === useSessionStore.getState().activeSessionId;
    if (isOwn) {
      await useUltraPlanStore.getState().refresh(sessionId);
    } else {
      const face = await fetchDemoFace(sessionId);
      if (!alive.current) return;
      setForeign(face);
    }
    if (alive.current) setResolvedFor(isOwn ? 'own' : 'foreign');
  }, [sessionId]);

  // 挂载时解析一次;页签在「本会话 / 别的会话」之间切换(用户切了会话)时换来源再解析。
  useEffect(() => {
    void resolve();
  }, [resolve, own]);

  const face: DemoFace | null = own
    ? storeSessionId === sessionId
      ? { state: storeState, demo: storeDemo, demoError: storeDemoError, faceError: storeFaceError }
      : null
    : foreign;
  const loading = resolvedFor !== (own ? 'own' : 'foreign');
  const view = resolveDemoView({
    upId,
    loading,
    face,
    locationHostname: location.hostname,
    locationOrigin: location.origin,
  });
  const flow = face?.state && face.state.id === upId ? face.state : null;
  const iteration = flow?.demoIteration ?? 0;
  const demoBlock = useChatStore((st) => (own ? findDemoBlock(st.messages, upId, iteration) : null));
  const probe = readProbe(demoBlock?.payload.probe);

  // 动作只对「当前会话的当前流程」开放:别的会话的页签即使取回了它自己的状态也不给点。
  const blocked = demoBlockReason({
    upId,
    ownerSessionId: sessionId,
    activeSessionId,
    flow: own ? activeFlow : null,
    activeRunId,
    pending,
    unsupportedReason,
  });

  // tabbar 文案跟流程标题走(页签由实时事件打开时流程状态可能还没回填,只写了 Demo)。
  const setTabTitle = useWorkbenchStore((st) => st.setTabTitle);
  const flowTitle = flow?.title ?? '';
  useEffect(() => {
    if (tabId && flowTitle.trim() !== '') setTabTitle(tabId, demoTabTitle(flowTitle));
  }, [tabId, flowTitle, setTabTitle]);

  // 流程往前走了 / 换了一版:挂着的回退确认作废。
  const stage = flow?.stage ?? null;
  useEffect(() => {
    setConfirmRollback(false);
    setRollbackError(null);
  }, [iteration, stage]);

  const frameRef = useRef<HTMLIFrameElement | null>(null);
  const showFrame = view.kind === 'ready' && !stopped;
  const frameKey = `${iteration}:${nonce}`;
  // iframe 加载出错(浏览器会派发 error 的场合):重新解析坐标——端口变了 src 随之更新,
  // 地址没变就停在原处,不自动循环重试。React 不给 iframe 挂 onError,这里直接监听 DOM 事件。
  useEffect(() => {
    const el = frameRef.current;
    if (!el || !showFrame) return;
    const onError = () => void resolve();
    el.addEventListener('error', onError);
    return () => el.removeEventListener('error', onError);
  }, [showFrame, frameKey, resolve]);

  const reload = () => {
    setStopped(false);
    setNonce((n) => n + 1);
    void resolve();
  };

  const rollback = async () => {
    if (blocked !== null || rollingBack) return;
    setRollingBack(true);
    setRollbackError(null);
    const result = await useUltraPlanStore.getState().rollbackDemo();
    if (!alive.current) return;
    setRollingBack(false);
    if (result.ok) {
      setConfirmRollback(false);
      return;
    }
    setRollbackError(actionErrorText(result.code, result.message));
  };

  const url = view.kind === 'ready' ? view.url : null;
  const heading = flow?.title || title || 'Demo';
  const note = flow?.demoNote ?? null;
  const tool =
    'flex h-[26px] shrink-0 items-center gap-1 rounded-md border border-edge px-2 text-[11.5px] text-fg-2 hover:bg-shell-hover disabled:cursor-not-allowed disabled:opacity-50';

  return (
    <div data-testid="demo-tab" data-own={own ? '1' : '0'} className="flex h-full min-h-0 flex-col bg-shell-bg">
      {/* 头:名称 + 第 N 版 + 验证徽标 | 重新加载 / 停止 / 系统浏览器 */}
      <div className="flex shrink-0 flex-wrap items-center gap-x-3 gap-y-2 border-b border-edge px-5 py-3">
        <div className="flex min-w-0 flex-1 items-center gap-2">
          <span className="flex h-6 w-6 shrink-0 items-center justify-center rounded-md bg-acc-bg text-acc">
            <Gamepad2 size={13} />
          </span>
          <span className="truncate font-serif text-[20px] font-bold text-fg" title={heading}>
            {heading}
          </span>
          {iteration > 0 && (
            <span data-testid="demo-tab-iteration" className="shrink-0 font-code text-[11px] text-fg-3">
              第 {iteration} 版
            </span>
          )}
          {flow && iteration > 0 && (
            <VerifiedBadge verified={flow.demoVerified} note={note} testId="demo-tab-verified" />
          )}
        </div>
        <div className="flex shrink-0 items-center gap-1.5">
          <button
            type="button"
            data-testid="demo-tab-reload"
            title="重新获取 Demo 地址并从头加载"
            onClick={reload}
            className={tool}
          >
            <RotateCw size={12} />
            重新加载
          </button>
          <button
            type="button"
            data-testid="demo-tab-stop"
            disabled={!showFrame}
            title={showFrame ? '卸载 Demo(跑飞 / 卡死时用)' : '没有在运行的 Demo'}
            onClick={() => setStopped(true)}
            className={tool}
          >
            <Square size={11} />
            停止
          </button>
          {url !== null ? (
            <a
              data-testid="demo-tab-external"
              href={url}
              target="_blank"
              rel="noopener noreferrer"
              title="在系统浏览器里打开同一个 Demo"
              className={tool}
            >
              <ExternalLink size={12} />
              在系统浏览器打开
            </a>
          ) : (
            <button type="button" data-testid="demo-tab-external" disabled title="没有可打开的 Demo 地址" className={tool}>
              <ExternalLink size={12} />
              在系统浏览器打开
            </button>
          )}
        </div>
      </div>

      {/* 说明:探测边界 + 切页签重置;探测报错可展开 */}
      <div className="flex shrink-0 flex-wrap items-start gap-x-3 gap-y-1 border-b border-edge bg-shell-sunk px-5 py-1.5 text-[11px] leading-[16px] text-fg-3">
        <span data-testid="demo-tab-manual-note">{DEMO_MANUAL_NOTE}</span>
        <span data-testid="demo-tab-reset-note">{DEMO_RESET_NOTE}</span>
        {note && (
          <span data-testid="demo-tab-demo-note" className="min-w-0 break-words text-fg-2">
            {note}
          </span>
        )}
        {probe && <ProbeErrors errors={probe.errors} testId="demo-tab-probe-errors" />}
      </div>

      {/* 动作:通过 / 提出修改 / 回到上一版 */}
      <div className="flex shrink-0 flex-col gap-1.5 border-b border-edge px-5 py-2">
        <div className="flex flex-wrap items-start gap-x-3 gap-y-1.5">
          <div className="min-w-0 flex-1">
            <DemoDecisionControls
              key={`${upId}:${iteration}`}
              upId={upId}
              iteration={iteration}
              verified={flow?.demoVerified === true}
              blocked={blocked}
              failed={activeFlow?.phase === 'failed'}
              testIdPrefix="demo-tab"
            />
          </div>
          {iteration > 1 && (
            <button
              type="button"
              data-testid="demo-tab-rollback"
              aria-expanded={confirmRollback}
              disabled={blocked !== null || rollingBack}
              title={blocked ?? `回到第 ${iteration - 1} 版之前保存的 Demo`}
              onClick={() => setConfirmRollback((v) => !v)}
              className={tool}
            >
              <RotateCcw size={12} />
              回到上一版
            </button>
          )}
        </div>
        {confirmRollback && iteration > 1 && (
          <div
            role="group"
            aria-label="确认回到上一版"
            data-testid="demo-tab-rollback-confirm"
            className="flex flex-wrap items-center gap-1.5 rounded-md border border-edge bg-shell-sunk px-2.5 py-1.5 text-[11px] leading-[16px] text-fg-3"
          >
            <span className="min-w-0 flex-[1_1_220px]">
              用上一版保存的 Demo 覆盖当前文件,版本号记为第 {iteration + 1} 版,且不会重新自动验证。
            </span>
            <button
              type="button"
              data-testid="demo-tab-rollback-cancel"
              disabled={rollingBack}
              onClick={() => setConfirmRollback(false)}
              className="flex h-[24px] items-center rounded-md px-2 text-[11px] text-fg-3 hover:bg-shell-hover hover:text-fg-2 disabled:cursor-not-allowed disabled:opacity-50"
            >
              取消
            </button>
            <button
              type="button"
              data-testid="demo-tab-rollback-ok"
              disabled={blocked !== null || rollingBack}
              title={blocked ?? undefined}
              onClick={() => void rollback()}
              className="flex h-[24px] items-center gap-1 rounded-md border border-warn/30 bg-warn-bg px-2 text-[11px] text-warn hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-50"
            >
              {rollingBack && <Loader2 size={11} className="animate-spin" />}
              确认回退
            </button>
          </div>
        )}
        {rollbackError !== null && (
          <div
            role="alert"
            data-testid="demo-tab-rollback-error"
            className="rounded-md border border-warn/30 bg-warn-bg px-2 py-1.5 text-[11px] leading-[16px] text-warn"
          >
            {rollbackError}
          </div>
        )}
      </div>

      {/* 正文:iframe / 已停止 / 加载中 / 错误 */}
      <div className="relative min-h-0 flex-1" data-testid="demo-tab-body">
        {view.kind === 'loading' && (
          <div data-testid="demo-tab-loading" className="flex items-center gap-2 px-5 py-3 text-[12px] text-fg-4">
            <Loader2 size={12} className="animate-spin" />
            正在获取 Demo 地址…
          </div>
        )}
        {view.kind === 'error' && (
          <div data-testid="demo-tab-error" data-code={view.error.code} className="flex justify-center px-8 pt-12">
            <div className="flex max-w-[520px] flex-col gap-2 rounded-lg border border-warn/30 bg-warn-bg px-4 py-3 text-warn">
              <span className="flex items-center gap-1.5 text-[13px] font-medium">
                <TriangleAlert size={13} />
                Demo 无法加载
              </span>
              <span className="break-words text-[12px] leading-[18px]">{view.error.message}</span>
              <span className="font-code text-[10.5px] opacity-80">{view.error.code}</span>
            </div>
          </div>
        )}
        {view.kind === 'ready' && stopped && (
          <div data-testid="demo-tab-stopped" className="flex justify-center px-8 pt-12 text-[12.5px] text-fg-3">
            已停止,点重新加载恢复
          </div>
        )}
        {showFrame && url !== null && (
          <iframe
            key={frameKey}
            ref={frameRef}
            data-testid="demo-tab-frame"
            title={`UltraPlan Demo · ${heading} · 第 ${iteration} 版`}
            src={url}
            sandbox={DEMO_SANDBOX}
            allow=""
            referrerPolicy="no-referrer"
            className="absolute inset-0 h-full w-full border-0 bg-white"
          />
        )}
      </div>
    </div>
  );
}
