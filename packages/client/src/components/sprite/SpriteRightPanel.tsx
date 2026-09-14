import { useEffect, useState } from 'react';
import { ChevronDown, ChevronUp, ListPlus, Plus, Trash2, X } from 'lucide-react';
import { cn } from '@/lib/cn';
import { useSpriteStore, type SpriteClip } from '@/lib/spriteStore';

/**
 * 精灵编辑器右栏(F-GAME-4):帧列表(改名/bbox 数字输入/帧级 pivot)+ 文档级 pivot +
 * clip 编辑器(CRUD/帧序列增删排序/fps/duration/loop/onFinish)+ animator JSON 文本域
 * (实时 parse 校验,坏 JSON 禁存如实报错——v1 诚实形态,不做图形化状态机)。
 */

const inputCls =
  'w-full rounded border border-edge bg-shell-input px-1.5 py-0.5 text-2xs text-fg outline-none focus:border-fg-4';
const numCls =
  'w-full min-w-0 rounded border border-edge bg-shell-input px-1 py-0.5 text-right font-mono text-2xs text-fg outline-none focus:border-fg-4';
const iconBtn =
  'flex h-5 w-5 shrink-0 items-center justify-center rounded text-fg-3 transition-colors hover:bg-shell-hover hover:text-fg-2 disabled:cursor-not-allowed disabled:opacity-30';
const sectionTitle = 'px-2.5 pb-1 pt-2 text-2xs font-medium text-fg-2';

/** 数字输入(草稿态,blur/Enter 提交;非法输入回落原值,不写入半成品)。 */
function NumField({
  value,
  onCommit,
  step = 1,
  testid,
}: {
  value: number;
  onCommit: (v: number) => void;
  step?: number;
  testid?: string;
}) {
  const [draft, setDraft] = useState(String(value));
  useEffect(() => setDraft(String(value)), [value]);
  const commit = () => {
    const v = Number(draft);
    if (Number.isFinite(v)) onCommit(v);
    else setDraft(String(value));
  };
  return (
    <input
      type="number"
      step={step}
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

/** 名称输入(草稿态,blur/Enter 提交改名)。 */
function NameField({
  value,
  onCommit,
  testid,
}: {
  value: string;
  onCommit: (v: string) => void;
  testid?: string;
}) {
  const [draft, setDraft] = useState(value);
  useEffect(() => setDraft(value), [value]);
  return (
    <input
      value={draft}
      data-testid={testid}
      onChange={(e) => setDraft(e.target.value)}
      onBlur={() => draft !== value && onCommit(draft)}
      onKeyDown={(e) => {
        if (e.key === 'Enter') (e.target as HTMLInputElement).blur();
      }}
      className={inputCls}
    />
  );
}

function FramesSection() {
  const doc = useSpriteStore((s) => s.doc);
  const selectedFrame = useSpriteStore((s) => s.selectedFrame);
  const selectedClip = useSpriteStore((s) => s.selectedClip);
  const st = useSpriteStore.getState;
  if (!doc) return null;
  const names = Object.keys(doc.frames);

  return (
    <div className="border-b border-edge pb-2" data-testid="sprite-frames">
      <p className={sectionTitle}>帧({names.length})</p>
      {names.length === 0 && (
        <p className="px-2.5 text-2xs text-fg-4">画布空白处拖拽画框,或用工具栏自动切帧</p>
      )}
      <div className="flex flex-col gap-0.5 px-1.5">
        {names.map((name) => {
          const f = doc.frames[name];
          const sel = name === selectedFrame;
          return (
            <div
              key={name}
              data-testid={`sprite-frame-row-${name}`}
              onClick={() => st().selectFrame(name)}
              className={cn(
                'rounded-md border px-1 py-1',
                sel ? 'border-acc bg-shell-active' : 'border-transparent hover:bg-shell-hover',
              )}
            >
              <div className="flex items-center gap-1">
                <NameField
                  value={name}
                  onCommit={(v) => st().renameFrame(name, v)}
                  testid={`sprite-frame-name-${name}`}
                />
                {selectedClip && (
                  <button
                    type="button"
                    title={`加入 clip「${selectedClip}」帧序列`}
                    className={iconBtn}
                    data-testid={`sprite-frame-addclip-${name}`}
                    onClick={(e) => {
                      e.stopPropagation();
                      st().clipAddFrame(selectedClip, name);
                    }}
                  >
                    <ListPlus size={11} />
                  </button>
                )}
                <button
                  type="button"
                  title="删除帧(clip 引用一并移除)"
                  className={iconBtn}
                  data-testid={`sprite-frame-del-${name}`}
                  onClick={(e) => {
                    e.stopPropagation();
                    st().deleteFrame(name);
                  }}
                >
                  <Trash2 size={11} />
                </button>
              </div>
              {sel && (
                <div className="mt-1 space-y-1">
                  <div className="grid grid-cols-4 gap-1">
                    {([0, 1, 2, 3] as const).map((i) => (
                      <label key={i} className="flex flex-col gap-px text-2xs text-fg-4">
                        {['x', 'y', 'w', 'h'][i]}
                        <NumField
                          value={f.bbox[i]}
                          testid={`sprite-bbox-${'xywh'[i]}`}
                          onCommit={(v) => {
                            const next = [...f.bbox] as [number, number, number, number];
                            next[i] = v;
                            st().setFrameBbox(name, next, { coalesce: `bbox-input:${name}:${i}` });
                          }}
                        />
                      </label>
                    ))}
                  </div>
                  <div className="flex items-end gap-1">
                    <label className="flex flex-1 flex-col gap-px text-2xs text-fg-4">
                      pivot x{f.pivot ? '(帧级)' : '(继承)'}
                      <NumField
                        value={(f.pivot ?? doc.pivot)[0]}
                        step={0.05}
                        testid="sprite-frame-pivot-x"
                        onCommit={(v) => st().setFramePivot(name, [v, (f.pivot ?? doc.pivot)[1]])}
                      />
                    </label>
                    <label className="flex flex-1 flex-col gap-px text-2xs text-fg-4">
                      pivot y
                      <NumField
                        value={(f.pivot ?? doc.pivot)[1]}
                        step={0.05}
                        testid="sprite-frame-pivot-y"
                        onCommit={(v) => st().setFramePivot(name, [(f.pivot ?? doc.pivot)[0], v])}
                      />
                    </label>
                    {f.pivot && (
                      <button
                        type="button"
                        data-testid="sprite-frame-pivot-clear"
                        title="清除帧级覆盖,回落文档级 pivot"
                        className="h-5 shrink-0 rounded border border-edge px-1.5 text-2xs text-fg-3 hover:bg-shell-hover"
                        onClick={() => st().setFramePivot(name, null)}
                      >
                        清除覆盖
                      </button>
                    )}
                  </div>
                </div>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}

function ClipEditor({ name, clip }: { name: string; clip: SpriteClip }) {
  const st = useSpriteStore.getState;
  return (
    <div className="mx-1.5 rounded-md border border-edge bg-shell-panel p-1.5" data-testid="sprite-clip-editor">
      <div className="flex items-center gap-1">
        <NameField value={name} onCommit={(v) => st().renameClip(name, v)} testid="sprite-clip-rename" />
      </div>

      {/* 帧序列(点帧列表 ⊕ 加入;此处排序/移除) */}
      <p className="pb-0.5 pt-1.5 text-2xs text-fg-4">帧序列(帧列表点 ⊕ 加入)</p>
      {clip.frames.length === 0 ? (
        <p className="text-2xs text-warn">空 clip 无法保存(后端拒绝),先加入帧</p>
      ) : (
        <div className="flex flex-col gap-0.5">
          {clip.frames.map((f, i) => (
            <div
              key={`${f}-${i}`}
              className="flex items-center gap-1 rounded bg-shell-sunk px-1 py-0.5"
              data-testid={`sprite-clip-seq-${i}`}
            >
              <span className="w-4 shrink-0 text-right font-mono text-2xs text-fg-4">{i}</span>
              <span className="min-w-0 flex-1 truncate text-2xs text-fg-2">{f}</span>
              <button
                type="button"
                title="上移"
                className={iconBtn}
                disabled={i === 0}
                data-testid={`sprite-clip-seq-up-${i}`}
                onClick={() => st().clipMoveFrame(name, i, -1)}
              >
                <ChevronUp size={11} />
              </button>
              <button
                type="button"
                title="下移"
                className={iconBtn}
                disabled={i === clip.frames.length - 1}
                data-testid={`sprite-clip-seq-down-${i}`}
                onClick={() => st().clipMoveFrame(name, i, 1)}
              >
                <ChevronDown size={11} />
              </button>
              <button
                type="button"
                title="移出序列"
                className={iconBtn}
                data-testid={`sprite-clip-seq-remove-${i}`}
                onClick={() => st().clipRemoveFrame(name, i)}
              >
                <X size={11} />
              </button>
            </div>
          ))}
        </div>
      )}

      {/* 时长/循环表单(duration 总秒数优先于 fps;留空 = 按 fps) */}
      <div className="mt-1.5 grid grid-cols-2 gap-1">
        <label className="flex flex-col gap-px text-2xs text-fg-4">
          fps
          <NumField
            value={clip.fps}
            step={1}
            testid="sprite-clip-fps"
            onCommit={(v) => st().setClipMeta(name, { fps: v })}
          />
        </label>
        <label className="flex flex-col gap-px text-2xs text-fg-4">
          duration 秒(优先;0=清)
          <NumField
            value={clip.duration ?? 0}
            step={0.1}
            testid="sprite-clip-duration"
            onCommit={(v) => st().setClipMeta(name, { duration: v > 0 ? v : null })}
          />
        </label>
      </div>
      <div className="mt-1.5 flex items-center gap-3">
        <label className="flex items-center gap-1 text-2xs text-fg-3">
          <input
            type="checkbox"
            checked={clip.loop}
            data-testid="sprite-clip-loop"
            onChange={(e) => st().setClipMeta(name, { loop: e.target.checked })}
          />
          循环
        </label>
        <label className="flex items-center gap-1 text-2xs text-fg-3">
          收尾
          <select
            value={clip.onFinish}
            data-testid="sprite-clip-onfinish"
            disabled={clip.loop}
            title={clip.loop ? '循环 clip 无收尾行为' : '非循环收尾:hold 停末帧 / first 回首帧'}
            onChange={(e) => st().setClipMeta(name, { onFinish: e.target.value as 'hold' | 'first' })}
            className="rounded border border-edge bg-shell-input px-1 py-0.5 text-2xs text-fg outline-none disabled:opacity-40"
          >
            <option value="hold">hold(停末帧)</option>
            <option value="first">first(回首帧)</option>
          </select>
        </label>
      </div>
    </div>
  );
}

function ClipsSection() {
  const doc = useSpriteStore((s) => s.doc);
  const selectedClip = useSpriteStore((s) => s.selectedClip);
  const [newName, setNewName] = useState('');
  const st = useSpriteStore.getState;
  if (!doc) return null;
  const names = Object.keys(doc.clips);

  const create = () => {
    if (newName.trim() === '') return;
    st().addClip(newName);
    setNewName('');
  };

  return (
    <div className="border-b border-edge pb-2" data-testid="sprite-clips">
      <p className={sectionTitle}>动画 clip({names.length})</p>
      <div className="flex items-center gap-1 px-2.5 pb-1">
        <input
          value={newName}
          placeholder="新 clip 名,如 walk"
          data-testid="sprite-clip-new-name"
          onChange={(e) => setNewName(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') create();
          }}
          className={inputCls}
        />
        <button
          type="button"
          title="新建 clip(选中帧自动作为首帧)"
          data-testid="sprite-clip-add"
          onClick={create}
          className="flex h-5 shrink-0 items-center gap-0.5 rounded border border-edge px-1.5 text-2xs text-fg-3 hover:bg-shell-hover"
        >
          <Plus size={11} />
          新建
        </button>
      </div>
      <div className="flex flex-col gap-0.5 px-1.5 pb-1">
        {names.map((n) => (
          <div
            key={n}
            data-testid={`sprite-clip-row-${n}`}
            onClick={() => st().selectClip(n)}
            className={cn(
              'flex items-center gap-1 rounded-md px-1.5 py-0.5',
              n === selectedClip ? 'bg-shell-active text-fg' : 'text-fg-3 hover:bg-shell-hover',
            )}
          >
            <span className="min-w-0 flex-1 truncate text-2xs">{n}</span>
            <span className="shrink-0 text-2xs text-fg-4">{doc.clips[n].frames.length} 帧</span>
            <button
              type="button"
              title="删除 clip"
              className={iconBtn}
              data-testid={`sprite-clip-del-${n}`}
              onClick={(e) => {
                e.stopPropagation();
                st().deleteClip(n);
              }}
            >
              <Trash2 size={11} />
            </button>
          </div>
        ))}
      </div>
      {selectedClip && doc.clips[selectedClip] && (
        <ClipEditor name={selectedClip} clip={doc.clips[selectedClip]} />
      )}
    </div>
  );
}

function AnimatorSection() {
  const animatorText = useSpriteStore((s) => s.animatorText);
  const animatorError = useSpriteStore((s) => s.animatorError);
  const setAnimatorText = useSpriteStore((s) => s.setAnimatorText);
  return (
    <div className="pb-2" data-testid="sprite-animator">
      <p className={sectionTitle}>animator 状态机(JSON;v1 无图形化编辑)</p>
      <div className="px-2.5">
        <textarea
          value={animatorText}
          onChange={(e) => setAnimatorText(e.target.value)}
          rows={8}
          spellCheck={false}
          placeholder={'留空 = 无状态机。形如:\n{ "defaultState": "idle", "states": { "idle": { "clip": "idle" } }, ... }'}
          data-testid="sprite-animator-input"
          className="w-full resize-y rounded-md border border-edge bg-shell-input px-2 py-1 font-mono text-2xs text-fg outline-none focus:border-fg-4"
        />
        {animatorError && (
          <p className="pt-0.5 text-2xs text-warn" data-testid="sprite-animator-error">
            JSON 无效(禁存):{animatorError}
          </p>
        )}
      </div>
    </div>
  );
}

export default function SpriteRightPanel() {
  const doc = useSpriteStore((s) => s.doc);
  const guid = useSpriteStore((s) => s.guid);
  const texW = useSpriteStore((s) => s.texW);
  const texH = useSpriteStore((s) => s.texH);
  const st = useSpriteStore.getState;
  if (!doc) return null;

  return (
    <div
      className="flex w-[300px] shrink-0 flex-col overflow-y-auto border-l border-edge bg-shell-sidebar"
      data-testid="sprite-right-panel"
    >
      {/* 文档信息 + 文档级 pivot */}
      <div className="border-b border-edge pb-2">
        <p className={sectionTitle}>文档</p>
        <div className="space-y-0.5 px-2.5 text-2xs text-fg-3">
          <p className="truncate font-mono" title={guid ?? ''}>
            GUID:{guid ?? '(未知)'}
          </p>
          <p>
            贴图:<span className="font-mono">{doc.texture}</span>
            {texW > 0 && (
              <span className="pl-1 text-fg-4">
                {texW}×{texH}px
              </span>
            )}
          </p>
        </div>
        <div className="flex gap-1 px-2.5 pt-1">
          <label className="flex flex-1 flex-col gap-px text-2xs text-fg-4">
            文档级 pivot x(0..1)
            <NumField
              value={doc.pivot[0]}
              step={0.05}
              testid="sprite-doc-pivot-x"
              onCommit={(v) => st().setDocPivot([v, doc.pivot[1]])}
            />
          </label>
          <label className="flex flex-1 flex-col gap-px text-2xs text-fg-4">
            pivot y(1=脚底)
            <NumField
              value={doc.pivot[1]}
              step={0.05}
              testid="sprite-doc-pivot-y"
              onCommit={(v) => st().setDocPivot([doc.pivot[0], v])}
            />
          </label>
        </div>
      </div>

      <FramesSection />
      <ClipsSection />
      <AnimatorSection />
    </div>
  );
}
