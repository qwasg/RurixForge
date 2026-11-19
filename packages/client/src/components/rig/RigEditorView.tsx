import { useEffect, useState } from 'react';
import { Bone, Brush, Grid3X3, Loader2, Play, Redo2, Save, SlidersHorizontal, Undo2, Wand2, X } from 'lucide-react';
import { cn } from '@/lib/cn';
import { useRigStore, type RigMode } from '@/lib/rigStore';
import RigCanvas from './RigCanvas';
import RigHierarchy from './RigHierarchy';
import RigParamsPanel from './RigParamsPanel';
import RigTimeline from './RigTimeline';
import RigWeightPanel from './RigWeightPanel';

/**
 * 绑骨编辑器 tab(F-GAME-5 W5 / D-050;workbench 单例页 'rig-editor',当前资产在 rigStore):
 * 工具栏(模式切换 网格/骨骼/权重/参数/动画 · 撤销/重做 · 自动网格 · 自动权重 · 保存)+
 * 左栏层级(部件/骨骼/形变器)+ 中央画布(WebGL2 纹理三角形 + 2D 手柄层)+ 右栏(按模式)+
 * 动画模式底部时间轴。图集 dataUrl 在此解码一次传给画布(照 SpriteEditorView)。
 * 入口:资产面板双击 / 右键「编辑绑骨」/ 资产检视器 / 实体检视器 Rig2D。
 */

/** 工具栏按钮:白色毛玻璃(index.css .glass-btn),与精灵编辑器工具栏一致 */
const toolBtn = 'glass-btn h-6 disabled:cursor-not-allowed';

const MODES: Array<{ key: RigMode; label: string; en: string; icon: typeof Bone; hint: string }> = [
  { key: 'mesh', label: '网格', en: 'Mesh', icon: Grid3X3, hint: '拖动静止网格顶点(画布像素)' },
  { key: 'bones', label: '骨骼', en: 'Bones', icon: Bone, hint: '拖骨头/骨尾;空白处拖拽新建子骨骼' },
  { key: 'weights', label: '权重', en: 'Weights', icon: Brush, hint: '热力图 + 权重笔刷(加/减/平滑)' },
  { key: 'params', label: '参数', en: 'Params', icon: SlidersHorizontal, hint: '参数滑杆实时形变;关键值处拖顶点记录 keyform' },
  { key: 'animate', label: '动画', en: 'Animate', icon: Play, hint: 'clip 轨道与关键帧;播放头驱动求值' },
];

export default function RigEditorView() {
  const assetPath = useRigStore((s) => s.assetPath);
  const doc = useRigStore((s) => s.doc);
  const loading = useRigStore((s) => s.loading);
  const saving = useRigStore((s) => s.saving);
  const busy = useRigStore((s) => s.busy);
  const dirty = useRigStore((s) => s.dirty);
  const error = useRigStore((s) => s.error);
  const notice = useRigStore((s) => s.notice);
  const mode = useRigStore((s) => s.mode);
  const texDataUrl = useRigStore((s) => s.texDataUrl);
  const undoDepth = useRigStore((s) => s.undoStack.length);
  const redoDepth = useRigStore((s) => s.redoStack.length);
  const playing = useRigStore((s) => s.playing);
  const st = useRigStore.getState;

  const [img, setImg] = useState<HTMLImageElement | null>(null);

  // 图集解码:dataUrl → Image(画布共用);jsdom 无解码面时 onload 不触发 → img 保持 null(画布降级为线框)。
  useEffect(() => {
    setImg(null);
    if (!texDataUrl) return;
    let alive = true;
    const image = new Image();
    image.onload = () => {
      if (!alive) return;
      setImg(image);
      useRigStore.getState().setTextureSize(image.naturalWidth, image.naturalHeight);
    };
    image.src = texDataUrl;
    return () => {
      alive = false;
      image.onload = null;
    };
  }, [texDataUrl]);

  // 播放循环:rAF 推进播放头(tick 内部按 clip loop 取模/停在末尾,并触发节流求值)。
  useEffect(() => {
    if (!playing || typeof requestAnimationFrame !== 'function') return;
    let raf = 0;
    let last = performance.now();
    const step = (now: number) => {
      const dt = Math.min(0.1, Math.max(0, (now - last) / 1000));
      last = now;
      useRigStore.getState().tick(dt);
      raf = requestAnimationFrame(step);
    };
    raf = requestAnimationFrame(step);
    return () => cancelAnimationFrame(raf);
  }, [playing]);

  // 快捷键(焦点在编辑器内):Ctrl+Z 撤销 / Ctrl+Shift+Z|Ctrl+Y 重做 / Ctrl+S 保存;文本控件避让。
  const onKeyDown = (e: React.KeyboardEvent<HTMLDivElement>) => {
    const t = e.target as HTMLElement;
    if (t.closest('input, textarea, select, [contenteditable="true"]')) return;
    if (!(e.ctrlKey || e.metaKey)) return;
    const k = e.key.toLowerCase();
    if (k === 'z' && !e.shiftKey) {
      e.preventDefault();
      st().undo();
    } else if ((k === 'z' && e.shiftKey) || k === 'y') {
      e.preventDefault();
      st().redo();
    } else if (k === 's') {
      e.preventDefault();
      void st().save();
    }
  };

  const busyText = busy === 'mesh' ? '自动网格中…' : busy === 'weights' ? '自动权重中…' : null;

  return (
    <div
      className="flex h-full min-h-0 min-w-0 flex-col bg-shell-bg"
      data-testid="rig-editor"
      data-mode={mode}
      onKeyDown={onKeyDown}
    >
      {/* 工具栏 */}
      <div className="flex h-[34px] shrink-0 items-center gap-1.5 border-b border-edge px-2">
        <span className="min-w-0 flex-initial truncate text-xs text-fg-2" title={assetPath ?? ''}>
          {assetPath ?? '绑骨编辑器 · Rig Editor'}
        </span>
        {doc && (
          <span className="shrink-0 text-2xs text-fg-4" data-testid="rig-toolbar-stats">
            {doc.parts.length} 部件 · {doc.bones.length} 骨骼 · {doc.parameters.length} 参数 ·{' '}
            {Object.keys(doc.clips).length} clip
          </span>
        )}
        <span className="flex-1" />
        {/* 模式切换(分段按钮) */}
        <div className="flex shrink-0 items-center gap-0.5" role="tablist" aria-label="编辑模式 / Mode">
          {MODES.map((m) => {
            const Icon = m.icon;
            const active = mode === m.key;
            return (
              <button
                key={m.key}
                type="button"
                role="tab"
                aria-selected={active}
                data-testid={`rig-mode-${m.key}`}
                disabled={!doc}
                title={`${m.label} / ${m.en}:${m.hint}`}
                onClick={() => st().setMode(m.key)}
                className={cn(toolBtn, active && 'ring-1 ring-acc text-fg')}
              >
                <Icon size={11} />
                {m.label}
              </button>
            );
          })}
        </div>
        <span className="h-4 w-px shrink-0 bg-edge-strong" />
        <button
          type="button"
          data-testid="rig-toolbar-automesh"
          className={toolBtn}
          disabled={!doc || busy !== null}
          title="rig_mesh_auto:按图集 alpha 自动生成网格(有选中部件时只处理该部件;会清空其权重)。先保存本地改动。"
          onClick={() => void st().autoMesh()}
        >
          {busy === 'mesh' ? <Loader2 size={11} className="animate-spin" /> : <Wand2 size={11} />}
          自动网格
        </button>
        <button
          type="button"
          data-testid="rig-toolbar-autoweights"
          className={toolBtn}
          disabled={!doc || busy !== null || (doc?.bones.length ?? 0) === 0}
          title="rig_weights_auto:按骨骼距离自动蒙皮权重(有选中部件时只处理该部件)。先保存本地改动。"
          onClick={() => void st().autoWeights()}
        >
          {busy === 'weights' ? <Loader2 size={11} className="animate-spin" /> : <Brush size={11} />}
          自动权重
        </button>
        <span className="h-4 w-px shrink-0 bg-edge-strong" />
        <button
          type="button"
          data-testid="rig-toolbar-undo"
          className={toolBtn}
          disabled={undoDepth === 0}
          title={`撤销 Undo(Ctrl+Z;栈深 ${undoDepth})`}
          onClick={() => st().undo()}
        >
          <Undo2 size={11} />
          撤销
        </button>
        <button
          type="button"
          data-testid="rig-toolbar-redo"
          className={toolBtn}
          disabled={redoDepth === 0}
          title={`重做 Redo(Ctrl+Shift+Z;栈深 ${redoDepth})`}
          onClick={() => st().redo()}
        >
          <Redo2 size={11} />
          重做
        </button>
        <button
          type="button"
          data-testid="rig-toolbar-save"
          disabled={!doc || saving}
          title="保存 Save(Ctrl+S):diff → rig_patch;差异不可表达或过大时 rig_set 整文档"
          onClick={() => void st().save()}
          className={
            dirty
              ? 'flex h-6 items-center gap-1 rounded-md bg-acc px-2.5 text-2xs text-white transition-opacity hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-40'
              : toolBtn
          }
        >
          <Save size={11} />
          {saving ? '保存中…' : dirty ? '保存 ●' : '保存'}
        </button>
      </div>

      {/* 错误条 / 提示条(工具错误 message 如实展示,不吞) */}
      {error && (
        <div
          className="flex shrink-0 items-center gap-2 border-b border-edge bg-shell-panel px-2 py-1"
          data-testid="rig-error"
        >
          <span className="min-w-0 flex-1 truncate text-2xs text-warn" title={error}>
            {error}
          </span>
          <button
            type="button"
            title="关闭"
            className="flex h-4 w-4 shrink-0 items-center justify-center rounded text-fg-3 hover:bg-shell-hover"
            onClick={() => useRigStore.setState({ error: null })}
          >
            <X size={10} />
          </button>
        </div>
      )}
      {(notice || busyText) && !error && (
        <div
          className="flex shrink-0 items-center gap-2 border-b border-edge bg-shell-panel px-2 py-1"
          data-testid="rig-notice"
        >
          <span className="min-w-0 flex-1 truncate text-2xs text-fg-3" title={notice ?? ''}>
            {busyText ?? notice}
          </span>
          {!busyText && (
            <button
              type="button"
              title="关闭"
              className="flex h-4 w-4 shrink-0 items-center justify-center rounded text-fg-3 hover:bg-shell-hover"
              onClick={() => useRigStore.setState({ notice: null })}
            >
              <X size={10} />
            </button>
          )}
        </div>
      )}

      {/* 主体 */}
      {!doc ? (
        <div className="flex flex-1 items-center justify-center p-8" data-testid="rig-empty">
          <div className="flex w-[440px] max-w-full flex-col gap-2 rounded-xl border border-edge bg-shell-panel p-6">
            <span className="text-[15px] font-medium text-fg">
              {loading ? '正在打开绑骨…' : '未打开绑骨 · No rig open'}
            </span>
            <span className="text-xs text-fg-3">
              在底部资产面板双击 .rxrig 资产,或右键「编辑绑骨」打开;也可在资产检视器 / 实体 Rig2D 组件里点「打开绑骨编辑器」。
            </span>
          </div>
        </div>
      ) : (
        <div className="flex min-h-0 min-w-0 flex-1">
          <RigHierarchy />
          <div className="flex min-w-0 flex-1 flex-col">
            <div className="min-h-0 flex-1">
              <RigCanvas img={img} />
            </div>
            {mode === 'animate' && <RigTimeline />}
          </div>
          <div
            className="flex w-[260px] shrink-0 flex-col overflow-y-auto border-l border-edge bg-shell-sidebar"
            data-testid="rig-right-panel"
          >
            {mode === 'weights' ? <RigWeightPanel /> : <RigParamsPanel />}
          </div>
        </div>
      )}
    </div>
  );
}
