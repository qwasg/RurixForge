import { useState } from 'react';
import type { ChatBlock } from '@/lib/timeline';
import ImageLightbox from './design/ImageLightbox';

/** A generated image remains visible when its preceding activity is collapsed. */
export default function GeneratedImage({ block }: { block: Extract<ChatBlock, { kind: 'image' }> }) {
  const [open, setOpen] = useState(false);
  const [failed, setFailed] = useState(false);
  const title = 'Codex 生成的图片';
  return (
    <figure data-testid="generated-image" className="my-2 flex max-w-[560px] flex-col gap-2">
      {failed ? (
        <div className="text-[12px] text-fg-3">图片文件暂不可读</div>
      ) : (
        <button type="button" aria-label="放大生成的图片" onClick={() => setOpen(true)} className="overflow-hidden rounded-lg border border-stroke text-left">
          <img src={block.url} alt={title} loading="lazy" onError={() => setFailed(true)} className="max-h-[480px] w-full object-contain" />
        </button>
      )}
      <figcaption className="flex items-center gap-3 text-[11px] text-fg-3">
        <span className="min-w-0 flex-1 truncate" title={block.imageFileRef}>{block.imageFileRef}</span>
        <a href={block.url} download={block.imageFileRef.split('/').pop()} className="shrink-0 hover:text-fg">保存图片</a>
      </figcaption>
      {open && <ImageLightbox src={block.url} title={title} onClose={() => setOpen(false)} />}
    </figure>
  );
}
