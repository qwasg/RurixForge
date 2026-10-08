import { useEffect, useMemo, useState } from 'react';
import { useAssetStore } from '@/lib/assetStore';
import { useEditorStore } from '@/lib/editorStore';

/**
 * D-045:Text 组件的属性编辑(文字、字号、颜色、对齐、字体)。
 * 引擎侧改字会重新光栅化贴图(rurix 会话按内容重建),所以文本框只在失焦 / Ctrl+Enter 时提交,
 * 不逐键写回;数值与下拉是离散操作,改完即提交。component_set 是整份替换语义,提交时带全量 props。
 */
export default function TextPropsEditor({ entityId, props }: { entityId: number; props: Record<string, unknown> }) {
  const setComponentProps = useEditorStore((s) => s.setComponentProps);
  const items = useAssetStore((s) => s.items);
  const fonts = useMemo(() => items.filter((a) => a.type === 'font'), [items]);
  const [text, setText] = useState(String(props.text ?? ''));
  useEffect(() => setText(String(props.text ?? '')), [props.text]);

  const commit = (patch: Record<string, unknown>) => void setComponentProps(entityId, 'Text', { ...props, ...patch });
  const fontGuid = typeof props.font === 'string' ? props.font : '';
  const color = Array.isArray(props.color) ? (props.color as number[]) : [1, 1, 1, 1];
  const hex = `#${color
    .slice(0, 3)
    .map((v) => Math.round(Math.min(1, Math.max(0, Number(v) || 0)) * 255).toString(16).padStart(2, '0'))
    .join('')}`;
  const field = 'h-6 rounded border border-edge bg-shell-panel px-1.5 text-2xs text-fg outline-none focus:border-acc-ring';

  return (
    <div data-testid="text-props-editor" className="flex flex-col gap-1.5 py-1 text-2xs text-fg-3">
      <textarea
        data-testid="text-props-text"
        rows={2}
        value={text}
        onChange={(e) => setText(e.target.value)}
        onBlur={() => text !== props.text && commit({ text })}
        onKeyDown={(e) => {
          if (e.key === 'Enter' && (e.ctrlKey || e.metaKey) && text !== props.text) commit({ text });
        }}
        className="w-full resize-y rounded border border-edge bg-shell-panel px-1.5 py-1 font-sans text-[12px] text-fg outline-none focus:border-acc-ring"
      />
      <div className="flex items-center gap-1.5">
        <label className="flex items-center gap-1">
          字号
          <input
            data-testid="text-props-size"
            type="number"
            min={1}
            max={512}
            defaultValue={Number(props.size ?? 32)}
            key={`size-${String(props.size)}`}
            onBlur={(e) => {
              const v = Number(e.target.value);
              if (Number.isFinite(v) && v > 0 && v !== props.size) commit({ size: v });
            }}
            className={`${field} w-14`}
          />
        </label>
        <label className="flex items-center gap-1">
          颜色
          <input
            data-testid="text-props-color"
            type="color"
            value={hex}
            onChange={(e) => {
              const h = e.target.value;
              const rgb = [1, 3, 5].map((i) => parseInt(h.slice(i, i + 2), 16) / 255);
              commit({ color: [...rgb, color[3] ?? 1] });
            }}
            className="h-6 w-8 cursor-pointer rounded border border-edge bg-transparent"
          />
        </label>
        <select
          data-testid="text-props-align"
          value={String(props.align ?? 'left')}
          onChange={(e) => commit({ align: e.target.value })}
          className={field}
        >
          <option value="left">左</option>
          <option value="center">中</option>
          <option value="right">右</option>
        </select>
      </div>
      <label className="flex items-center gap-1">
        字体
        <select
          data-testid="text-props-font"
          value={String(props.font ?? '')}
          onChange={(e) => commit({ font: e.target.value })}
          className={`${field} min-w-0 flex-1`}
        >
          {fontGuid !== '' && !fonts.some((f) => f.guid === fontGuid) && (
            <option value={fontGuid}>{fontGuid}(未找到)</option>
          )}
          {fontGuid === '' && <option value="">(未选字体,不渲染)</option>}
          {fonts.map((f) => (
            <option key={f.guid} value={f.guid}>
              {f.path}
            </option>
          ))}
        </select>
      </label>
    </div>
  );
}
