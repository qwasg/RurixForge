import { useEffect, useRef, useState } from 'react';
import { Check, Trash2 } from 'lucide-react';
import { cn } from '@/lib/cn';
import { CATEGORY_META, type EntityCategory } from '@/lib/entityCategory';
import { useEditorStore } from '@/lib/editorStore';
import { useDesignBoardStore, type EntityKindDef, type KindTone } from '@/lib/designBoardStore';
import { useToastStore } from '@/lib/toastStore';
import { TONE_META } from './DesignBoardNode';

/**
 * 自定义实体类型编辑浮层(画板波 v2)。
 * 角色 / 地图之外,用户可自定义由 ECS 组件构成的实体类型:名称 + 配色 +
 * 引擎分类(后端 classify 只认 role/map/interaction,必须落其一)+ 默认特性
 * (勾选已注册组件,新建该类型实体时自动铺开)。
 * 自定义类型存 localStorage 随画板一起持久化;仍有实体在用时拒删(如实给出原因)。
 */

const TONES: KindTone[] = ['info', 'sage', 'warn', 'acc', 'danger'];

const CATEGORIES: EntityCategory[] = ['role', 'map', 'interaction'];

export interface DesignBoardKindFormProps {
  /** 编辑既有自定义类型;缺省 = 新建 */
  editing?: EntityKindDef;
  onClose: () => void;
}

export default function DesignBoardKindForm({ editing, onClose }: DesignBoardKindFormProps) {
  const componentTypes = useEditorStore((s) => s.componentTypes);
  const addKind = useDesignBoardStore((s) => s.addKind);
  const updateKind = useDesignBoardStore((s) => s.updateKind);
  const removeKind = useDesignBoardStore((s) => s.removeKind);
  const push = useToastStore((s) => s.push);

  const [label, setLabel] = useState(editing?.label ?? '');
  const [tone, setTone] = useState<KindTone>(editing?.tone ?? 'warn');
  const [category, setCategory] = useState<EntityCategory>(editing?.category ?? 'role');
  const [features, setFeatures] = useState<string[]>(editing?.defaultFeatures ?? []);
  const [err, setErr] = useState<string | null>(null);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const onDown = (ev: MouseEvent) => {
      if (!ref.current?.contains(ev.target as Node)) onClose();
    };
    document.addEventListener('mousedown', onDown, true);
    return () => document.removeEventListener('mousedown', onDown, true);
  }, [onClose]);

  const toggleFeature = (name: string) =>
    setFeatures((prev) => (prev.includes(name) ? prev.filter((x) => x !== name) : [...prev, name]));

  const submit = () => {
    const draft = { label, tone, category, defaultFeatures: features };
    if (editing) {
      updateKind(editing.id, draft);
      // updateKind 对空名/撞名静默忽略,这里回读确认是否真的写进去了
      const after = useDesignBoardStore.getState().kinds.find((k) => k.id === editing.id);
      if (after?.label !== label.trim()) {
        setErr('名称为空或与已有类型重名');
        return;
      }
      push('success', `已更新实体类型「${label.trim()}」`);
    } else {
      const id = addKind(draft);
      if (id === null) {
        setErr('名称为空或与已有类型重名');
        return;
      }
      push('success', `已新建实体类型「${label.trim()}」`);
    }
    onClose();
  };

  const onDelete = () => {
    if (!editing) return;
    const reason = removeKind(editing.id);
    if (reason !== null) {
      setErr(reason);
      return;
    }
    push('success', `已删除实体类型「${editing.label}」`);
    onClose();
  };

  return (
    <div
      ref={ref}
      data-testid="board-kind-form"
      className="absolute left-2 top-full z-30 mt-1 w-[268px] rounded-md border border-edge-strong bg-shell-float p-2 shadow-pop"
    >
      <p className="pb-1 text-2xs font-medium text-fg">
        {editing ? `编辑实体类型「${editing.label}」` : '新建实体类型'}
      </p>

      <label className="block pb-1 text-[10px] text-fg-4" htmlFor="board-kind-label">
        名称
      </label>
      <input
        id="board-kind-label"
        autoFocus
        data-testid="board-kind-label"
        value={label}
        onChange={(e) => {
          setLabel(e.target.value);
          setErr(null);
        }}
        onKeyDown={(e) => {
          if (e.key === 'Enter') submit();
          if (e.key === 'Escape') onClose();
        }}
        placeholder="如:敌人 / 道具 / 触发器"
        className="mb-2 w-full rounded border border-edge-strong bg-shell-panel px-1.5 py-0.5 text-2xs text-fg outline-none placeholder:text-fg-4 focus:border-fg-4"
      />

      <p className="pb-1 text-[10px] text-fg-4">配色</p>
      <div className="mb-2 flex items-center gap-1.5">
        {TONES.map((t) => (
          <button
            key={t}
            type="button"
            data-testid={`board-kind-tone-${t}`}
            aria-label={`配色 ${t}`}
            aria-pressed={tone === t}
            onClick={() => setTone(t)}
            className={cn(
              'h-4 w-4 rounded-full border-2 transition-colors',
              TONE_META[t].dot,
              tone === t ? 'border-fg' : 'border-transparent',
            )}
          />
        ))}
      </div>

      <label className="block pb-1 text-[10px] text-fg-4" htmlFor="board-kind-category">
        引擎分类(后端只认 role / map / interaction)
      </label>
      <select
        id="board-kind-category"
        data-testid="board-kind-category"
        value={category}
        onChange={(e) => setCategory(e.target.value as EntityCategory)}
        className="mb-2 w-full rounded border border-edge-strong bg-shell-panel px-1 py-0.5 text-2xs text-fg outline-none focus:border-fg-4"
      >
        {CATEGORIES.map((c) => (
          <option key={c} value={c}>
            {CATEGORY_META[c].label}({c})
          </option>
        ))}
      </select>

      <p className="pb-1 text-[10px] text-fg-4">默认特性(新建该类型实体时自动铺开)</p>
      {componentTypes.length === 0 ? (
        <p className="pb-1 text-[10px] text-fg-4">组件清单未加载(后端离线),可稍后在实体上逐个添加</p>
      ) : (
        <div className="mb-2 max-h-[104px] overflow-y-auto rounded border border-edge p-1">
          {componentTypes.map((t) => (
            <label
              key={t.name}
              className="flex cursor-pointer items-center gap-1 rounded px-1 py-px text-2xs text-fg-2 hover:bg-shell-hover"
            >
              <input
                type="checkbox"
                data-testid={`board-kind-feat-${t.name}`}
                checked={features.includes(t.name)}
                onChange={() => toggleFeature(t.name)}
              />
              <span className="truncate font-mono text-[10px]">{t.name}</span>
            </label>
          ))}
        </div>
      )}

      {err && (
        <p data-testid="board-kind-error" className="pb-1 text-[10px] text-danger">
          {err}
        </p>
      )}

      <div className="flex items-center gap-1.5">
        {editing && (
          <button
            type="button"
            data-testid="board-kind-delete"
            title="删除该自定义类型(仍有实体在用时不可删)"
            onClick={onDelete}
            className="flex items-center gap-1 rounded border border-edge-strong px-1.5 py-0.5 text-2xs text-fg-3 transition-colors hover:border-danger/50 hover:text-danger"
          >
            <Trash2 size={10} strokeWidth={1.8} />
            删除
          </button>
        )}
        <span className="flex-1" />
        <button
          type="button"
          data-testid="board-kind-cancel"
          onClick={onClose}
          className="rounded border border-edge-strong px-1.5 py-0.5 text-2xs text-fg-3 transition-colors hover:bg-shell-hover"
        >
          取消
        </button>
        <button
          type="button"
          data-testid="board-kind-save"
          onClick={submit}
          className="flex items-center gap-1 rounded bg-acc px-2 py-0.5 text-2xs font-medium text-fg-inv transition-opacity hover:opacity-90"
        >
          <Check size={10} strokeWidth={2} />
          保存
        </button>
      </div>
    </div>
  );
}
