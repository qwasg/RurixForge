import { useEffect } from 'react';
import {
  Box,
  FileAudio,
  FileCode,
  FileImage,
  Film,
  FolderOpen,
  LayoutGrid,
  Package,
} from 'lucide-react';
import { useAssetStore, type AssetItem } from '@/lib/assetStore';

/**
 * 资产缩略图共享件(F10 抽自 AssetsPanel:AssetInspectorPanel 复用):
 * 贴图原图直出;其他类型图标占位,tooltip 说明目前只有贴图提供缩略图。
 */

export function typeIcon(type: string, size: number) {
  switch (type) {
    case 'mesh':
    case 'model':
      return <Box size={size} strokeWidth={1.5} />;
    case 'texture':
      return <FileImage size={size} strokeWidth={1.5} />;
    case 'material':
      return <Package size={size} strokeWidth={1.5} />;
    case 'prefab':
      return <LayoutGrid size={size} strokeWidth={1.5} />;
    case 'scene':
      return <FolderOpen size={size} strokeWidth={1.5} />;
    case 'script':
      return <FileCode size={size} strokeWidth={1.5} />;
    case 'sprite':
      // F-GAME-4:精灵图集(.rxsprite 帧动画语义 → 胶片图标)。
      return <Film size={size} strokeWidth={1.5} />;
    case 'audio':
      return <FileAudio size={size} strokeWidth={1.5} />;
    default:
      return <Package size={size} strokeWidth={1.5} />;
  }
}

export default function Thumb({ item, size }: { item: AssetItem; size: number }) {
  const thumb = useAssetStore((s) => s.thumbs[item.guid]);
  const loadThumb = useAssetStore((s) => s.loadThumb);
  useEffect(() => {
    if (item.type === 'texture' && thumb === undefined) void loadThumb(item);
  }, [item, thumb, loadThumb]);

  if (item.type === 'texture' && thumb && thumb !== 'none') {
    return (
      <img
        src={thumb}
        alt={item.path}
        className="h-full w-full rounded-md object-cover"
        draggable={false}
      />
    );
  }
  const hint =
    item.type === 'texture'
      ? '缩略图不可用(源缺失或超 8MiB 上限)'
      : '该类型暂无预览图(目前只有贴图提供缩略图)';
  return (
    <div
      className="flex h-full w-full items-center justify-center rounded-md bg-shell-sunk text-fg-3"
      title={hint}
    >
      {typeIcon(item.type, size)}
    </div>
  );
}
