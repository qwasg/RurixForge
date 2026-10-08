import { useEffect, useState } from 'react';
import { ArrowUpRight, FileCode2, RotateCw } from 'lucide-react';
import { getAgentRecommendations, type AgentRecommendationsFace } from '@/lib/forgeApi';
import { useComposerPrefillStore } from '@/lib/composerStore';
import { useSessionStore } from '@/lib/sessionStore';
import { useWorkspaceStore } from '@/lib/workspaceStore';
import { useSystemStore } from '@/lib/systemStore';
import { useOverlayStore } from '@/lib/overlayStore';
import { runCommand } from '@/lib/commands';
import { composerModeMeta } from '../chat/composerModes';

const ART: Record<string, string> = { level: 'level.png', defense: 'defense.png', plan: 'plan.png', debug: 'debug.png' };

/** Display the backend's actual suggestions and dispatch exactly their draft and mode. */
export default function HomeRecommendations() {
  const prefill = useComposerPrefillStore((state) => state.prefill);
  const sessionId = useSessionStore((state) => state.activeSessionId);
  const session = useSessionStore((state) => state.sessions.find((item) => item.id === state.activeSessionId));
  const draftEngine = useSessionStore((state) => state.draftAgentEngine);
  const workspaceId = useWorkspaceStore((state) => state.activeWorkspaceId);
  const online = useSystemStore((state) => state.online);
  const settingsOpen = useOverlayStore((state) => state.settings);
  const engine = session?.agentEngine ?? draftEngine;
  const kind = session?.agentKind ?? 'coding';
  const [revision, refresh] = useState(0);
  const requestKey = JSON.stringify([sessionId, workspaceId, engine, kind, online, settingsOpen, revision]);
  const [result, setResult] = useState<{ key: string; face: AgentRecommendationsFace | null; error: boolean } | null>(null);
  const current = result?.key === requestKey ? result : null;

  useEffect(() => {
    let cancelled = false;
    void getAgentRecommendations({ sessionId, workspaceId, agentEngine: engine }).then((face) => {
      if (!Array.isArray(face.recommendations)) throw new Error('推荐响应不完整');
      if (!cancelled) setResult({ key: requestKey, face, error: false });
    }).catch(() => {
      if (!cancelled) setResult({ key: requestKey, face: null, error: true });
    });
    return () => { cancelled = true; };
  }, [requestKey, sessionId, workspaceId, engine]);

  return (
    <section data-testid="home-quick-starts" data-mode={current?.face?.context.gameMode} aria-label="Agent 推荐任务" className="mt-5">
      <div className="mb-2.5 flex items-center justify-between px-0.5">
        <span className="text-[11px] font-medium tracking-wide text-fg-3">从这里开始</span>
        <button type="button" aria-label="刷新推荐" title="刷新当前项目的推荐" onClick={() => refresh((value) => value + 1)} className="flex h-6 w-6 items-center justify-center rounded-full text-fg-3 transition-colors hover:bg-shell-hover hover:text-fg">
          <RotateCw size={11} />
        </button>
      </div>
      {current?.error ? (
        <div role="status" className="flex min-h-[166px] items-center justify-center gap-2 rounded-2xl border border-dashed border-edge text-[12px] text-fg-3">
          暂时无法获取推荐
          <button type="button" onClick={() => refresh((value) => value + 1)} className="rounded-md px-2 py-1 text-fg-2 hover:bg-shell-hover">重试</button>
        </div>
      ) : (
        <div className="home-recommendation-grid" aria-busy={!current?.face}>
          {current?.face ? current.face.recommendations.map((item) => {
            const mode = composerModeMeta(item.mode);
            const image = ART[item.image] ?? ART.level;
            return (
              <button key={item.id} type="button" data-testid={`home-quick-${item.id}`} onClick={() => prefill(item.draft, item.mode)} className="home-recommendation group" title={item.draft}>
                <div className="home-recommendation-art">
                  <img src={`${import.meta.env.BASE_URL}recommendations/${image}`} alt="" draggable={false} decoding="async" width="1536" height="1024" />
                  <span className="home-recommendation-mode"><mode.icon size={10} />{mode.label}</span>
                </div>
                <div className="min-w-0 px-3.5 pb-3.5 pt-3">
                  <div className="flex items-center justify-between gap-1.5">
                    <span className="text-[12.5px] font-medium leading-snug text-fg">{item.label}</span>
                    <ArrowUpRight size={13} className="home-recommendation-arrow shrink-0 text-fg-3" />
                  </div>
                  <p className="mt-1.5 text-[11px] leading-relaxed text-fg-3">{item.desc}</p>
                </div>
              </button>
            );
          }) : [0, 1, 2].map((item) => <div key={item} aria-hidden className="home-recommendation-skeleton" />)}
        </div>
      )}
      <div className="mt-2 flex justify-end">
        <button type="button" data-testid="home-open-editor" onClick={() => runCommand('tab.editor')} className="flex h-[26px] items-center gap-1.5 rounded-full px-2.5 text-[11px] text-fg-3 transition-colors hover:bg-shell-hover hover:text-fg-2">
          <FileCode2 size={11} />打开编辑器
        </button>
      </div>
    </section>
  );
}
