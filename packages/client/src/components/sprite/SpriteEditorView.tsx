import { useEffect, useState } from 'react';
import { Save, Scissors, Undo2, Wand2, X } from 'lucide-react';
import { cn } from '@/lib/cn';
import { useSpriteStore } from '@/lib/spriteStore';
import SpriteCanvas from './SpriteCanvas';
import SpritePreview from './SpritePreview';
import SpriteRightPanel from './SpriteRightPanel';

/**
 * 精灵编辑器 tab(F-GAME-4 workbench 内建页):
 * 工具栏(Auto-Detect 本地 / 服务端切帧 / 撤销 / 保存 dirty 高亮 / 错误条)+
 * 画布区(SpriteCanvas)+ 预览播放器(SpritePreview)+ 右栏(SpriteRightPanel)。
 * 贴图 HTMLImageElement 在此解码一次,画布与预览共用;像素回填 store 供本地检测。
 * 入口:资产面板右键 texture「编辑精灵(新建 .rxsprite)」/ sprite「编辑精灵」/ 双击 .rxsprite。
 */

const toolBtn =
  'flex h-6 items-center gap-1 rounded-md border border-edge px-2 text-2xs text-fg-3 transition-colors hover:bg-shell-hover hover:text-fg-2 disabled:cursor-not-allowed disabled:opacity-40';

export default function SpriteEditorView() {
  const assetPath = useSpriteStore((s) => s.assetPath);
  const doc = useSpriteStore((s) => s.doc);
  const loading = useSpriteStore((s) => s.loading);
  const saving = useSpriteStore((s) => s.saving);
  const dirty = useSpriteStore((s) => s.dirty);
  const error = useSpriteStore((s) => s.error);
  const texDataUrl = useSpriteStore((s) => s.texDataUrl);
  const texPixelsReady = useSpriteStore((s) => s.texPixels !== null);
  const undoDepth = useSpriteStore((s) => s.undoStack.length);
  const animatorError = useSpriteStore((s) => s.animatorError);

  const [img, setImg] = useState<HTMLImageElement | null>(null);

  // 贴图解码:dataUrl → Image(单一所有权,画布/预览共用);像素回填 store 供本地 Auto-Detect。
  // jsdom 无解码面时 onload 不触发 → img 保持 null,画布仅显示 bbox(降级如实)。
  useEffect(() => {
    setImg(null);
    if (!texDataUrl) return;
    let alive = true;
    const image = new Image();
    image.onload = () => {
      if (!alive) return;
      setImg(image);
      try {
        const off = document.createElement('canvas');
        off.width = image.naturalWidth;
        off.height = image.naturalHeight;
        const octx = off.getContext('2d');
        if (octx) {
          octx.drawImage(image, 0, 0);
          const data = octx.getImageData(0, 0, off.width, off.height);
          useSpriteStore.getState().setTextureBitmap(off.width, off.height, data.data);
        }
      } catch {
        // 像素读取失败(环境限制):本地检测按钮如实禁用,不伪造。
      }
    };
    image.src = texDataUrl;
    return () => {
      alive = false;
      image.onload = null;
    };
  }, [texDataUrl]);

  const st = useSpriteStore.getState;
  const frameCount = doc ? Object.keys(doc.frames).length : 0;
  const clipCount = doc ? Object.keys(doc.clips).length : 0;

  return (
    <div className="flex h-full min-h-0 min-w-0 flex-col bg-shell-bg" data-testid="sprite-editor">
      {/* 工具栏 */}
      <div className="flex h-[34px] shrink-0 items-center gap-1.5 border-b border-edge px-2">
        <span className="min-w-0 flex-initial truncate text-xs text-fg-2" title={assetPath ?? ''}>
          {assetPath ?? '精灵编辑器'}
        </span>
        {doc && (
          <span className="shrink-0 text-2xs text-fg-4">
            {frameCount} 帧 · {clipCount} clip
          </span>
        )}
        <span className="flex-1" />
        <button
          type="button"
          data-testid="sprite-toolbar-autodetect"
          className={toolBtn}
          disabled={!doc || !texPixelsReady}
          title={
            texPixelsReady
              ? '前端本地连通域切帧(零往返;背景 = alpha<5 或品红族,与引擎色键同规则)'
              : '贴图像素未就绪(等待解码或环境不支持),可用「服务端切帧」'
          }
          onClick={() => st().autoDetect()}
        >
          <Wand2 size={11} />
          Auto-Detect
        </button>
        <button
          type="button"
          data-testid="sprite-toolbar-slice"
          className={toolBtn}
          disabled={!doc}
          title="服务端 sprite_autoslice(同规则;结果重建 frames)"
          onClick={() => void st().serverAutoslice()}
        >
          <Scissors size={11} />
          服务端切帧
        </button>
        <button
          type="button"
          data-testid="sprite-toolbar-undo"
          className={toolBtn}
          disabled={undoDepth === 0}
          title={`撤销(栈深 ${undoDepth})`}
          onClick={() => st().undo()}
        >
          <Undo2 size={11} />
          撤销
        </button>
        <button
          type="button"
          data-testid="sprite-toolbar-save"
          disabled={!doc || saving || animatorError !== null}
          title={animatorError !== null ? `animator JSON 无效,禁存:${animatorError}` : 'sprite_set 整文档写回'}
          onClick={() => void st().save()}
          className={cn(
            'flex h-6 items-center gap-1 rounded-md px-2.5 text-2xs transition-colors disabled:cursor-not-allowed disabled:opacity-40',
            dirty ? 'bg-acc text-white' : 'border border-edge text-fg-3 hover:bg-shell-hover',
          )}
        >
          <Save size={11} />
          {saving ? '保存中…' : dirty ? '保存 ●' : '保存'}
        </button>
      </div>

      {/* 错误条(打开/保存/切帧失败的 message 如实展示,不吞) */}
      {error && (
        <div
          className="flex shrink-0 items-center gap-2 border-b border-edge bg-shell-panel px-2 py-1"
          data-testid="sprite-error"
        >
          <span className="min-w-0 flex-1 truncate text-2xs text-warn" title={error}>
            {error}
          </span>
          <button
            type="button"
            title="关闭错误条"
            className="flex h-4 w-4 shrink-0 items-center justify-center rounded text-fg-3 hover:bg-shell-hover"
            onClick={() => useSpriteStore.setState({ error: null })}
          >
            <X size={10} />
          </button>
        </div>
      )}

      {/* 主体 */}
      {!doc ? (
        <div className="flex flex-1 items-center justify-center p-8" data-testid="sprite-empty">
          <div className="flex w-[440px] max-w-full flex-col gap-2 rounded-xl border border-edge bg-shell-panel p-6">
            <span className="text-[15px] font-medium text-fg">
              {loading ? '正在打开精灵…' : '未打开精灵'}
            </span>
            <span className="text-xs text-fg-3">
              在底部资产面板:右键贴图「编辑精灵(新建 .rxsprite)」,或右键/双击 .rxsprite 资产打开。
            </span>
          </div>
        </div>
      ) : (
        <div className="flex min-h-0 min-w-0 flex-1">
          <div className="flex min-w-0 flex-1 flex-col">
            <div className="min-h-0 flex-1">
              <SpriteCanvas img={img} />
            </div>
            <SpritePreview img={img} />
          </div>
          <SpriteRightPanel />
        </div>
      )}
    </div>
  );
}
