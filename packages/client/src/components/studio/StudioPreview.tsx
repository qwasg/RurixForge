import { useState } from 'react';
import { AudioLines, Boxes, Clapperboard, FileText, Image as ImageIcon } from 'lucide-react';
import { cn } from '@/lib/cn';
import type { AssetItem } from '@/lib/assetStore';
import type { StudioKind, StudioVersion } from '@/lib/studioStore';
import Thumb from '../editor/assetThumb';

/**
 * 创作产物预览(主画布卡片与详情画布共用):
 * - image:会话 dataUrl 直显;已入库走 Thumb(资产缩略链);仅 fileRef(重开会话)如实占位;
 * - video/audio:dataUrl 直接 <video>/<audio> 控件;仅 fileRef 如实占位;
 * - model:glb 无浏览器内联渲染面;供应商回了缩略图(如 meshy thumbnail_url)就显示它——
 *   那是产物本身的渲染而非臆造;无缩略图则图标 + 文件名如实占位(入库后可在 Assets 面板查看);
 * - text:正文摘要;空态给类型图标。不伪造任何缩略。
 */

export const KIND_ICON: Record<StudioKind, typeof FileText> = {
  text: FileText,
  image: ImageIcon,
  model: Boxes,
  video: Clapperboard,
  audio: AudioLines,
};

function fileBase(ref: string): string {
  return ref.split('/').pop() ?? ref;
}

const VIEW_LABELS: Record<string, string> = {
  front: '前',
  right: '右',
  back: '后',
  left: '左',
  alpha: '透明底',
};

/**
 * 3D 产物的多视角预览。图生 3D 供应商渲四向,文生 3D 只有正面——
 * 视角按钮只在真有多张时出现,不为「看起来像有四向」而摆灰按钮。
 */
function MeshViews({
  views,
  className,
  compact,
}: {
  views: { label: string; dataUrl: string }[];
  className?: string;
  compact: boolean;
}) {
  const [idx, setIdx] = useState(0);
  const cur = views[Math.min(idx, views.length - 1)];
  if (compact) {
    return (
      <div className={cn('overflow-hidden', className)}>
        <img src={cur.dataUrl} alt="3D 产物预览(供应商渲染)" className="h-full w-full object-contain" />
      </div>
    );
  }
  return (
    <div className={cn('relative overflow-hidden', className)}>
      <img
        src={cur.dataUrl}
        alt={`3D 产物 ${cur.label} 视角(供应商渲染)`}
        className="h-full w-full object-contain"
      />
      {views.length > 1 && (
        <div
          data-testid="studio-mesh-views"
          className="absolute bottom-1 left-1/2 flex -translate-x-1/2 gap-0.5 rounded-md border border-edge-strong bg-shell-float/90 p-0.5"
        >
          {views.map((v, i) => (
            <button
              key={v.label}
              type="button"
              data-testid={`studio-mesh-view-${v.label}`}
              title={`${v.label} 视角`}
              onClick={() => setIdx(i)}
              className={cn(
                'rounded px-1.5 py-px text-[10px] transition-colors',
                i === idx ? 'bg-shell-active text-fg' : 'text-fg-3 hover:bg-shell-hover',
              )}
            >
              {VIEW_LABELS[v.label] ?? v.label}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

export default function StudioPreview({
  kind,
  version,
  className,
  /** compact = 主画布小卡(不渲染播放控件,只给状态图形) */
  compact = false,
}: {
  kind: StudioKind;
  version: StudioVersion | undefined;
  className?: string;
  compact?: boolean;
}) {
  const Icon = KIND_ICON[kind];

  if (version === undefined) {
    return (
      <div className={cn('flex items-center justify-center text-fg-4', className)}>
        <Icon size={compact ? 22 : 40} strokeWidth={1.4} />
      </div>
    );
  }

  if (kind === 'text') {
    return (
      <div className={cn('overflow-hidden px-1.5 py-1 text-left', className)}>
        <p className={cn('whitespace-pre-wrap text-fg-3', compact ? 'text-[10px] leading-4' : 'text-2xs leading-5')}>
          {version.text !== undefined && version.text !== '' ? version.text : '(空文本)'}
        </p>
      </div>
    );
  }

  if (kind === 'image') {
    if (version.dataUrl !== undefined) {
      return (
        <div className={cn('overflow-hidden', className)}>
          <img src={version.dataUrl} alt="生成候选" className="h-full w-full object-cover" />
        </div>
      );
    }
    if (version.guid !== undefined && version.assetPath !== undefined) {
      return (
        <div className={cn('overflow-hidden', className)}>
          <Thumb
            item={{ path: version.assetPath, guid: version.guid, type: 'texture', size: 0 } as AssetItem}
            size={compact ? 88 : 220}
          />
        </div>
      );
    }
    // 重开会话:预览字节不在内存,凭 fileRef 如实占位(不伪造缩略)。
    return (
      <div className={cn('flex flex-col items-center justify-center gap-1 text-fg-4', className)}>
        <Icon size={compact ? 18 : 32} strokeWidth={1.4} />
        {version.fileRef !== undefined && (
          <span className="max-w-full truncate px-1 font-mono text-[9px]">{fileBase(version.fileRef)}</span>
        )}
      </div>
    );
  }

  if (kind === 'video' && version.dataUrl !== undefined && !compact) {
    return (
      <div className={cn('overflow-hidden bg-black/60', className)}>
        {/* 生成候选直读 dataUrl(会话态);无字幕轨,产物本身无对白 */}
        {/* eslint-disable-next-line jsx-a11y/media-has-caption */}
        <video src={version.dataUrl} controls className="h-full w-full object-contain" />
      </div>
    );
  }

  if (kind === 'audio' && version.dataUrl !== undefined && !compact) {
    return (
      <div className={cn('flex items-center justify-center px-2', className)}>
        {/* eslint-disable-next-line jsx-a11y/media-has-caption */}
        <audio src={version.dataUrl} controls className="w-full" />
      </div>
    );
  }

  if (kind === 'model') {
    const views = version.previews ?? [];
    if (views.length > 0) {
      return <MeshViews views={views} className={className} compact={compact} />;
    }
    // 落盘预览缺席时退回供应商签名 URL(旧会话/MCP 路径;URL 可能已过期,过期即显占位)。
    if (version.thumbnailUrl !== undefined) {
      return (
        <div className={cn('overflow-hidden', className)}>
          <img src={version.thumbnailUrl} alt="3D 产物缩略(供应商渲染)" className="h-full w-full object-contain" />
        </div>
      );
    }
  }

  // model / 无预览字节的 video/audio:图标 + 文件名如实占位。
  return (
    <div className={cn('flex flex-col items-center justify-center gap-1 text-fg-4', className)}>
      <Icon size={compact ? 18 : 32} strokeWidth={1.4} />
      {version.fileRef !== undefined && (
        <span className="max-w-full truncate px-1 font-mono text-[9px]">{fileBase(version.fileRef)}</span>
      )}
      {version.assetPath !== undefined && (
        <span className="max-w-full truncate px-1 font-mono text-[9px] text-sage">{version.assetPath}</span>
      )}
    </div>
  );
}
