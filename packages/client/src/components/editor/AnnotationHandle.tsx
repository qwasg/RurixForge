import { MessageSquarePlus } from 'lucide-react';
import type { EditorAnnotation, EditorReference } from '@forge/protocol';
import { EDITOR_REFERENCE_MIME, makeAnnotation, useEditorAnnotationStore } from '@/lib/editorReferences';

/** Separate grip keeps ordinary object movement and gizmos intact. Click/right-click are keyboard alternatives. */
export default function AnnotationHandle({ reference, label, annotations }: { reference?: EditorReference; label: string; annotations?: EditorAnnotation[] }) {
  const items = () => annotations ?? (reference ? [makeAnnotation(reference, label)] : []);
  const add = () => useEditorAnnotationStore.getState().add(items());
  return <button type="button" draggable data-no-drag data-no-pan title={`添加批注：${label}（可拖入对话）`} aria-label={`添加批注 ${label}`}
    onPointerDown={(e) => e.stopPropagation()} onClick={(e) => { e.stopPropagation(); add(); }}
    onContextMenu={(e) => { e.preventDefault(); e.stopPropagation(); add(); }}
    onDragStart={(e) => { e.stopPropagation(); e.dataTransfer.effectAllowed = 'copy'; e.dataTransfer.setData(EDITOR_REFERENCE_MIME, JSON.stringify(items())); }}
    className="inline-flex h-6 w-6 shrink-0 items-center justify-center rounded text-fg-3 hover:bg-acc-bg hover:text-acc"><MessageSquarePlus size={13} /></button>;
}
