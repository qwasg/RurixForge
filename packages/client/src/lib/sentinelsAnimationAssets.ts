/** Display metadata for genuine extracted video frames. This module never
 * synthesizes motion pixels or predicts game events. */
export const V5_ANIMATION_BASE = '/games/code-sentinels/animation-v5/';
export type FrameBox = [number, number, number, number];
export type VideoClip = { start: number; endExclusive: number; fps: number; loop: boolean };
export type VideoAtlas = {
  image: HTMLImageElement; width: number; height: number; boxes: FrameBox[];
  clips: Record<string, VideoClip>; normalizationSpan: number;
};
const atlases = new Map<string, Promise<VideoAtlas>>();

export function loadVideoAtlas(relative: string): Promise<VideoAtlas> {
  const existing = atlases.get(relative); if (existing) return existing;
  const load = (async () => {
    const response = await fetch(V5_ANIMATION_BASE + relative + '.json');
    if (!response.ok) throw new Error(`动画素材未找到：${relative}`);
    const data = await response.json();
    const boxes = data.boxes ?? data.frames;
    if (!Number.isInteger(data.width) || data.width <= 0 || !Number.isInteger(data.height) || data.height <= 0 || !Array.isArray(boxes) || boxes.length === 0
      || boxes.some((box: unknown) => !Array.isArray(box) || box.length !== 4 || box.some(n => !Number.isFinite(n) || n < 0)
        || box[2] <= 0 || box[3] <= 0 || box[0] + box[2] > data.width || box[1] + box[3] > data.height)) {
      throw new Error(`动画图集坐标无效：${relative}`);
    }
    const clips: Record<string, VideoClip> = {};
    for (const [name, raw] of Object.entries(data.clips ?? {})) {
      const value = raw as VideoClip;
      if (!Number.isInteger(value.start) || !Number.isInteger(value.endExclusive) || value.start < 0
        || value.endExclusive > boxes.length || value.endExclusive <= value.start || !Number.isFinite(value.fps) || value.fps <= 0) {
        throw new Error(`动画片段无效：${relative}/${name}`);
      }
      clips[name] = { start: value.start, endExclusive: value.endExclusive, fps: value.fps, loop: Boolean(value.loop) };
    }
    const requiredClips = relative.startsWith('buildings/') ? ['land', 'work', 'destroy'] : ['oneshot'];
    if (requiredClips.some(name => !clips[name]) || (relative.startsWith('buildings/') && (clips.land.loop || clips.destroy.loop))) {
      throw new Error(`动画片段不完整：${relative}`);
    }
    const normalizationSpan = data.normalizationSpan ?? .640625;
    if (!Number.isFinite(normalizationSpan) || normalizationSpan <= 0 || normalizationSpan > 1) {
      throw new Error(`动画占地比例无效：${relative}`);
    }
    const image = new Image(); image.decoding = 'async'; image.src = V5_ANIMATION_BASE + relative + '.png'; await image.decode();
    if (image.naturalWidth !== data.width || image.naturalHeight !== data.height) throw new Error(`动画尺寸不匹配：${relative}`);
    return { image, width: data.width, height: data.height, boxes, clips, normalizationSpan };
  })();
  atlases.set(relative, load);
  void load.catch(() => { if (atlases.get(relative) === load) atlases.delete(relative); });
  return load;
}

export function videoFrame(atlas: VideoAtlas, clipName: string, age: number): number {
  const clip = atlas.clips[clipName];
  if (!clip) return 0;
  const count = clip.endExclusive - clip.start, index = Math.max(0, Math.floor(age * clip.fps));
  return clip.start + (clip.loop ? index % count : Math.min(count - 1, index));
}

export function drawVideoFrame(ctx: CanvasRenderingContext2D, atlas: VideoAtlas, index: number,
  x: number, y: number, width: number, height = width, opacity = 1) {
  const box = atlas.boxes[index]; if (!box) return;
  ctx.save(); ctx.globalAlpha = opacity;
  ctx.drawImage(atlas.image, box[0], box[1], box[2], box[3], x - width / 2, y - height / 2, width, height);
  ctx.restore();
}
