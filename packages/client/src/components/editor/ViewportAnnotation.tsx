import { useEffect, useState } from 'react';
import { createPortal } from 'react-dom';
import type { EditorObservation, EditorSelection } from '@forge/protocol';
import { captureEditorReference } from '@/lib/editorApi';
import { activeAnnotationDraftKey, editorReference, makeAnnotation, useEditorAnnotationStore } from '@/lib/editorReferences';
import { useToastStore } from '@/lib/toastStore';

export default function ViewportAnnotation() {
  const [capture, setCapture] = useState<(EditorObservation & { key: string }) | null>(null);
  const [busy, setBusy] = useState(false);
  const [region, setRegion] = useState<NonNullable<EditorSelection['region']> | null>(null);
  const [origin, setOrigin] = useState<[number, number] | null>(null);
  useEffect(() => { useEditorAnnotationStore.setState({ viewportOverlayOpen: capture !== null }); return () => useEditorAnnotationStore.setState({ viewportOverlayOpen: false }); }, [capture]);
  const take = async () => {
    const reference = editorReference('viewport');
    const key = activeAnnotationDraftKey();
    setBusy(true);
    try {
      const result = await captureEditorReference(reference);
      if (!result.observationId) throw new Error('场景观测未返回有效截图标识');
      setRegion(null); setCapture({ ...result, reference: result.reference ?? reference, key });
    } catch (error) { useToastStore.getState().push('error', (error as Error).message); }
    finally { setBusy(false); }
  };
  const add = () => {
    if (!capture?.reference) return;
    const selectedRegion = region && region.width > 0.002 && region.height > 0.002 ? region : null;
    const annotation = makeAnnotation({ ...capture.reference, selection: selectedRegion ? { region: selectedRegion } : undefined }, selectedRegion ? '场景画面区域' : '场景观测');
    annotation.observationId = capture.observationId;
    useEditorAnnotationStore.getState().add([annotation], capture.key); setCapture(null);
  };
  return <><button type="button" className="shrink-0 rounded px-2 py-0.5 hover:bg-shell-hover" disabled={busy} onClick={() => void take()}>{busy ? '捕获中…' : '画面批注'}</button>
    {capture && createPortal(<div role="dialog" aria-label="场景画面批注" className="fixed inset-0 z-[1000] flex items-center justify-center bg-black/70 p-8">
      <div className="flex max-h-full max-w-5xl flex-col gap-2 rounded-lg bg-shell-panel p-4 text-fg">
        <p className="text-sm">在捕获画面上拖动框选；未框选时引用整帧。</p>
        {capture.imageUrl ? <div className="relative min-h-0 select-none overflow-hidden" onPointerDown={(e) => {
          const box = e.currentTarget.getBoundingClientRect(); const x = (e.clientX-box.left)/box.width, y=(e.clientY-box.top)/box.height;
          e.currentTarget.setPointerCapture(e.pointerId); setOrigin([x,y]); setRegion({x,y,width:0,height:0});
        }} onPointerMove={(e) => { if (!origin) return; const box=e.currentTarget.getBoundingClientRect(); const x=Math.max(0,Math.min(1,(e.clientX-box.left)/box.width)),y=Math.max(0,Math.min(1,(e.clientY-box.top)/box.height)); setRegion({x:Math.min(x,origin[0]),y:Math.min(y,origin[1]),width:Math.abs(x-origin[0]),height:Math.abs(y-origin[1])}); }} onPointerUp={() => setOrigin(null)}>
          <img src={capture.imageUrl} alt="捕获的真实场景帧" draggable={false} className="max-h-[70vh] max-w-full object-contain" />
          {region && <div className="pointer-events-none absolute border-2 border-blue-500 bg-blue-500/10" style={{left:`${region.x*100}%`,top:`${region.y*100}%`,width:`${region.width*100}%`,height:`${region.height*100}%`}} />}
        </div> : <p className="text-warn">当前截图无法预览，可添加整帧观测。</p>}
        <div className="flex justify-end gap-3 text-sm"><button onClick={() => setCapture(null)}>取消</button><button onClick={add} className="rounded bg-acc px-3 py-1 text-white">添加批注</button></div>
      </div>
    </div>, document.body)}
  </>;
}
