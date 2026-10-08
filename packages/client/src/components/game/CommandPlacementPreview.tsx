import { useEffect, useState } from 'react';
import { loadVideoAtlas } from '@/lib/sentinelsAnimationAssets';

type Props = { asset: string; x: number; y: number; footprint: number; fallback: string; fallbackSize: number };
type PreviewFrame = { url: string; normalizationSpan: number };
const previewFrames = new Map<string, Promise<PreviewFrame>>();

function loadPreviewFrame(asset: string): Promise<PreviewFrame> {
  const existing = previewFrames.get(asset); if (existing) return existing;
  const loading = loadVideoAtlas('buildings/' + asset).then(atlas => {
    const box = atlas.boxes[atlas.clips.work.start];
    const canvas = document.createElement('canvas'); canvas.width = box[2]; canvas.height = box[3];
    const context = canvas.getContext('2d'); if (!context) throw new Error('Placement frame crop unavailable');
    // Supply only this authentic source frame to the SVG renderer. A nested SVG
    // viewport/filter must never be responsible for hiding the rest of an atlas.
    context.drawImage(atlas.image, box[0], box[1], box[2], box[3], 0, 0, box[2], box[3]);
    return { url: canvas.toDataURL('image/png'), normalizationSpan: atlas.normalizationSpan };
  });
  previewFrames.set(asset, loading);
  void loading.catch(() => { if (previewFrames.get(asset) === loading) previewFrames.delete(asset); });
  return loading;
}

/** Preview the same first work frame and normalized footprint as native V5. */
export default function CommandPlacementPreview({ asset, x, y, footprint, fallback, fallbackSize }: Props) {
  const [loaded, setLoaded] = useState<{ asset: string; frame: PreviewFrame } | null>(null);
  useEffect(() => {
    let active = true;
    void loadPreviewFrame(asset).then(frame => { if (active) setLoaded({ asset, frame }); })
      .catch(() => { if (active) setLoaded(null); });
    return () => { active = false; };
  }, [asset]);
  const frame = loaded?.asset === asset ? loaded.frame : null;
  const size = frame ? footprint / frame.normalizationSpan : fallbackSize;
  return <image href={frame?.url ?? fallback} x={x - size / 2} y={y - size / 2}
    width={size} height={size} className="command-ghost"/>;
}
