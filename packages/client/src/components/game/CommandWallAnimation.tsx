import { useEffect, useLayoutEffect, useRef } from 'react';
import type { V4State } from '@/lib/sentinelsV4';
import { V5_EVENT, V5_FX, type V5AnimationState } from '@/lib/sentinelsV5';
import { drawVideoFrame, loadVideoAtlas, videoFrame, type VideoAtlas } from '@/lib/sentinelsAnimationAssets';

const WIDTH = 1280, HEIGHT = 720, WORLD_WIDTH = 320 / 9, WORLD_HEIGHT = 20;
type WallEvent = V5AnimationState['events'][number] & { started: number };
type VisualClock = { time: number; target: number; receivedAt: number; running: boolean; speed: number };
const clockTime = (clock: VisualClock, now: number) => Math.min(clock.target,
  clock.time + (clock.running ? Math.max(0, now - clock.receivedAt) / 1000 * clock.speed : 0));
type Props = { state: V4State; animation: V5AnimationState; paused: boolean; connected: boolean;
  onReady(ready: boolean): void; onError(message: string): void };

/** Walls share native topology and time with the engine. Only presentation of
 * the supplied real video frames occurs here; this canvas accepts no input. */
export default function CommandWallAnimation(props: Props) {
  const canvas = useRef<HTMLCanvasElement>(null), latest = useRef(props);
  const clock = useRef<VisualClock>({ time: props.animation.time, target: props.animation.time,
    receivedAt: performance.now(), running: false, speed: props.state.speed });
  const cache = useRef(new Map<number, WallEvent>()), seq = useRef(-1), lastTime = useRef(-1);
  useLayoutEffect(() => {
    const now = performance.now(), animation = props.animation;
    const reset = animation.time < lastTime.current || animation.eventSeq < seq.current;
    const resync = props.connected && !latest.current.connected;
    // Interpolate only up to a confirmed native publication. Pausing or losing
    // the channel freezes this cursor; neither can accumulate local catch-up time.
    // Reconnection uses the fresh server cursor, preserving event sequence IDs.
    clock.current = { time: reset || resync ? animation.time : clockTime(clock.current, now), target: animation.time,
      receivedAt: now, running: props.connected && !props.paused, speed: Math.max(0, props.state.speed) };
    latest.current = props;
    if (reset) { cache.current.clear(); seq.current = -1; }
    lastTime.current = animation.time;
    for (const event of animation.events) {
      // A native ring slot may expire before the interpolated cursor finishes.
      // Keep an accepted event until its actual video lifetime has elapsed.
      if (!event.active || event.seq <= seq.current || ![V5_EVENT.wallHit, V5_EVENT.wallLand, V5_EVENT.wallDestroy].some(type => type === event.type)) continue;
      cache.current.set(event.seq, { ...event, started: animation.time - event.age });
    }
    seq.current = animation.eventSeq;
  }, [props]);

  useEffect(() => {
    let alive = true, raf = 0;
    let wall: VideoAtlas | null = null;
    const effects = new Map<number, VideoAtlas>(), requestedEffects = new Set<number>();
    const requestEffect = (id: number) => {
      if (requestedEffects.has(id)) return;
      const effect = V5_FX.find(effect => effect.id === id); if (!effect) return;
      requestedEffects.add(id);
      void loadVideoAtlas('effects/' + effect.name).then(atlas => { if (alive) effects.set(effect.id, atlas); })
        .catch(error => { if (alive) latest.current.onError((error as Error).message); });
    };
    latest.current.onReady(false);
    void loadVideoAtlas('buildings/cudad-wall').then(atlas => {
      if (!alive) return;
      wall = atlas; latest.current.onReady(true);
    }).catch(error => { if (alive) latest.current.onError((error as Error).message); });
    // Native wall hits currently use heavy-impact; other impact kinds remain
    // available on demand without decoding every native-only effect atlas.
    requestEffect(3);
    const draw = (now: number) => {
      if (!alive) return;
      const current = latest.current, ctx = canvas.current?.getContext('2d');
      const elapsed = clockTime(clock.current, now);
      if (ctx && wall) {
        ctx.clearRect(0, 0, WIDTH, HEIGHT);
        const x = (world: number) => (world / WORLD_WIDTH + .5) * WIDTH;
        const y = (world: number) => (.5 - world / WORLD_HEIGHT) * HEIGHT;
        const size = WIDTH / WORLD_WIDTH;
        const births = new Map<number, WallEvent>();
        const hits: WallEvent[] = [];
        for (const [id, event] of cache.current) {
          const age = Math.max(0, elapsed - event.started);
          const effect = V5_FX.find(effect => effect.id === event.fxKind);
          const lifetime = event.type === V5_EVENT.wallHit && effect ? Math.max(event.duration, effect.frames / effect.fps) : event.duration;
          if (age >= lifetime) { cache.current.delete(id); continue; }
          if (event.type === V5_EVENT.wallLand) births.set(event.subject, event);
          if (event.type === V5_EVENT.wallHit) hits.push(event);
          if (event.type === V5_EVENT.wallDestroy) {
            // The actual last video frame can hold as rubble, then fade out.
            const clip = wall.clips.destroy;
            const length = (clip.endExclusive - clip.start) / clip.fps;
            const fade = age <= length ? 1 : Math.max(0, 1 - (age - length) / Math.max(.01, event.duration - length));
            drawVideoFrame(ctx, wall, videoFrame(wall, 'destroy', age), x(event.x), y(event.y), size / wall.normalizationSpan, size / wall.normalizationSpan, fade);
          }
        }
        for (let cell = 0; cell < current.state.wallCells.length; cell++) {
          if (!current.state.wallCells[cell]) continue;
          const birth = births.get(cell), age = birth ? Math.max(0, elapsed - birth.started) : Infinity;
          const clip = wall.clips.land, landTime = (clip.endExclusive - clip.start) / clip.fps;
          const frame = age < landTime ? videoFrame(wall, 'land', age) : current.state.shieldCells[cell]
            ? videoFrame(wall, 'work', elapsed + cell % 7 * .05) : wall.clips.work.start;
          drawVideoFrame(ctx, wall, frame, x(cell % 32 - 15.5), y(9.5 - Math.floor(cell / 32)), size / wall.normalizationSpan);
        }
        for (const event of hits) {
          requestEffect(event.fxKind);
          const atlas = effects.get(event.fxKind); if (!atlas) continue;
          const clipName = 'oneshot';
          const age = Math.max(0, elapsed - event.started), clip = atlas.clips[clipName];
          if (age >= (clip.endExclusive - clip.start) / clip.fps) continue;
          ctx.save(); ctx.globalCompositeOperation = V5_FX.find(effect => effect.id === event.fxKind)?.blend === 'alpha' ? 'source-over' : 'lighter';
          const effectSize = size * Math.min(2.1, .95 + Math.sqrt(Math.max(0, event.magnitude)) * .1);
          drawVideoFrame(ctx, atlas, videoFrame(atlas, clipName, age), x(event.x), y(event.y), effectSize);
          ctx.restore();
        }
      }
      raf = requestAnimationFrame(draw);
    };
    raf = requestAnimationFrame(draw);
    return () => { alive = false; cancelAnimationFrame(raf); latest.current.onReady(false); };
  }, []);

  return <canvas ref={canvas} width={WIDTH} height={HEIGHT} className="command-wall-animation" aria-hidden="true"/>;
}
