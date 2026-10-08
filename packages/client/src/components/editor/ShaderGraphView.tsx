import { useEffect, useRef, useState } from 'react';
import type { ShaderDomain, ShaderNode, ShaderValueSource, ShaderValueType } from '@forge/protocol';
import { useShaderGraphStore, SHADER_NODE_INPUTS, shaderNodeOutputs, shaderOutputs, shaderCompileSignature } from '@/lib/shaderGraphStore';
import { useWorkspaceStore } from '@/lib/workspaceStore';
import { useEditorStore } from '@/lib/editorStore';
import { useAssetStore } from '@/lib/assetStore';
import { useCanvasViewport } from '@/lib/useCanvasViewport';
import { editorReference, entityReference, useEditorAnnotationStore } from '@/lib/editorReferences';
import CodeEditor from '../workbench/CodeEditor';
import CanvasHud from './CanvasHud';
import AnnotationHandle from './AnnotationHandle';
import { shaderGeneratedSource, shaderNodeAtLine } from '@/lib/shaderSource';

const button = 'rounded border border-edge px-2 py-1 text-xs hover:bg-shell-hover disabled:opacity-40';
const field = 'min-w-0 rounded border border-edge bg-shell-input px-2 py-1 text-xs text-fg outline-none focus:border-acc';
const NODE_W = 206;
const sourceLabel = (source?: ShaderValueSource): string => source ? 'node' in source ? `${source.node.slice(0,8)}.${source.pin}` : 'param' in source ? `参数 ${source.param}` : JSON.stringify(source.const) : '默认';
function JsonField({ value, onChange, label }: { value: unknown; onChange: (next: unknown) => void; label: string }) {
  const [draft,setDraft]=useState(JSON.stringify(value ?? 0)); const [error,setError]=useState(false);
  useEffect(()=>setDraft(JSON.stringify(value ?? 0)),[value]);
  return <input aria-label={label} value={draft} title={error?'请输入有效 JSON 数字或向量':label} className={`${field} w-full ${error?'border-danger':''}`}
    onChange={(e)=>setDraft(e.target.value)} onBlur={()=>{try{const v=JSON.parse(draft);onChange(v);setError(false);}catch{setError(true);}}} onKeyDown={(e)=>{if(e.key==='Enter')e.currentTarget.blur();}}/>;
}

export default function ShaderGraphView() {
  const ws=useWorkspaceStore((s)=>s.activeWorkspaceId);
  return <ShaderGraphCanvas key={ws ?? 'default'} workspaceId={ws}/>;
}
function ShaderGraphCanvas({workspaceId}:{workspaceId:string|null}) {
  const state=useShaderGraphStore();
  const {graph,selected,change}=state;
  const vp=useCanvasViewport({storageKey:`forge:shader-view:${workspaceId}:${graph.id}`,panExclude:'[data-shader-node],button,input,select'});
  const [path,setPath]=useState(state.path);
  const [search,setSearch]=useState('');
  const [wire,setWire]=useState<{node:string;pin:string}|null>(null);
  const [drag,setDrag]=useState<{ids:string[];start:[number,number];positions:Record<string,[number,number]>;dx:number;dy:number}|null>(null);
  const [marquee,setMarquee]=useState<{x:number;y:number;w:number;h:number}|null>(null);
  const marqueeStart=useRef<[number,number]|null>(null);
  const [source,setSource]=useState<'wgsl'|'godot'|null>(null);
  const [sourceLine,setSourceLine]=useState<{line:number;token:number}|null>(null);
  const [shape,setShape]=useState<'sphere'|'plane'>('sphere');
  const [slot,setSlot]=useState(0);
  const [parameterType,setParameterType]=useState<ShaderValueType>('float');
  const selectedEntity=useEditorStore((s)=>s.entities.find((e)=>e.id===s.selectedId));
  const reveal=useEditorAnnotationStore((s)=>s.reveal);
  const autoCompiled=useRef<string|null>(null);
  const compileSignature=shaderCompileSignature(graph);
  const assets=useAssetStore((s)=>s.items);
  useEffect(()=>{state.bindWorkspace(workspaceId);},[workspaceId,state.bindWorkspace]);
  useEffect(()=>setPath(state.path),[state.path]);
  useEffect(()=>{
    if(!workspaceId || state.busy || autoCompiled.current===compileSignature)return;
    const timer=setTimeout(()=>{if(!useShaderGraphStore.getState().busy){autoCompiled.current=compileSignature;void useShaderGraphStore.getState().compile();}},500);
    return()=>clearTimeout(timer);
  },[workspaceId,compileSignature,state.busy]);
  useEffect(()=>{
    if(reveal?.reference.kind!=='shaderGraph')return;
    const ref=reveal.reference;
    if(ref.path && ref.path!==state.path && !state.dirty){void state.load(ref.path);return;}
    const ids=ref.selection?.nodeIds ?? []; useShaderGraphStore.setState({selected:ids});
    const nodes=graph.nodes.filter((n)=>ids.includes(n.id));
    if(nodes.length)vp.fitTo(nodes.map((n)=>({x:n.pos[0],y:n.pos[1],w:NODE_W,h:160})));
  },[reveal,graph.id,state.path,state.dirty,state.load,vp.fitTo]);
  const shaderRef=(nodeIds?:string[])=>editorReference('shaderGraph',{resourceId:graph.id,path:state.path || undefined,revision:state.sourceHash ?? undefined,selection:nodeIds?.length?{nodeIds}:undefined});
  const position=(node:ShaderNode):[number,number]=>drag?.ids.includes(node.id)?[node.pos[0]+drag.dx,node.pos[1]+drag.dy]:node.pos;
  const nodeHeight=(node:ShaderNode)=>76+(SHADER_NODE_INPUTS[node.type]?.length??0)*27;
  const finishDrag=()=>{if(drag&&(drag.dx!==0||drag.dy!==0)){change((g)=>({...g,nodes:g.nodes.map((n)=>drag.ids.includes(n.id)?{...n,pos:[n.pos[0]+drag.dx,n.pos[1]+drag.dy]}:n)}));}setDrag(null);};
  const connect=(node:string,pin:string)=>{if(wire){state.connect(wire,node,pin);setWire(null);}};
  const removeInput=(node:string,pin:string)=>change((g)=>{
    if(node==='@output'){delete g.outputs[pin];return g;}
    const n=g.nodes.find((n)=>n.id===node);if(n)delete n.inputs[pin];return g;
  });
  const patchOption=(node:string,key:string,value:unknown)=>change((g)=>({...g,nodes:g.nodes.map((n)=>n.id===node?{...n,options:{...n.options,[key]:value}}:n)}));
  const port=(node:string,pin:string,src?:ShaderValueSource)=><div key={pin} className="flex h-[27px] items-center gap-1 border-t border-edge px-2 text-[10px]">
    <button data-shader-input={`${node}:${pin}`} aria-label={`连接到 ${node}.${pin}`} onClick={()=>connect(node,pin)} onPointerUp={()=>connect(node,pin)} className={`h-3 w-3 shrink-0 rounded-full border ${wire?'border-acc bg-acc/30':'border-fg-4'}`}/>
    <span className="w-16 shrink-0">{pin}</span><span className="min-w-0 flex-1 truncate text-fg-4" title={sourceLabel(src)}>{sourceLabel(src)}</span>
    {src&&<button title="断开输入" onClick={()=>removeInput(node,pin)}>×</button>}
  </div>;
  const selectedNode=graph.nodes.find((n)=>n.id===selected[0]);
  const outputX=Math.max(620,...graph.nodes.map((n)=>n.pos[0]+NODE_W+110));
  const compiled=state.compilation?.compiled ?? state.compilation;
  const generated=shaderGeneratedSource(state.compilation, graph.domain, source ?? 'wgsl');
  const revealNode=(id:string|undefined, revealSource=false)=>{
    const node=graph.nodes.find((n)=>n.id===id);if(!node)return;
    useShaderGraphStore.setState({selected:[node.id]});vp.reveal({x:node.pos[0],y:node.pos[1],w:NODE_W,h:nodeHeight(node)});
    if(revealSource){const span=generated.spans.find((span)=>span.nodeId===id);if(span)setSourceLine({line:span.line,token:Date.now()});}
  };
  return <div className="flex min-h-0 flex-1 flex-col bg-shell-sunk text-fg" data-testid="shader-graph-view">
    <div className="flex flex-wrap items-center gap-2 border-b border-edge bg-shell-panel px-2 py-1.5">
      <strong className="text-xs">Shader Graph</strong>
      <input aria-label="Shader 名称" value={graph.name} className={`${field} w-36`} onChange={(e)=>change((g)=>({...g,name:e.target.value}))}/>
      <select aria-label="Shader 域" value={graph.domain} className={field} onChange={(e)=>change((g)=>({...g,domain:e.target.value as ShaderDomain,outputs:Object.fromEntries(Object.entries(g.outputs).filter(([k])=>shaderOutputs(e.target.value as ShaderDomain).includes(k)))}))}>
        <option value="sprite2d">2D 精灵</option><option value="pbr3d">3D PBR</option><option value="unlit3d">3D Unlit</option>
      </select>
      <button className={button} onClick={()=>state.create(graph.domain)}>新建</button>
      <button className={button} disabled={!state.past.length} onClick={state.undo}>撤销</button><button className={button} disabled={!state.future.length} onClick={state.redo}>重做</button>
      <button className={button} disabled={state.busy} onClick={()=>void state.save()}>保存{state.dirty?' ·':''}</button>
      <button className={button} disabled={state.busy} onClick={()=>{autoCompiled.current=compileSignature;void state.compile();}}>验证与编译</button>
      <AnnotationHandle reference={shaderRef(selected)} label={selected.length?`${graph.name} · ${selected.length} 个节点`:graph.name}/>
    </div>
    <div className="flex items-center gap-2 border-b border-edge px-2 py-1"><input aria-label="Shader 路径" className={`${field} flex-1`} value={path} onChange={(e)=>setPath(e.target.value)} placeholder="Content/Shaders/name.rxshadergraph"/><button className={button} disabled={state.busy||!path.trim()} onClick={()=>void state.load(path)}>打开</button>{state.busy&&<span className="text-xs text-fg-4">处理中…</span>}</div>
    {state.error&&<p role="alert" className="border-b border-edge px-3 py-2 text-xs text-danger">{state.error}{state.previewUrl?'（保留上一次有效预览）':''}</p>}
    <div className="flex min-h-0 flex-1">
      <aside className="flex w-36 shrink-0 flex-col gap-1 overflow-y-auto border-r border-edge p-2">
        <input aria-label="搜索 Shader 节点" placeholder="搜索节点…" className={field} value={search} onChange={(e)=>setSearch(e.target.value)}/>
        {Object.keys(SHADER_NODE_INPUTS).filter((name)=>name.toLowerCase().includes(search.toLowerCase())).map((type)=><button key={type} className="rounded px-2 py-1 text-left text-xs hover:bg-shell-hover" onClick={()=>state.add(type,[-vp.view.x/vp.view.k+60,-vp.view.y/vp.view.k+60])}>+ {type}</button>)}
      </aside>
      <div ref={vp.ref} tabIndex={0} data-testid="shader-canvas" className="relative min-h-0 min-w-0 flex-1 overflow-hidden outline-none" style={vp.gridStyle}
        onKeyDown={(e)=>{if((e.target as HTMLElement).closest('input,textarea,select'))return;if(e.key==='Delete'){e.preventDefault();state.removeSelected();}if(e.key==='Escape'){setWire(null);setMarquee(null);setDrag(null);}if((e.ctrlKey||e.metaKey)&&e.key==='d'){e.preventDefault();state.duplicate();}if((e.ctrlKey||e.metaKey)&&e.key==='z'){e.preventDefault();if(e.shiftKey)state.redo();else state.undo();}}}
        onPointerDown={(e)=>{if((e.target as HTMLElement).closest('[data-shader-node]'))return;if(e.shiftKey&&e.button===0){e.currentTarget.setPointerCapture(e.pointerId);const [x,y]=vp.toWorld(e);marqueeStart.current=[x,y];setMarquee({x,y,w:0,h:0});}else vp.onPointerDown(e);}}
        onPointerMove={(e)=>{if(marqueeStart.current){const [x,y]=vp.toWorld(e);const [sx,sy]=marqueeStart.current;setMarquee({x:Math.min(x,sx),y:Math.min(y,sy),w:Math.abs(x-sx),h:Math.abs(y-sy)});}if(drag){const [x,y]=vp.toWorld(e);setDrag({...drag,dx:x-drag.start[0],dy:y-drag.start[1]});}}}
        onPointerUp={()=>{if(marquee){useShaderGraphStore.setState({selected:graph.nodes.filter((n)=>n.pos[0]+NODE_W>=marquee.x&&n.pos[0]<=marquee.x+marquee.w&&n.pos[1]+nodeHeight(n)>=marquee.y&&n.pos[1]<=marquee.y+marquee.h).map((n)=>n.id)});setMarquee(null);marqueeStart.current=null;}finishDrag();}}>
        <div className="absolute left-0 top-0 h-0 w-0" style={vp.worldStyle}>
          <svg width="1" height="1" className="pointer-events-none absolute overflow-visible">
            {[...graph.nodes.flatMap((n)=>Object.entries(n.inputs).map(([pin,src])=>({node:n.id,pin,src}))),...Object.entries(graph.outputs).map(([pin,src])=>({node:'@output',pin,src}))].filter((edge)=>'node'in edge.src).map((edge)=>{
              if(!('node'in edge.src))return null;const sourceNode=edge.src.node;const from=graph.nodes.find((n)=>n.id===sourceNode);const to=graph.nodes.find((n)=>n.id===edge.node);if(!from||(!to&&edge.node!=='@output'))return null;
              const [fx,fy]=position(from);const [tx,ty]=to?position(to):[outputX,100];const pins=to?SHADER_NODE_INPUTS[to.type]??[]:shaderOutputs(graph.domain);const y=ty+50+Math.max(0,pins.indexOf(edge.pin))*27;
              return <path key={`${edge.node}:${edge.pin}`} d={`M ${fx+NODE_W} ${fy+40} C ${fx+NODE_W+50} ${fy+40},${tx-50} ${y},${tx} ${y}`} stroke="var(--accent)" fill="none" strokeWidth="2"/>;
            })}
          </svg>
          {graph.nodes.map((node)=>{const [x,y]=position(node);return <div key={node.id} data-shader-node={node.id} className={`absolute rounded-md border bg-shell-panel shadow-composer ${selected.includes(node.id)?'border-acc ring-1 ring-acc':'border-edge-strong'}`} style={{left:x,top:y,width:NODE_W}}
            onPointerDown={(e)=>{if(e.button!==0||(e.target as HTMLElement).closest('button,input,select'))return;e.stopPropagation();e.currentTarget.setPointerCapture(e.pointerId);const ids=e.ctrlKey||e.metaKey?(selected.includes(node.id)?selected.filter((id)=>id!==node.id):[...selected,node.id]):selected.includes(node.id)?selected:[node.id];useShaderGraphStore.setState({selected:ids});setDrag({ids,start:vp.toWorld(e),positions:{},dx:0,dy:0});}}>
            <div className="flex h-9 items-center gap-1 border-b border-edge px-2"><span className="flex-1 text-xs font-medium">{node.type}</span><AnnotationHandle reference={shaderRef([node.id])} label={`${graph.name} · ${node.type}`}/></div>
            <div className="flex h-7 justify-end gap-2 px-2">{shaderNodeOutputs(node).map((pin)=><button key={pin} title="拖到或点击目标输入端口" aria-label={`输出 ${node.id}.${pin}`} className={`text-[10px] ${wire?.node===node.id?'text-acc':'text-fg-3'}`} onPointerDown={(e)=>{e.stopPropagation();setWire({node:node.id,pin});}} onClick={()=>setWire({node:node.id,pin})}>{pin} ●</button>)}</div>
            {(SHADER_NODE_INPUTS[node.type]??[]).map((pin)=>port(node.id,pin,node.inputs[pin]))}
          </div>;})}
          <div data-shader-node="@output" className="absolute rounded-md border border-acc bg-shell-panel shadow-composer" style={{left:outputX,top:100,width:NODE_W}}><h3 className="flex h-9 items-center px-2 text-xs font-semibold">{graph.domain} 输出</h3>{shaderOutputs(graph.domain).map((pin)=>port('@output',pin,graph.outputs[pin]))}</div>
          {marquee&&<div className="pointer-events-none absolute border border-acc bg-blue-500/10" style={{left:marquee.x,top:marquee.y,width:marquee.w,height:marquee.h}}/>}
        </div>
        <div className="absolute left-3 bottom-3 text-[10px] text-fg-4">{wire?'选择目标输入端口 · Esc 取消':'Shift 拖动框选 · Ctrl 点击多选 · Ctrl+D 复制 · Delete 删除'}</div>
        <CanvasHud vp={vp} prefix="shader" onFit={()=>vp.fitTo([...graph.nodes.map((n)=>({x:n.pos[0],y:n.pos[1],w:NODE_W,h:nodeHeight(n)})),{x:outputX,y:100,w:NODE_W,h:240}])}/>
      </div>
      <aside className="flex w-64 shrink-0 flex-col gap-3 overflow-y-auto border-l border-edge bg-shell-panel p-3 text-xs">
        {selectedNode&&<section className="space-y-2"><strong>{selectedNode.type}</strong><p className="break-all text-[10px] text-fg-4">{selectedNode.id}</p>
          {['constant','color'].includes(selectedNode.type)&&<JsonField label="节点值" value={selectedNode.options.value} onChange={(value)=>patchOption(selectedNode.id,'value',value)}/>}
          {selectedNode.type==='parameter'&&<select aria-label="参数节点引用" className={`${field} w-full`} value={String(selectedNode.options.parameter??'')} onChange={(e)=>patchOption(selectedNode.id,'parameter',e.target.value)}><option value="">选择参数</option>{graph.parameters.map((p)=><option key={p.id} value={p.id}>{p.name} ({p.type})</option>)}</select>}
          {selectedNode.type==='texture'&&<><select aria-label="纹理色彩空间" className={`${field} w-full`} value={String(selectedNode.options.colorSpace??'srgb')} onChange={(e)=>patchOption(selectedNode.id,'colorSpace',e.target.value)}><option value="srgb">sRGB</option><option value="linear">Linear</option></select><select aria-label="采样纹理参数" className={`${field} w-full`} value={'param'in(selectedNode.inputs.texture??{})?(selectedNode.inputs.texture as {param:string}).param:''} onChange={(e)=>state.connect({param:e.target.value},selectedNode.id,'texture')}><option value="">选择纹理参数</option>{graph.parameters.filter((p)=>p.type==='texture2d').map((p)=><option key={p.id} value={p.id}>{p.name}</option>)}</select></>}
          {(SHADER_NODE_INPUTS[selectedNode.type]??[]).filter((pin)=>pin!=='texture').map((pin)=><label key={pin} className="block space-y-1"><span>{pin} 常量</span><JsonField label={`${pin} 常量`} value={'const'in(selectedNode.inputs[pin]??{})?(selectedNode.inputs[pin] as {const:unknown}).const:0} onChange={(value)=>state.connect({const:value},selectedNode.id,pin)}/></label>)}
          <div className="flex gap-2"><button className={button} onClick={state.duplicate}>复制选中</button><button className={button} onClick={state.removeSelected}>删除选中</button></div>
        </section>}
        <section className="space-y-2"><strong>材质参数</strong>{graph.parameters.map((p)=><div key={p.id} className="space-y-1 rounded border border-edge p-2"><div className="flex items-center gap-1"><input aria-label="参数名称" className={`${field} w-full`} value={p.name} onChange={(e)=>change((g)=>({...g,parameters:g.parameters.map((v)=>v.id===p.id?{...v,name:e.target.value}:v)}))}/><button onClick={()=>change((g)=>({...g,parameters:g.parameters.filter((v)=>v.id!==p.id)}))} title="删除参数">×</button></div><span className="text-fg-4">{p.type}</span>{p.type==='float'&&<input aria-label={`${p.name} 滑杆`} type="range" min="0" max="1" step="0.01" value={Number(p.default)||0} className="w-full" onPointerDown={state.beginGesture} onPointerUp={state.endGesture} onPointerCancel={state.endGesture} onKeyDown={state.beginGesture} onKeyUp={state.endGesture} onChange={(e)=>change((g)=>({...g,parameters:g.parameters.map((v)=>v.id===p.id?{...v,default:Number(e.target.value)}:v)}))}/>}
          {p.type==='texture2d'?<select aria-label={`${p.name} 纹理资产`} className={`${field} w-full`} value={String(p.default??'')} onChange={(e)=>change((g)=>({...g,parameters:g.parameters.map((v)=>v.id===p.id?{...v,default:e.target.value}:v)}))}><option value="">选择纹理</option>{assets.filter((a)=>a.type==='texture').map((a)=><option key={a.guid} value={a.guid}>{a.path}</option>)}</select>:<JsonField label={`${p.name} 默认值`} value={p.default} onChange={(value)=>change((g)=>({...g,parameters:g.parameters.map((v)=>v.id===p.id?{...v,default:value}:v)}))}/>}</div>)}
          <div className="flex gap-1"><select aria-label="新参数类型" className={field} value={parameterType} onChange={(e)=>setParameterType(e.target.value as ShaderValueType)}>{['float','vec2','vec3','vec4','color','texture2d'].map((type)=><option key={type}>{type}</option>)}</select><button className={button} onClick={()=>change((g)=>({...g,parameters:[...g.parameters,{id:crypto.randomUUID(),name:`Parameter${g.parameters.length+1}`,type:parameterType,default:parameterType==='float'?0:parameterType==='texture2d'?'':Array(parameterType==='vec2'?2:parameterType==='vec3'?3:4).fill(1)}]}))}>添加</button></div>
        </section>
        <section className="space-y-2"><strong>预览与材质绑定</strong><div className="flex gap-1"><select className={field} aria-label="预览几何体" value={shape} onChange={(e)=>setShape(e.target.value as 'sphere'|'plane')}><option value="sphere">球体</option><option value="plane">平面</option></select><button className={button} disabled={state.busy} onClick={()=>void state.preview(shape)}>预览</button></div>
          {state.previewUrl&&<img src={state.previewUrl} alt="真实 Shader 预览" className="w-full rounded border border-edge"/>}
          <p className="text-fg-4">{selectedEntity?`绑定目标：${selectedEntity.name}`:'请在层级选择需要绑定的实体'}</p>
          <label className="flex items-center gap-2">材质槽<input aria-label="材质槽" type="number" min="0" value={slot} className={`${field} w-16`} onChange={(e)=>setSlot(Math.max(0,Number(e.target.value)||0))}/></label>
          <button className={`${button} w-full`} disabled={!selectedEntity||state.busy} onClick={()=>selectedEntity&&void state.bind(entityReference(selectedEntity),slot)}>应用到选中实体（可撤销）</button>
          <div className="flex gap-2"><button className={button} disabled={!compiled} onClick={()=>setSource(source==='wgsl'?null:'wgsl')}>WGSL</button><button className={button} disabled={!compiled} onClick={()=>setSource(source==='godot'?null:'godot')}>Godot</button></div>
          {state.compilation?.validation&&<p className="text-[10px] text-fg-4">{Object.entries(state.compilation.validation).map(([k,v])=>`${k}: ${v}`).join(' · ')}</p>}
        </section>
      </aside>
    </div>
    {state.diagnostics.length>0&&<div className="max-h-32 shrink-0 overflow-auto border-t border-edge p-2">{state.diagnostics.map((d,i)=><button key={i} className="block text-left text-xs text-danger" onClick={()=>revealNode(d.nodeId,true)}>{d.nodeId ? `${d.nodeId}.${d.pin??''} · `:''}{d.message}</button>)}</div>}
    {source&&<div className="h-48 shrink-0 border-t border-edge"><CodeEditor key={`${source}:${compiled?.hash}`} path={source==='wgsl'?'generated.wgsl':'generated.gdshader'} initialDoc={generated.code} readOnly className="h-full" revealLine={sourceLine} onSelectionChanged={(range)=>revealNode(shaderNodeAtLine(generated.spans,range.startLine))}/></div>}
  </div>;
}
