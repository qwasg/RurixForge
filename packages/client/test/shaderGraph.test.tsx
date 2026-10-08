import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, afterEach, describe, expect, it, vi } from 'vitest';
import ShaderGraphView from '@/components/editor/ShaderGraphView';
import { createsShaderCycle, newShaderGraph, useShaderGraphStore } from '@/lib/shaderGraphStore';
import { useWorkspaceStore } from '@/lib/workspaceStore';
import { useEditorStore } from '@/lib/editorStore';
import { shaderGeneratedSource, shaderNodeAtLine } from '@/lib/shaderSource';
const initial=useShaderGraphStore.getState();
beforeEach(()=>{localStorage.clear();useShaderGraphStore.setState({...initial,workspaceId:'ws',graph:newShaderGraph(),selected:[],past:[],future:[]},true);useWorkspaceStore.setState({activeWorkspaceId:'ws'});});
afterEach(()=>{cleanup();vi.unstubAllGlobals();});
describe('editable Shader Graph',()=>{
  it('does not recompile layout or name edits, but recompiles changed constants and output connections',async()=>{
    vi.useFakeTimers();
    try {
      const fetchMock=vi.fn(async()=>({ok:true,json:async()=>({ok:true,compiled:{hash:'shader'}})}));vi.stubGlobal('fetch',fetchMock);
      render(<ShaderGraphView/>);
      await act(async()=>{await vi.advanceTimersByTimeAsync(550);});expect(fetchMock).toHaveBeenCalledTimes(1);
      act(()=>useShaderGraphStore.getState().change((g)=>({...g,name:'New label',nodes:g.nodes.map((n)=>({...n,pos:[500,300]}))})));
      await act(async()=>{await vi.advanceTimersByTimeAsync(550);});expect(fetchMock).toHaveBeenCalledTimes(1);
      act(()=>useShaderGraphStore.getState().change((g)=>({...g,nodes:g.nodes.map((n)=>({...n,options:{value:[0,1,0,1]}}))})));
      await act(async()=>{await vi.advanceTimersByTimeAsync(550);});expect(fetchMock).toHaveBeenCalledTimes(2);
      act(()=>useShaderGraphStore.getState().connect({const:[1,0,0,1]},'@output','color'));
      await act(async()=>{await vi.advanceTimersByTimeAsync(550);});expect(fetchMock).toHaveBeenCalledTimes(3);
      expect(useShaderGraphStore.getState().dirty).toBe(true);expect(useShaderGraphStore.getState().past).toHaveLength(3);
    } finally { cleanup(); vi.useRealTimers(); }
  });
  it('selects the domain source without shifting source-map lines or attributing helper code to a node',()=>{
    const compilation={compiled:{domain:'sprite2d' as const,wgsl:{sprite:'sprite source',model:'model source'},godot:{canvas:'canvas source',spatial:'spatial source'},sourceMap:{'wgsl.sprite':[{nodeId:'color1',line:12}]}}};
    const generated=shaderGeneratedSource(compilation,'sprite2d','wgsl');
    expect(generated.code).toBe('sprite source');expect(shaderNodeAtLine(generated.spans,13)).toBe('color1');
    expect(shaderNodeAtLine(generated.spans,14)).toBeUndefined();expect(shaderGeneratedSource(compilation,'sprite2d','godot').code).toBe('canvas source');
    expect(shaderGeneratedSource({...compilation,compiled:{...compilation.compiled,domain:'pbr3d'}},'pbr3d','wgsl').code).toBe('model source');
  });
  it('binds the canonical saved graph and source hash, but never binds if the user edits during save',async()=>{
    const canonical={...useShaderGraphStore.getState().graph,name:'Canonical'};
    const requests:Array<{url:string;body:Record<string,unknown>}>=[];
    vi.spyOn(useEditorStore.getState(),'loadEntities').mockResolvedValue();
    vi.stubGlobal('fetch',vi.fn(async(url,init)=>{const body=JSON.parse(init?.body??'{}');requests.push({url:String(url),body});return {ok:true,json:async()=>String(url).endsWith('/save')?{graph:canonical,path:'Shaders/canonical.rxshadergraph',sourceHash:'saved-hash'}:{}};}));
    await useShaderGraphStore.getState().bind({workspaceId:'ws',kind:'entity',entityGuid:'stable'},0);
    expect(requests.find((r)=>r.url.endsWith('/bind'))?.body.arguments).toMatchObject({graph:canonical,sourceHash:'saved-hash'});
    requests.length=0;
    vi.stubGlobal('fetch',vi.fn(async(url,init)=>{const body=JSON.parse(init?.body??'{}');requests.push({url:String(url),body});if(String(url).endsWith('/save'))useShaderGraphStore.getState().change((g)=>({...g,name:'Concurrent local edit'}));return {ok:true,json:async()=>({graph:canonical,path:'Shaders/canonical.rxshadergraph',sourceHash:'next-hash'})};}));
    await useShaderGraphStore.getState().bind({workspaceId:'ws',kind:'entity',entityGuid:'stable'},0);
    expect(requests.some((r)=>r.url.endsWith('/bind'))).toBe(false);expect(useShaderGraphStore.getState().graph.name).toBe('Concurrent local edit');
    vi.restoreAllMocks();
  });
  it('coalesces a parameter slider gesture into one undo step',()=>{
    const st=useShaderGraphStore.getState();const old=st.graph;st.beginGesture();
    for(let n=0;n<10;n++)st.change((g)=>({...g,name:`value${n}`}));st.endGesture();
    expect(useShaderGraphStore.getState().past).toHaveLength(1);st.undo();expect(useShaderGraphStore.getState().graph).toEqual(old);
  });
  it('ignores compilation of an older graph without leaving the editor busy',async()=>{
    let release!:(value:unknown)=>void;vi.stubGlobal('fetch',vi.fn(()=>new Promise((resolve)=>{release=resolve;})));
    const compiling=useShaderGraphStore.getState().compile();useShaderGraphStore.getState().add('time',[100,200]);
    release({ok:true,json:async()=>({ok:true,compiled:{hash:'obsolete'}})});expect(await compiling).toBe(false);
    expect(useShaderGraphStore.getState().busy).toBe(false);expect(useShaderGraphStore.getState().compilation).toBeNull();
  });
  it('rejects cyclic connections without adding undo history, and undo restores a real graph edit',()=>{
    const st=useShaderGraphStore.getState();st.add('add',[300,100]);const id=useShaderGraphStore.getState().selected[0];st.connect({node:'color1',pin:'out'},id,'a');
    const g=useShaderGraphStore.getState().graph;expect(createsShaderCycle(g,id,'color1')).toBe(true);
    const before=useShaderGraphStore.getState().past.length;st.connect({node:id,pin:'out'},'color1','value');expect(useShaderGraphStore.getState().past).toHaveLength(before);
    st.undo();expect(useShaderGraphStore.getState().graph.nodes.find((n)=>n.id===id)?.inputs.a).toBeUndefined();st.redo();expect(useShaderGraphStore.getState().graph.nodes.find((n)=>n.id===id)?.inputs.a).toEqual({node:'color1',pin:'out'});
  });
  it('compiles through the host and saves with the exact previous source hash',async()=>{
    const requests:Array<{url:string;body:Record<string,unknown>}>=[];
    vi.stubGlobal('fetch',vi.fn(async(url,init)=>{const body=JSON.parse(init?.body??'{}');requests.push({url:String(url),body});return {ok:true,json:async()=>String(url).endsWith('/compile')?{ok:true,compiled:{hash:'compiled',wgsl:{sprite:'real code'}}}:{path:'Content/Shaders/Test.rxshadergraph',sourceHash:'new'}};}));
    useShaderGraphStore.setState({path:'Content/Shaders/Test.rxshadergraph',sourceHash:'old'});
    expect(await useShaderGraphStore.getState().compile()).toBe(true);expect(useShaderGraphStore.getState().compilation?.compiled?.hash).toBe('compiled');
    expect(await useShaderGraphStore.getState().save()).toBe(true);expect(requests.find((r)=>r.url.endsWith('/save'))?.body.arguments).toMatchObject({path:'Content/Shaders/Test.rxshadergraph',expectedHash:'old'});
  });
  it('UI adds editable nodes and invokes real compilation, preserving diagnostics',async()=>{
    const fetchMock=vi.fn(async()=>({ok:true,json:async()=>({ok:false,diagnostics:[{nodeId:'color1',message:'type mismatch',pin:'out'}]})}));vi.stubGlobal('fetch',fetchMock);
    render(<ShaderGraphView/>);fireEvent.click(screen.getByText('+ multiply'));expect(useShaderGraphStore.getState().graph.nodes.some((n)=>n.type==='multiply')).toBe(true);
    fireEvent.click(screen.getByText('验证与编译'));await screen.findByText(/type mismatch/);expect(fetchMock).toHaveBeenCalled();
    fireEvent.click(screen.getByText(/type mismatch/));expect(useShaderGraphStore.getState().selected).toEqual(['color1']);
  });
});
