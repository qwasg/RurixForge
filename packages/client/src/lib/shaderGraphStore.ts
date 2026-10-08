import { create } from 'zustand';
import type { EditorReference, ShaderCompilation, ShaderDiagnostic, ShaderDomain, ShaderGraphDoc, ShaderNode, ShaderValueSource } from '@forge/protocol';
import { shaderAction } from './editorApi';
import { useAssetStore } from './assetStore';
import { useEditorStore } from './editorStore';

export const SHADER_NODE_INPUTS: Record<string, string[]> = {
  constant: [], color: [], parameter: [], uv: [], time: [], texture: ['texture','uv'],
  add: ['a','b'], subtract: ['a','b'], multiply: ['a','b'], divide: ['a','b'], mix: ['a','b','t'], clamp: ['value','min','max'],
  sin: ['value'], cos: ['value'], abs: ['value'], oneMinus: ['value'], normalize: ['value'], dot: ['a','b'], split: ['value'], combine: ['x','y','z','w'], normalMap: ['color','strength'],
  fract: ['value'], uvTransform: ['uv','tiling','offset'],
};
export const shaderOutputs = (domain: ShaderDomain) => domain === 'pbr3d' ? ['baseColor','metallic','roughness','emission','normal','alpha'] : ['color','alpha'];
export const shaderNodeOutputs = (node: ShaderNode) => node.type === 'split' ? ['x','y','z','w'] : ['out'];
/** Layout and the document label do not change emitted shader semantics. */
export function shaderCompileSignature(graph: ShaderGraphDoc): string {
  return JSON.stringify({ version: graph.version, id: graph.id, domain: graph.domain, parameters: graph.parameters,
    nodes: graph.nodes.map(({ id, type, inputs, options }) => ({ id, type, inputs, options })), outputs: graph.outputs });
}
export function newShaderGraph(domain: ShaderDomain = 'sprite2d'): ShaderGraphDoc {
  return { version:1, id:crypto.randomUUID(), name:'NewShader', domain, parameters:[], nodes:[{id:'color1',type:'color',pos:[80,100],inputs:{},options:{value:[1,1,1,1]}}], outputs:{[domain === 'pbr3d' ? 'baseColor' : 'color']:{node:'color1',pin:'out'}} };
}
export function createsShaderCycle(graph: ShaderGraphDoc, from: string, to: string): boolean {
  if (from === to) return true;
  const seen = new Set<string>();
  const walk = (id: string): boolean => {
    if (id === to) return true;
    if (seen.has(id)) return false; seen.add(id);
    return Object.values(graph.nodes.find((n) => n.id === id)?.inputs ?? {}).some((src) => 'node' in src && walk(src.node));
  };
  return walk(from);
}
interface ShaderState {
  workspaceId: string | null; path: string; graph: ShaderGraphDoc; selected: string[]; dirty: boolean; busy: boolean;
  diagnostics: ShaderDiagnostic[]; compilation: ShaderCompilation | null; error: string | null; previewUrl: string | null; previewHash: string | null;
  past: ShaderGraphDoc[]; future: ShaderGraphDoc[]; sourceHash: string | null;
  gesture: ShaderGraphDoc | null;
  editSequence: number;
  beginGesture: () => void;
  endGesture: () => void;
  bindWorkspace: (id: string | null) => void;
  change: (fn: (graph: ShaderGraphDoc) => ShaderGraphDoc) => void;
  create: (domain: ShaderDomain) => void;
  load: (path: string) => Promise<void>;
  save: () => Promise<boolean>;
  compile: () => Promise<boolean>;
  preview: (shape: 'sphere' | 'plane') => Promise<void>;
  bind: (reference: EditorReference, slot: number) => Promise<void>;
  add: (type: string, pos: [number,number]) => void;
  connect: (source: ShaderValueSource, target: string, pin: string) => void;
  removeSelected: () => void;
  duplicate: () => void;
  undo: () => void; redo: () => void;
}
const draftKey = (id: string | null) => `forge:shader-draft:${id ?? 'default'}`;
export const useShaderGraphStore = create<ShaderState>((set, get) => {
  const keep = () => { try { localStorage.setItem(draftKey(get().workspaceId), JSON.stringify({graph:get().graph,path:get().path,dirty:get().dirty,sourceHash:get().sourceHash})); } catch { /* memory draft survives */ } };
  const fail = (error: unknown) => set({ error: (error as Error).message, busy:false });
  return {
    workspaceId:null,path:'',graph:newShaderGraph(),selected:[],dirty:false,busy:false,diagnostics:[],compilation:null,error:null,previewUrl:null,previewHash:null,past:[],future:[],sourceHash:null,gesture:null,editSequence:0,
    beginGesture: () => { if(!get().gesture)set({gesture:get().graph}); },
    endGesture: () => {const {gesture,graph}=get();if(gesture&&gesture!==graph)set({past:[...get().past.slice(-99),gesture],future:[],gesture:null});else set({gesture:null});keep();},
    bindWorkspace: (id) => {
      if (get().workspaceId === id) return; keep();
      let draft: {graph:ShaderGraphDoc;path:string;dirty:boolean;sourceHash?:string} | null = null;
      try { draft=JSON.parse(localStorage.getItem(draftKey(id)) ?? 'null'); } catch { /* ignore broken local JSON */ }
      set({workspaceId:id,graph:draft?.graph ?? newShaderGraph(),path:draft?.path ?? '',dirty:draft?.dirty ?? false,sourceHash:draft?.sourceHash ?? null,gesture:null,selected:[],past:[],future:[],compilation:null,diagnostics:[],previewUrl:null,error:null,busy:false});
    },
    change: (fn) => { const old=get().graph, next=fn(structuredClone(old)); if(JSON.stringify(old)===JSON.stringify(next))return; set({graph:next,editSequence:get().editSequence+1,past:get().gesture?get().past:[...get().past.slice(-99),old],future:[],dirty:true,error:null}); keep(); },
    create: (domain) => { get().change(() => newShaderGraph(domain)); set({path:'',sourceHash:null,selected:[],compilation:null,previewUrl:null,diagnostics:[]}); keep(); },
    load: async (path) => {
      const {workspaceId:ws,editSequence}=get(); if (!ws || !path.trim()) return; set({busy:true,error:null});
      try { const result=await shaderAction<{graph:ShaderGraphDoc;path?:string;sourceHash?:string}>(ws,'get',{path}); if (get().workspaceId !== ws) return;
        if(get().editSequence !== editSequence){set({busy:false,error:'加载期间图已更新，本地修改已保留'});return;}
        set({graph:result.graph,path:result.path ?? path,sourceHash:result.sourceHash ?? null,dirty:false,past:[],future:[],selected:[],diagnostics:[],compilation:null,busy:false}); keep();
      } catch(error) { if(get().workspaceId === ws) fail(error); }
    },
    compile: async () => {
      const {workspaceId:ws,graph}=get(); if(!ws) {set({error:'请先打开工作区'});return false;} set({busy:true,error:null});
      const signature=shaderCompileSignature(graph);
      try { const result=await shaderAction<ShaderCompilation>(ws,'compile',{graph}); if(get().workspaceId !== ws) return false; if(shaderCompileSignature(get().graph) !== signature) { set({busy:false}); return false; }
        set({compilation:result.compiled || result.wgsl ? result : get().compilation,diagnostics:result.diagnostics ?? [],busy:false}); return result.ok !== false && !(result.diagnostics ?? []).some((d)=>d.severity !== 'warning');
      } catch(error) { if(get().workspaceId === ws) fail(error); return false; }
    },
    save: async () => {
      const {workspaceId:ws,graph,path}=get(); if(!ws) return false; set({busy:true,error:null});
      try { const result=await shaderAction<{path?:string;sourceHash?:string;graph?:ShaderGraphDoc;diagnostics?:ShaderDiagnostic[];ok?:boolean}>(ws,'save',{graph,path:path || `Content/Shaders/${graph.name.replace(/[^\w\u4e00-\u9fff-]/g,'_')}.rxshadergraph`,...(get().sourceHash?{expectedHash:get().sourceHash}:{})}); if(get().workspaceId !== ws) return false;
        if(result.ok === false) { set({diagnostics:result.diagnostics ?? [],busy:false}); return false; }
        if(get().graph.id!==graph.id){set({busy:false});return false;}
        const edited=get().graph!==graph; set({graph:edited?get().graph:result.graph??graph,path:result.path ?? path,sourceHash:result.sourceHash ?? null,dirty:edited,busy:false}); keep(); void useAssetStore.getState().load(); return true;
      } catch(error) {if(get().workspaceId === ws) fail(error);return false;}
    },
    preview: async (shape) => {
      const {workspaceId:ws,graph}=get(); if(!ws) return; set({busy:true,error:null});
      try { const result=await shaderAction<{imageUrl?:string;hash?:string}>(ws,'preview',{graph,shape}); if(get().workspaceId !== ws) return; if(get().graph !== graph) { set({busy:false}); return; }
        if(!result.imageUrl) throw new Error('预览未返回真实图像'); set({previewUrl:result.imageUrl,previewHash:result.hash ?? null,busy:false});
      } catch(error) {if(get().workspaceId === ws) fail(error);}
    },
    bind: async (reference,slot) => {
      const {workspaceId:ws,editSequence}=get(); if(!ws || ws !== reference.workspaceId) return;
      if(!await get().save()) return;
      if(get().workspaceId !== ws)return;
      if(get().editSequence !== editSequence){set({error:'保存期间图已更新，请重新应用'});return;}
      set({busy:true,error:null});
      try { await shaderAction(ws,'bind',{graph:get().graph,path:get().path,sourceHash:get().sourceHash,reference,materialSlot:slot}); if(get().workspaceId !== ws) return; set({busy:false}); await useEditorStore.getState().loadEntities(); }
      catch(error){if(get().workspaceId === ws) fail(error);}
    },
    add: (type,pos) => {
      const id=crypto.randomUUID(); get().change((g)=>({...g,nodes:[...g.nodes,{id,type,pos,inputs:{},options:type==='constant'?{value:0}:type==='color'?{value:[1,1,1,1]}:{}}]}));set({selected:[id]});
    },
    connect: (source,target,pin) => {
      if('node' in source && target !== '@output' && createsShaderCycle(get().graph,source.node,target)){set({error:'连接会形成循环，未修改节点图'});return;}
      get().change((g)=>target==='@output'?{...g,outputs:{...g.outputs,[pin]:source}}:{...g,nodes:g.nodes.map((n)=>n.id===target?{...n,inputs:{...n.inputs,[pin]:source}}:n)});
    },
    removeSelected: () => { const ids=new Set(get().selected); const filter=(inputs:Record<string,ShaderValueSource>)=>Object.fromEntries(Object.entries(inputs).filter(([,v])=>!('node'in v)||!ids.has(v.node)));
      get().change((g)=>({...g,nodes:g.nodes.filter((n)=>!ids.has(n.id)).map((n)=>({...n,inputs:filter(n.inputs)})),outputs:filter(g.outputs)}));set({selected:[]}); },
    duplicate: () => {const ids=new Map(get().selected.map((id)=>[id,crypto.randomUUID()]));get().change((g)=>({...g,nodes:[...g.nodes,...g.nodes.filter((n)=>ids.has(n.id)).map((n)=>({...n,id:ids.get(n.id)!,pos:[n.pos[0]+30,n.pos[1]+30] as [number,number],inputs:Object.fromEntries(Object.entries(n.inputs).map(([pin,s])=>[pin,'node'in s&&ids.has(s.node)?{...s,node:ids.get(s.node)!}:s]))}))]}));set({selected:[...ids.values()]});},
    undo:()=>{const {past,graph,future}=get();if(!past.length)return;set({graph:past[past.length-1],editSequence:get().editSequence+1,past:past.slice(0,-1),future:[graph,...future],dirty:true});keep();},
    redo:()=>{const {past,graph,future}=get();if(!future.length)return;set({graph:future[0],editSequence:get().editSequence+1,past:[...past,graph],future:future.slice(1),dirty:true});keep();},
  };
});
