import { useEffect, useRef, useState } from 'react';
import { Pause, Play, Plus, Trash2 } from 'lucide-react';
import { cn } from '@/lib/cn';
import { useRigStore } from '@/lib/rigStore';
import { sampleTrack, splitTarget, type Interp, type RigDoc, type RigTrack } from '@/lib/rigTypes';

/**
 * 动画模式底部时间轴(F-GAME-5 W5):clip 列表(新建/删除/时长/循环)、轨道(增删、插值方式)、
 * 关键帧(双击轨道空白处加键 / 拖动菱形改时间 / 选中后改值或 Delete 删除)、播放头拖拽
 * (clip + time 送入节流 rig_evaluate 求值,画布实时预览)。
 */

const numCls =
  'w-14 min-w-0 rounded border border-edge bg-shell-input px-1 py-0.5 text-right font-mono text-2xs text-fg outline-none focus:border-fg-4';
const selectCls = 'h-6 rounded border border-edge bg-shell-input px-1 text-2xs text-fg outline-none focus:border-fg-4';
const iconBtn =
  'flex h-5 w-5 shrink-0 items-center justify-center rounded text-fg-3 transition-colors hover:bg-shell-hover hover:text-fg-2 disabled:cursor-not-allowed disabled:opacity-30';

const BONE_CHANNELS = ['rotation', 'x', 'y', 'sx', 'sy'];

/** 可加轨道的全部目标串(param:/bone:.ch/part:.opacity)。 */
export function trackTargets(doc: RigDoc): string[] {
  return [
    ...doc.parameters.map((p) => `param:${p.id}`),
    ...doc.bones.flatMap((b) => BONE_CHANNELS.map((c) => `bone:${b.id}.${c}`)),
    ...doc.parts.map((p) => `part:${p.id}.opacity`),
  ];
}

/** 新键的缺省值:有键 → 该时刻采样;参数轨 → 当前参数值;缩放/不透明度 1;其余 0。 */
export function defaultKeyValue(track: RigTrack, t: number, params: Record<string, number>, doc: RigDoc): number {
  if (track.keys.length > 0) return sampleTrack(track, t);
  const s = splitTarget(track.target);
  if (!s) return 0;
  if (s.kind === 'param') return params[s.id] ?? doc.parameters.find((p) => p.id === s.id)?.default ?? 0;
  if (s.channel === 'sx' || s.channel === 'sy' || s.channel === 'opacity') return 1;
  return 0;
}

function NumField({ value, onCommit, testid }: { value: number; onCommit: (v: number) => void; testid?: string }) {
  const [draft, setDraft] = useState(String(value));
  useEffect(() => setDraft(String(value)), [value]);
  const commit = () => {
    const v = Number(draft);
    if (draft.trim() !== '' && Number.isFinite(v)) onCommit(v);
    else setDraft(String(value));
  };
  return (
    <input
      value={draft}
      data-testid={testid}
      onChange={(e) => setDraft(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => {
        if (e.key === 'Enter') commit();
      }}
      className={numCls}
    />
  );
}

export default function RigTimeline() {
  const doc = useRigStore((s) => s.doc);
  const clipName = useRigStore((s) => s.clip);
  const time = useRigStore((s) => s.time);
  const playing = useRigStore((s) => s.playing);
  const params = useRigStore((s) => s.params);
  const st = useRigStore.getState;

  const [newName, setNewName] = useState('');
  const [selKey, setSelKey] = useState<{ track: number; key: number } | null>(null);
  const keyDrag = useRef<{ track: number; key: number; gesture: string } | null>(null);
  const scrubbing = useRef(false);
  const gesture = useRef(0);

  useEffect(() => setSelKey(null), [clipName]);

  if (!doc) return null;
  const clip = clipName ? doc.clips[clipName] : undefined;
  const dur = clip && clip.duration > 0 ? clip.duration : 1;
  const clipNames = Object.keys(doc.clips);
  const used = new Set(clip?.tracks.map((t) => t.target) ?? []);
  const available = trackTargets(doc).filter((t) => !used.has(t));

  /** 指针 x → clip 时间(按元素宽度;宽度 0 时返回 0)。 */
  const timeAt = (el: HTMLElement, clientX: number): number => {
    const r = el.getBoundingClientRect();
    if (r.width <= 0) return 0;
    return Math.min(dur, Math.max(0, ((clientX - r.left) / r.width) * dur));
  };

  const selTrack = selKey && clip ? clip.tracks[selKey.track] : undefined;
  const selKeyVal = selTrack && selKey ? selTrack.keys[selKey.key] : undefined;

  return (
    <div
      className="flex h-[210px] shrink-0 flex-col border-t border-edge bg-shell-sidebar"
      data-testid="rig-timeline"
      tabIndex={-1}
      onKeyDown={(e) => {
        if ((e.key === 'Delete' || e.key === 'Backspace') && selKey && clipName) {
          const t = e.target as HTMLElement;
          if (t.closest('input, select, textarea')) return;
          e.preventDefault();
          st().deleteKeyAt(clipName, selKey.track, selKey.key);
          setSelKey(null);
        }
      }}
    >
      {/* clip 工具条 */}
      <div className="flex h-8 shrink-0 items-center gap-1.5 border-b border-edge px-2 text-2xs text-fg-3">
        <button
          type="button"
          className="glass-btn h-6"
          data-testid="rig-timeline-play"
          disabled={!clip}
          title={playing ? '暂停 Pause' : '播放 Play'}
          onClick={() => (playing ? st().pause() : st().play())}
        >
          {playing ? <Pause size={11} /> : <Play size={11} />}
          {playing ? '暂停' : '播放'}
        </button>
        <select
          className={selectCls}
          data-testid="rig-timeline-clip"
          value={clipName ?? ''}
          onChange={(e) => st().selectClip(e.target.value || null)}
        >
          {clipNames.length === 0 && <option value="">(无 clip)</option>}
          {clipNames.map((n) => (
            <option key={n} value={n}>
              {n}
            </option>
          ))}
        </select>
        {clip && clipName && (
          <>
            <span>时长</span>
            <NumField value={clip.duration} onCommit={(v) => st().setClipMeta(clipName, { duration: v })} testid="rig-clip-duration" />
            <label className="flex items-center gap-0.5">
              <input
                type="checkbox"
                checked={clip.loop}
                onChange={(e) => st().setClipMeta(clipName, { loop: e.target.checked })}
              />
              循环
            </label>
            <button
              type="button"
              className={iconBtn}
              title="删除该 clip"
              data-testid="rig-clip-delete"
              onClick={() => st().removeClip(clipName)}
            >
              <Trash2 size={11} />
            </button>
          </>
        )}
        <span className="font-mono text-fg-2" data-testid="rig-timeline-time">
          {time.toFixed(2)}s
        </span>
        <span className="flex-1" />
        <input
          value={newName}
          placeholder="新 clip 名"
          data-testid="rig-clip-new-name"
          onChange={(e) => setNewName(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && newName.trim()) {
              st().addClip(newName);
              setNewName('');
            }
          }}
          className="h-6 w-24 rounded border border-edge bg-shell-input px-1.5 text-2xs text-fg outline-none focus:border-fg-4"
        />
        <button
          type="button"
          className="glass-btn h-6"
          data-testid="rig-clip-add"
          disabled={newName.trim() === ''}
          onClick={() => {
            st().addClip(newName);
            setNewName('');
          }}
        >
          <Plus size={11} />
          clip
        </button>
        {clip && clipName && (
          <select
            className={selectCls}
            value=""
            data-testid="rig-track-add"
            onChange={(e) => {
              if (e.target.value) st().addTrack(clipName, e.target.value);
            }}
          >
            <option value="">+ 轨道…</option>
            {available.map((t) => (
              <option key={t} value={t}>
                {t}
              </option>
            ))}
          </select>
        )}
      </div>

      {!clip || !clipName ? (
        <p className="p-3 text-2xs text-fg-4">没有 clip:在右上输入名称新建,或让 agent 用 rig_patch set_clip 生成。</p>
      ) : (
        <div className="flex min-h-0 flex-1">
          {/* 轨道名列 */}
          <div className="w-[200px] shrink-0 overflow-y-auto border-r border-edge">
            <div className="h-5 border-b border-edge" />
            {clip.tracks.map((t, ti) => (
              <div key={t.target} className="flex h-6 items-center gap-1 border-b border-edge/50 px-1.5 text-2xs">
                <span className="min-w-0 flex-1 truncate font-mono text-fg-2" title={t.target}>
                  {t.target}
                </span>
                <select
                  className="h-5 rounded border border-edge bg-shell-input text-2xs text-fg"
                  value={t.interp ?? 'linear'}
                  title="插值 interp"
                  onChange={(e) => st().setTrackInterp(clipName, ti, e.target.value as Interp)}
                >
                  <option value="linear">linear</option>
                  <option value="step">step</option>
                  <option value="smooth">smooth</option>
                </select>
                <button
                  type="button"
                  className={iconBtn}
                  title="在播放头处加键(参数轨取当前参数值)"
                  data-testid={`rig-track-key-${ti}`}
                  onClick={() => st().addKey(clipName, ti, time, defaultKeyValue(t, time, params, doc))}
                >
                  <Plus size={10} />
                </button>
                <button
                  type="button"
                  className={iconBtn}
                  title="删除轨道"
                  onClick={() => {
                    st().removeTrack(clipName, ti);
                    setSelKey(null);
                  }}
                >
                  <Trash2 size={10} />
                </button>
              </div>
            ))}
            {clip.tracks.length === 0 && <p className="p-2 text-2xs text-fg-4">无轨道:右上「+ 轨道」添加</p>}
          </div>

          {/* 时间区:标尺 + 轨道 lane + 播放头 */}
          <div className="relative min-w-0 flex-1 overflow-y-auto">
            <div
              className="relative h-5 cursor-ew-resize border-b border-edge bg-shell-panel"
              data-testid="rig-timeline-ruler"
              onPointerDown={(e) => {
                e.currentTarget.setPointerCapture?.(e.pointerId);
                scrubbing.current = true;
                st().pause();
                st().setTime(timeAt(e.currentTarget, e.clientX));
              }}
              onPointerMove={(e) => {
                if (scrubbing.current) st().setTime(timeAt(e.currentTarget, e.clientX));
              }}
              onPointerUp={() => {
                scrubbing.current = false;
              }}
            >
              {Array.from({ length: 11 }, (_, i) => (
                <span
                  key={i}
                  className="pointer-events-none absolute top-0 h-full border-l border-edge pl-0.5 font-mono text-[9px] text-fg-4"
                  style={{ left: `${i * 10}%` }}
                >
                  {((dur * i) / 10).toFixed(dur < 2 ? 2 : 1)}
                </span>
              ))}
            </div>
            {clip.tracks.map((t, ti) => (
              <div
                key={t.target}
                className="relative h-6 border-b border-edge/50"
                data-testid={`rig-track-lane-${ti}`}
                onDoubleClick={(e) => {
                  const tt = timeAt(e.currentTarget, e.clientX);
                  st().addKey(clipName, ti, tt, defaultKeyValue(t, tt, params, doc));
                }}
                onPointerMove={(e) => {
                  const d = keyDrag.current;
                  if (!d || d.track !== ti) return;
                  const idx = st().moveKeyTo(clipName, ti, d.key, timeAt(e.currentTarget, e.clientX), d.gesture);
                  d.key = idx;
                  setSelKey({ track: ti, key: idx });
                }}
                onPointerUp={() => {
                  keyDrag.current = null;
                }}
              >
                {t.keys.map((k, ki) => {
                  const isSel = selKey?.track === ti && selKey.key === ki;
                  return (
                    <span
                      key={`${ki}-${k[0]}`}
                      role="button"
                      title={`t=${k[0]} v=${k[1]}`}
                      data-testid={`rig-key-${ti}-${ki}`}
                      onPointerDown={(e) => {
                        e.stopPropagation();
                        e.currentTarget.parentElement?.setPointerCapture?.(e.pointerId);
                        gesture.current += 1;
                        keyDrag.current = { track: ti, key: ki, gesture: `rig-key-${gesture.current}` };
                        setSelKey({ track: ti, key: ki });
                      }}
                      onDoubleClick={(e) => e.stopPropagation()}
                      className={cn(
                        'absolute top-1/2 h-2.5 w-2.5 -translate-x-1/2 -translate-y-1/2 rotate-45 cursor-pointer border',
                        isSel ? 'border-white bg-acc' : 'border-fg-3 bg-fg-2',
                      )}
                      style={{ left: `${(k[0] / dur) * 100}%` }}
                    />
                  );
                })}
              </div>
            ))}
            <div
              className="pointer-events-none absolute bottom-0 top-0 w-px bg-acc"
              style={{ left: `${(Math.min(time, dur) / dur) * 100}%` }}
              data-testid="rig-timeline-playhead"
            />
          </div>

          {/* 选中关键帧 */}
          <div className="w-[150px] shrink-0 border-l border-edge p-2 text-2xs text-fg-3">
            {selKey && selKeyVal ? (
              <div className="flex flex-col gap-1" data-testid="rig-key-inspector">
                <span className="truncate font-mono text-fg-2" title={selTrack?.target}>
                  {selTrack?.target}
                </span>
                <label className="flex items-center justify-between gap-1">
                  时间
                  <NumField
                    value={selKeyVal[0]}
                    onCommit={(v) => {
                      const idx = st().moveKeyTo(clipName, selKey.track, selKey.key, v);
                      setSelKey({ track: selKey.track, key: idx });
                    }}
                  />
                </label>
                <label className="flex items-center justify-between gap-1">
                  值
                  <NumField
                    value={selKeyVal[1]}
                    onCommit={(v) => st().setKeyValue(clipName, selKey.track, selKey.key, v)}
                    testid="rig-key-value"
                  />
                </label>
                <button
                  type="button"
                  className="glass-btn h-6"
                  onClick={() => {
                    st().deleteKeyAt(clipName, selKey.track, selKey.key);
                    setSelKey(null);
                  }}
                >
                  <Trash2 size={10} />
                  删除键
                </button>
              </div>
            ) : (
              <p className="text-fg-4">双击轨道加键 · 拖菱形改时间 · 选中后 Delete 删除</p>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
