"""Full eight-wave campaign and defeat on a fresh native engine PID.

Only ordinary cs commands and real engine play_step are used. Purchases use
the same balances and costs as the browser. No debug money or direct state edits.
"""
import json,time,base64
from engine_client import Mcp,ROOT
OUT=ROOT/'game/native'
def snapshot(m):return {e['name']:e['transform'] for e in m.call('entity_list')['entities'] if e['name'].startswith('CS_')}
def cmd(m,code):m.call('logic_inject_input',{'action':'cs','value':float(code)});m.call('play_step')
def run(defend=True):
    m=Mcp();evidence={'kind':'victory' if defend else 'defeat','states':[],'errors':[]}
    t0=time.time()
    try:
        evidence['host']=m.call('host_ping');print('fresh host',evidence['host'],flush=True)
        m.call('scene_load',{'path':'Content/Scenes/Main.rxscene'});m.call('play_enter');m.call('play_pause');m.call('play_step')
        cmd(m,7000);cmd(m,7000)
        if defend:
            for s in [2,6,10]:cmd(m,1000+s*10+1)
        last_wave=-1
        for batch in range(350):
            s=snapshot(m);main=s['CS_State'];aux=s['CS_StateAux'];money,hp,wave=main['translation'];phase=main['scale'][0]
            if wave!=last_wave:
                record={'wave':wave,'money':money,'hp':hp,'phase':phase,'elapsed':time.time()-t0,'cells':[s[f'CS_Cell{i}']['translation'][:2] for i in range(12)]}
                evidence['states'].append(record);last_wave=wave;print(record,flush=True)
            if phase>=2:break
            if defend:
                for lane in range(3):
                    for col,kind,cost in [(1,3,145),(3,2,115),(0,4,165)]:
                        cell=lane*4+col
                        if s[f'CS_Cell{cell}']['translation'][0]==0 and money>=cost:
                            cmd(m,1000+cell*10+kind);money-=cost
                s=snapshot(m);money=s['CS_State']['translation'][0]
                for cell in range(12):
                    kind,level,cost=s[f'CS_Cell{cell}']['translation']
                    if kind>0 and level<3 and money>=cost:cmd(m,2000+cell);money-=cost
                if aux['translation'][0]<=0:
                    threat=[(v['translation'][0],v['translation'][1]) for name,v in s.items() if name.startswith('CS_Enemy') and -10<v['translation'][0]<0]
                    if threat:
                        _,y=min(threat);lane=min(range(3),key=lambda i:abs(y-([2.6,0,-2.6][i]-.52)))
                        cmd(m,4000+lane)
            if phase==0:cmd(m,5000)
            for _ in range(120):m.call('play_step')
            events=m.call('host_events_drain');events=events.get('events',[]) if isinstance(events,dict) else events
            evidence['errors'] += [ev for ev in events if ev.get('event') in ['logic.call_error','logic.unsupported','anim.warn']]
        final=snapshot(m);evidence['final']=final['CS_State'];evidence['finalAux']=final['CS_StateAux'];evidence['seconds']=time.time()-t0
        expected=2 if defend else 3
        evidence['pass']=final['CS_State']['scale'][0]==expected and not evidence['errors']
        if defend:evidence['pass'] &= final['CS_State']['translation'][2]==8 and final['CS_State']['translation'][1]>0
        frame=m.call('viewport_frame',{'width':1280,'height':720})
        from PIL import Image
        Image.frombytes('RGBA',(frame['width'],frame['height']),base64.b64decode(frame['pixelsB64'])).save(OUT/f'campaign-{evidence["kind"]}.png')
        print('FINAL',evidence['pass'],evidence['final'],'wall seconds',evidence['seconds'],flush=True)
        m.call('play_exit')
    finally:
        m.close();(OUT/f'campaign-{evidence["kind"]}.json').write_text(json.dumps(evidence,indent=2),encoding='utf8')
    assert evidence['pass'],evidence['final']
if __name__=='__main__':
    import sys
    run('--defeat' not in sys.argv)
