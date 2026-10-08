import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { EditorAnnotation } from '@forge/protocol';
import Composer from '@/components/chat/Composer';
import { annotationDraftKey, decodeAnnotationDrop, EDITOR_REFERENCE_MIME, useEditorAnnotationStore } from '@/lib/editorReferences';
import { useChatStore } from '@/lib/chatStore';
import { useSessionStore } from '@/lib/sessionStore';
import { useWorkspaceStore } from '@/lib/workspaceStore';
import { useEditorStore } from '@/lib/editorStore';
import { useAssetStore } from '@/lib/assetStore';
import { editorEventCursor, notifyAgentToolSettled } from '@/lib/editorSync';
import { publishEditorSelection } from '@/lib/editorSelection';
import { mockForgeBackend } from './forgeMock';
import { useCollaborationStore } from '@/lib/collaborationStore';

const initialChat=useChatStore.getState(), initialSession=useSessionStore.getState(), initialEditor=useEditorStore.getState(), initialAssets=useAssetStore.getState();
const annotation=(id='a',entityId=3):EditorAnnotation=>({id,label:'Hero',reference:{workspaceId:'ws',kind:'entity',sceneGuid:'scene',entityGuid:`hero${entityId}`,entityId,revision:8,hostEpoch:'epoch',targetMode:'edit'},note:'颜色调整'});
beforeEach(()=>{
  useChatStore.setState(initialChat,true);useChatStore.getState().reset();useSessionStore.setState(initialSession,true);
  useEditorStore.setState(initialEditor,true);useAssetStore.setState(initialAssets,true);
  useSessionStore.setState({activeSessionId:'session'});useWorkspaceStore.setState({activeWorkspaceId:'ws'});
  localStorage.setItem('forge:activeWorkspace','ws');
  useEditorAnnotationStore.setState({drafts:{},reveal:null});
  useChatStore.setState({models:[{id:'mock',label:'Mock',provider:'mock',availability:'available'}],selectedModelId:'mock'});
  vi.stubGlobal('fetch',mockForgeBackend({}, {'/api/forge/skills/list':{skills:[]}}));
});
afterEach(()=>{cleanup();vi.useRealTimers();vi.unstubAllGlobals();vi.restoreAllMocks();});

describe('durable editor annotations',()=>{
  it('deduplicates identities but retains changed notes during acknowledgement and keeps conversation drafts separate',()=>{
    const key=annotationDraftKey('session','ws');const st=useEditorAnnotationStore.getState();
    st.add([annotation()],key);st.add([annotation('duplicate')],key);expect(useEditorAnnotationStore.getState().drafts[key]).toHaveLength(1);
    st.update('a','发送中补充',key);st.acknowledge([annotation()],key);expect(useEditorAnnotationStore.getState().drafts[key][0].note).toBe('发送中补充');
    st.add([annotation('other',4)],annotationDraftKey('other','ws'));expect(useEditorAnnotationStore.getState().drafts[key]).toHaveLength(1);
  });
  it('rejects malformed drag payloads and preserves precise local selections',()=>{
    const item={...annotation(),reference:{...annotation().reference,selection:{component:'Light',property:'color'}}};
    expect(decodeAnnotationDrop({getData:()=>JSON.stringify([item])})).toEqual([item]);
    expect(decodeAnnotationDrop({getData:()=>'{'})).toEqual([]);
    expect(decodeAnnotationDrop({getData:()=>JSON.stringify([{id:'x',reference:{workspaceId:'ws',kind:'shell'}}])})).toEqual([]);
  });
  it('dragging into Composer preserves text, sends a structured payload, and retains the draft on rejection',async()=>{
    const send=vi.fn(async()=>false);useChatStore.setState({sendMessage:send});render(<Composer/>);
    fireEvent.change(screen.getByTestId('composer-input'),{target:{value:'检查这个角色'}});
    fireEvent.drop(screen.getByTestId('composer'),{dataTransfer:{types:[EDITOR_REFERENCE_MIME],getData:()=>JSON.stringify([annotation()])}});
    expect(screen.getByLabelText('批注意见 Hero')).toHaveValue('颜色调整');
    fireEvent.click(screen.getByTestId('composer-send'));
    await waitFor(()=>expect(send).toHaveBeenCalledWith('检查这个角色','build',undefined,{annotations:[annotation()]}));
    expect(screen.getByTestId('composer-input')).toHaveValue('检查这个角色');expect(screen.getByTestId('editor-annotations')).toBeInTheDocument();
  });
  it('a successful submission clears only its original snapshot while newer input stays',async()=>{
    let release!:(value:boolean)=>void;useChatStore.setState({sendMessage:vi.fn(()=>new Promise<boolean>((resolve)=>{release=resolve;}))});render(<Composer/>);
    fireEvent.change(screen.getByTestId('composer-input'),{target:{value:'原始要求'}});fireEvent.click(screen.getByTestId('composer-send'));
    fireEvent.change(screen.getByTestId('composer-input'),{target:{value:'正在补充'}});await act(async()=>release(true));
    expect(screen.getByTestId('composer-input')).toHaveValue('正在补充');
  });
  it('steering identity changes when references change, even when the text is identical',async()=>{
    const steer=vi.fn<ReturnType<typeof useChatStore.getState>['steerAgent']>(async()=>false);useChatStore.setState({activeRunId:'run',steerAgent:steer});useCollaborationStore.setState({supported:true});render(<Composer/>);
    fireEvent.change(screen.getByTestId('composer-input'),{target:{value:'改成红色'}});
    const key=annotationDraftKey('session','ws');act(()=>useEditorAnnotationStore.getState().add([annotation()],key));fireEvent.click(screen.getByTestId('composer-send'));
    await waitFor(()=>expect(steer).toHaveBeenCalledTimes(1));await act(async()=>Promise.resolve());
    act(()=>{useEditorAnnotationStore.getState().remove('a',key);useEditorAnnotationStore.getState().add([annotation('b',4)],key);});fireEvent.click(screen.getByTestId('composer-send'));
    await waitFor(()=>expect(steer).toHaveBeenCalledTimes(2));expect(steer.mock.calls[0][2]).not.toBe(steer.mock.calls[1][2]);expect(steer.mock.calls[1][3]?.[0].reference.entityId).toBe(4);
  });
  it('retains annotations in historical user events and passes them through resend',async()=>{
    const item=annotation();useChatStore.getState().applyEvent({id:'event1',sessionId:'session',seq:1,type:'composer.user.message',ts:'2026-10-07T00:00:00Z',payload:{text:'修改',annotations:[item]}});
    expect(useChatStore.getState().messages[0].annotations).toEqual([item]);
    const send=vi.fn(async()=>true);useChatStore.setState({sendMessage:send,messages:[{id:'local-1',role:'user',text:'旧要求',annotations:[item],blocks:[],time:''}]});
    await useChatStore.getState().editAndResend('local-1','新要求');expect(send).toHaveBeenCalledWith('新要求','build',undefined,{annotations:[item]});
  });
});
describe('editor synchronization',()=>{
  it('resets its event cursor on a new bus epoch even when sequence numbers become smaller',()=>{
    const accept=editorEventCursor();expect(accept({epoch:'old',seq:90,type:'editor.changed'})).toBe('reset');
    expect(accept({epoch:'old',seq:91,type:'editor.changed'})).toBe('change');expect(accept({epoch:'old',seq:90})).toBe('ignore');
    expect(accept({epoch:'new',seq:1,type:'editor.reset'})).toBe('reset');expect(accept({epoch:'new',seq:2})).toBe('change');
    expect(accept({epoch:'new',seq:2,type:'editor.reset'})).toBe('reset');
  });
  it('publishes only the latest selection and drops a pending old-workspace selection',async()=>{
    vi.useFakeTimers();const fetchMock=vi.fn(async(_url: RequestInfo | URL, _init?: RequestInit)=>({ok:true,json:async()=>({})}));vi.stubGlobal('fetch',fetchMock);
    publishEditorSelection([annotation().reference]);publishEditorSelection([annotation('other',4).reference]);await vi.advanceTimersByTimeAsync(200);
    expect(fetchMock).toHaveBeenCalledTimes(1);expect(JSON.parse(fetchMock.mock.calls[0]?.[1]?.body as string).references[0].entityId).toBe(4);
    publishEditorSelection([annotation().reference]);localStorage.setItem('forge:activeWorkspace','other');await vi.advanceTimersByTimeAsync(200);expect(fetchMock).toHaveBeenCalledTimes(1);
  });
  it('unions scene and asset invalidation in a mixed tool burst',()=>{
    vi.useFakeTimers();const entities=vi.fn(async()=>{}),summary=vi.fn(async()=>{}),assets=vi.fn(async()=>{});
    useEditorStore.setState({loadEntities:entities,refreshSummary:summary});useAssetStore.setState({load:assets});
    notifyAgentToolSettled('mcp__engine-scene__entity_create');notifyAgentToolSettled('mcp__asset-pipeline__asset_import');vi.advanceTimersByTime(300);
    expect(entities).toHaveBeenCalledOnce();expect(summary).toHaveBeenCalledOnce();expect(assets).toHaveBeenCalledOnce();
  });
  it('same-count content revision changes refresh actual entity properties',async()=>{
    const fresh={id:1,name:'Updated',transform:{translation:[2,0,0],rotation:[0,0,0,1],scale:[1,1,1]},components:[]};
    useEditorStore.setState({entities:[{...fresh,name:'Old'}],contentRevision:4,sceneGuid:'scene',hostEpoch:'epoch'});
    const mock=mockForgeBackend({scene_summary:{name:'Main',entityCount:1,playState:'edit',render:{},sceneGuid:'scene',hostEpoch:'epoch',contentRevision:5},entity_list:{entities:[fresh]}});vi.stubGlobal('fetch',mock);
    await useEditorStore.getState().refreshSummary();expect(useEditorStore.getState().entities[0].name).toBe('Updated');
  });
});
