import type { EditorAnnotation } from '@forge/protocol';
import { revealAnnotation } from '@/lib/editorReferences';
export default function AnnotationChips({ annotations, onRemove, onNote }: { annotations: EditorAnnotation[]; onRemove?: (id: string) => void; onNote?: (id: string, note: string) => void }) {
  return <div data-testid="editor-annotations" className="flex flex-wrap gap-1.5 px-1">
    {annotations.map((item) => <div key={item.id} className="max-w-full rounded-md border border-edge bg-shell-panel px-2 py-1 text-xs">
      <div className="flex items-center gap-1"><button type="button" className="truncate text-acc" title="定位原对象" onClick={() => void revealAnnotation(item)}>{item.label ?? item.reference.kind}</button>
        {onRemove && <button type="button" aria-label={`移除批注 ${item.label ?? item.id}`} onClick={() => onRemove(item.id)}>×</button>}</div>
      {item.reference.identityPersisted === false && <span className="text-[10px] text-warn">临时引用 · 保存场景后身份可持久化</span>}
      {item.observationId && <div className="relative mt-1 w-40"><img src={`/api/forge/editor/images/${encodeURIComponent(item.observationId)}?workspaceId=${encodeURIComponent(item.reference.workspaceId)}`} alt="批注捕获帧" className="w-full rounded" />{item.reference.selection?.region && <div className="pointer-events-none absolute border border-blue-500" style={{left:`${item.reference.selection.region.x*100}%`,top:`${item.reference.selection.region.y*100}%`,width:`${item.reference.selection.region.width*100}%`,height:`${item.reference.selection.region.height*100}%`}} />}</div>}
      {onNote ? <input aria-label={`批注意见 ${item.label ?? item.id}`} placeholder="补充此处的修改要求…" value={item.note ?? ''} onChange={(e) => onNote(item.id, e.target.value)} className="mt-1 w-52 max-w-full bg-transparent text-fg outline-none" /> : item.note && <p className="whitespace-pre-wrap text-fg-2">{item.note}</p>}
    </div>)}
  </div>;
}
