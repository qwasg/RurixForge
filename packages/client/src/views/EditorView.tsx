import { useEffect } from 'react';
import {
  FlaskConical,
  FolderOpen,
  Move,
  PanelBottom,
  Pause,
  Play,
  Redo2,
  Rotate3d,
  Save,
  Scaling,
  Square,
  StepForward,
  Undo2,
} from 'lucide-react';
import { cn } from '@/lib/cn';
import { ViewportCanvas } from '@/components/editor/ViewportCanvas';
import AssetsPanel from '@/components/editor/AssetsPanel';
import DesignBoardView from '@/components/editor/DesignBoardView';
import NodeGraphView from '@/components/editor/NodeGraphView';
import StudioBoardView from '@/components/studio/StudioBoardView';
import { useGraphStore } from '@/lib/graphStore';
import { useEditorStore, type CenterTab, type GizmoMode, type PlayState } from '@/lib/editorStore';
import { useWorkspaceStore } from '@/lib/workspaceStore';

/**
 * 编辑器视图:07 §1 七区骨架。
 * C Viewport(+G NodeGraph 同位页签)/ B Assets 底栏。
 * F7 wave.3(D-F7-B):F Chat dock 移除——agent 对话统一由壳内对话列承接;
 * 本视图作为壳内 workbench 的「编辑器」tab 内容嵌入(游戏原生内部逻辑零改动)。
 * UI 融合波(2026-08-20 用户拍板大面积优化):旧 ink/muted/line/panel 静态 token
 * 全域迁壳语义 token(fg/edge/shell/acc/dot),亮暗主题打通。
 * 响应式波(2026-08-24 用户拍板):A Hierarchy / D Inspector 迁壳右栏(见 RightPane)。
 * 底栏波(2026-08-24 用户拍板):E Workbench(Console/Problems/Metrics)整块退役,
 * 其位让给 B Assets 横向底栏;场景级操作(Undo/Redo/Save/Load/Playtest)并入视口工具条。
 * Assets 不再与视口争宽度,故宽度分档收放一并退役,仅留工具条手动开合(持久化)。
 * 画板波(2026-08-24):新增第三同位页签「画板」(DesignBoardView)——角色/地图节点
 * 自由拉线,交互描述写在线上,画好流程图一键交给 AI 制作素材与代码。
 */

const iconBtn =
  'flex h-6 w-6 items-center justify-center rounded-md text-fg-3 transition-colors hover:bg-shell-hover hover:text-fg-2 disabled:opacity-40 disabled:hover:bg-transparent disabled:hover:text-fg-3';

// ---------- B Assets(F2 wave.3 全量) ----------
// AssetsPanel 已迁至 components/editor/AssetsPanel.tsx:网格/列表、类型过滤、搜索、
// 拖拽实例化、右键六菜单、buildState 角标。

// ---------- C Viewport / G NodeGraph ----------

/** 场景级操作:Undo/Redo/Save/Load + playtest(原挂 Workbench tab 条右侧,底栏波并入视口工具条)。 */
function SceneActions() {
  const undo = useEditorStore((s) => s.undo);
  const redo = useEditorStore((s) => s.redo);
  const saveScene = useEditorStore((s) => s.saveScene);
  const loadScene = useEditorStore((s) => s.loadScene);
  const runPlaytest = useEditorStore((s) => s.runPlaytest);

  return (
    <>
      <button type="button" title="Undo" className={iconBtn} onClick={() => void undo()}>
        <Undo2 size={13} strokeWidth={1.8} />
      </button>
      <button type="button" title="Redo" className={iconBtn} onClick={() => void redo()}>
        <Redo2 size={13} strokeWidth={1.8} />
      </button>
      <span className="mx-0.5 h-4 w-px bg-edge-strong" />
      <button type="button" title="Save Scene" className={iconBtn} onClick={() => void saveScene()}>
        <Save size={13} strokeWidth={1.8} />
      </button>
      <button type="button" title="Load Scene" className={iconBtn} onClick={() => void loadScene()}>
        <FolderOpen size={13} strokeWidth={1.8} />
      </button>
      <span className="mx-0.5 h-4 w-px bg-edge-strong" />
      <button
        type="button"
        title="Run maze playtest(结果走 toast)"
        className={iconBtn}
        onClick={() => void runPlaytest('tests/maze/matrix.json')}
      >
        <FlaskConical size={13} strokeWidth={1.8} />
      </button>
    </>
  );
}

const GIZMOS: Array<{ mode: GizmoMode; title: string; icon: typeof Move }> = [
  { mode: 'translate', title: 'Move (W)', icon: Move },
  { mode: 'rotate', title: 'Rotate (E)', icon: Rotate3d },
  { mode: 'scale', title: 'Scale (R)', icon: Scaling },
];

function PlayControls() {
  const playState = useEditorStore((s) => s.playState);
  const playEnter = useEditorStore((s) => s.playEnter);
  const playPause = useEditorStore((s) => s.playPause);
  const playResume = useEditorStore((s) => s.playResume);
  const playStep = useEditorStore((s) => s.playStep);
  const playExit = useEditorStore((s) => s.playExit);

  return (
    <span className="flex items-center gap-0.5">
      <button
        type="button"
        title="Play"
        className={cn(iconBtn, playState !== 'edit' && playState !== 'play_paused' && 'opacity-40')}
        disabled={playState === 'play_running'}
        onClick={() => void (playState === 'play_paused' ? playResume() : playEnter())}
      >
        <Play size={13} strokeWidth={1.8} />
      </button>
      <button
        type="button"
        title="Pause"
        className={iconBtn}
        disabled={playState !== 'play_running'}
        onClick={() => void playPause()}
      >
        <Pause size={13} strokeWidth={1.8} />
      </button>
      <button
        type="button"
        title="Step"
        className={iconBtn}
        disabled={playState !== 'play_paused'}
        onClick={() => void playStep()}
      >
        <StepForward size={13} strokeWidth={1.8} />
      </button>
      <button
        type="button"
        title="Stop (exit PIE)"
        className={iconBtn}
        disabled={playState === 'edit'}
        onClick={() => void playExit()}
      >
        <Square size={12} strokeWidth={1.8} />
      </button>
    </span>
  );
}

const PIE_DOT: Record<PlayState, string> = {
  edit: 'bg-dot-idle',
  play_running: 'bg-dot-done',
  play_paused: 'bg-info',
};

/** 中央区同位页签(画板波:+「画板」自由流程图;素材创作波:+「素材创作」AI 创作板) */
const CENTER_TABS: Array<{ id: CenterTab; label: string }> = [
  { id: 'viewport', label: 'Viewport' },
  { id: 'nodegraph', label: 'NodeGraph' },
  { id: 'design', label: '画板' },
  { id: 'studio', label: '素材创作' },
];

function ViewportPanel() {
  const centerTab = useEditorStore((s) => s.centerTab);
  const setCenterTab = useEditorStore((s) => s.setCenterTab);
  const gizmo = useEditorStore((s) => s.gizmo);
  const setGizmo = useEditorStore((s) => s.setGizmo);
  const playState = useEditorStore((s) => s.playState);
  const sceneName = useEditorStore((s) => s.sceneName);
  const panes = useEditorStore((s) => s.editorPanes);
  const togglePane = useEditorStore((s) => s.toggleEditorPane);

  return (
    <section className="flex min-h-0 flex-1 flex-col" aria-label="Viewport">
      {/* 顶部工具条:Assets 显隐 + Viewport/NodeGraph 页签 + gizmo + PIE 控制 + 场景操作
          (F7:Chat 开关移除,对话入壳对话列;底栏波:场景操作从 Workbench tab 条并入此处) */}
      <div className="flex shrink-0 flex-wrap items-center gap-2 border-b border-edge px-2 py-1">
        <button
          type="button"
          title={panes.assets ? '隐藏 Assets 底栏' : '显示 Assets 底栏'}
          aria-label={panes.assets ? '隐藏 Assets 底栏' : '显示 Assets 底栏'}
          data-testid="editor-toggle-assets"
          className={cn(iconBtn, panes.assets && 'bg-shell-active text-fg')}
          onClick={() => togglePane('assets')}
        >
          <PanelBottom size={13} strokeWidth={1.8} />
        </button>
        <span className="h-4 w-px bg-edge-strong" />
        <span className="flex items-center gap-0.5 rounded-md bg-shell-sunk p-0.5">
          {CENTER_TABS.map((t) => (
            <button
              key={t.id}
              type="button"
              onClick={() => setCenterTab(t.id)}
              className={cn(
                'rounded px-2 py-0.5 text-xs capitalize transition-colors',
                centerTab === t.id ? 'bg-shell-panel text-fg shadow-sm' : 'text-fg-3 hover:text-fg-2',
              )}
            >
              {t.label}
            </button>
          ))}
        </span>
        <span className="h-4 w-px bg-edge-strong" />
        <span className="flex items-center gap-0.5" aria-label="Gizmo">
          {GIZMOS.map(({ mode, title, icon: Icon }) => (
            <button
              key={mode}
              type="button"
              title={title}
              className={cn(iconBtn, gizmo === mode && 'bg-shell-active text-fg')}
              onClick={() => setGizmo(mode)}
            >
              <Icon size={13} strokeWidth={1.8} />
            </button>
          ))}
        </span>
        <span className="h-4 w-px bg-edge-strong" />
        <PlayControls />
        <span className="flex-1" />
        <SceneActions />
      </div>

      {centerTab === 'viewport' ? (
        <div className="relative min-h-0 flex-1 bg-ink">
          {/* GPU 场景实渲染帧 + 点选/相机/gizmo(wave.2,RD-F1-001 回填) */}
          <ViewportCanvas />
          {/* PIE 状态条(play_state 实测) */}
          <div className="absolute inset-x-0 bottom-0 flex items-center gap-2 bg-black/40 px-2 py-1">
            <span className={cn('h-1.5 w-1.5 rounded-full', PIE_DOT[playState])} />
            <span className="font-mono text-2xs text-white/70">{playState}</span>
            <span className="flex-1" />
            <span className="truncate text-2xs text-white/40">{sceneName || 'Untitled'}</span>
          </div>
        </div>
      ) : centerTab === 'nodegraph' ? (
        /* G NodeGraph 同位页签(F4 wave.4):图查看/微调/保存 */
        <NodeGraphView />
      ) : centerTab === 'design' ? (
        /* 画板同位页签(画板波):角色/地图节点拉线,交互在线上,交给 AI 制作 */
        <DesignBoardView />
      ) : (
        /* 素材创作同位页签(素材创作波):大纲/原画/贴图/UI/3D/视频/音频 AI 创作板 */
        <StudioBoardView />
      )}
    </section>
  );
}

// ---------- 总装 ----------

export default function EditorView() {
  const loadEntities = useEditorStore((s) => s.loadEntities);
  const refreshSummary = useEditorStore((s) => s.refreshSummary);
  const refreshPlayState = useEditorStore((s) => s.refreshPlayState);
  const ensureDefaultScene = useEditorStore((s) => s.ensureDefaultScene);
  const centerTab = useEditorStore((s) => s.centerTab);
  const selectedId = useEditorStore((s) => s.selectedId);
  const panes = useEditorStore((s) => s.editorPanes);
  const loadGraphForSelected = useGraphStore((s) => s.loadForSelectedEntity);

  // 进视图即拉一次真实数据;空场景 → 默认加载迷宫(打开即见真实工程,非空壳)。
  // 工作区切换 = 视口所连 engine-host 换成新项目的实例,场景/实体/相机/选中全部按新项目重拉。
  const activeWorkspaceId = useWorkspaceStore((s) => s.activeWorkspaceId);
  useEffect(() => {
    useEditorStore.getState().selectEntity(null);
    void ensureDefaultScene().then(() => loadEntities());
    void refreshSummary();
    void refreshPlayState();
    void useEditorStore.getState().loadCamera();
  }, [ensureDefaultScene, loadEntities, refreshSummary, refreshPlayState, activeWorkspaceId]);

  // F9(D1):MCP 侧(agent 聊天)实体变更同步腿——周期 scene_summary;
  // entityCount 漂移时 refreshSummary 内真实重拉 entity_list,Hierarchy 随之刷新。
  useEffect(() => {
    const t = setInterval(() => void refreshSummary(), 1000);
    return () => clearInterval(t);
  }, [refreshSummary]);

  // F4 wave.4:切到 NodeGraph 页签,或页签可见时选中实体变化 → 按 Script.graphRef 载图(无 → 空态)
  useEffect(() => {
    if (centerTab === 'nodegraph') void loadGraphForSelected();
  }, [centerTab, selectedId, loadGraphForSelected]);

  return (
    // min-w 320:极窄下兜底横向滚动,防视口 0 宽压溃
    <div className="flex h-full min-h-0 min-w-[320px] flex-col overflow-x-auto bg-shell-bg">
      {/* C Viewport(G NodeGraph 同位) */}
      <ViewportPanel />

      {/* B Assets 底栏(占旧 Workbench 位;工具条 PanelBottom 钮开合,偏好持久化) */}
      {panes.assets && (
        <div
          data-testid="editor-pane-assets"
          className="flex h-[190px] shrink-0 flex-col border-t border-edge bg-shell-sunk"
        >
          <AssetsPanel />
        </div>
      )}
    </div>
  );
}
