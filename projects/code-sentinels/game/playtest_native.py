"""Exercise compiled native game inside an isolated real engine-host.

Captures concrete state and GPU output; assertions never read browser state.
"""
import json,time,pathlib,base64
from engine_client import Mcp,ROOT
OUT=ROOT/'game/native';report=[]
def check(name,condition,actual):
    report.append({'name':name,'pass':bool(condition),'actual':actual})
    print(name,'PASS' if condition else 'FAIL',str(actual)[:500],flush=True)
    if not condition:raise AssertionError(name)
def state(m):
    entities=m.call('entity_list')['entities'];return {e['name']:e for e in entities}
def transform(s,name):return s[name]['transform']
def command(m,value):m.call('logic_inject_input',{'action':'cs','value':float(value)});m.call('play_step')
def main():
    code=Mcp('code-forge-mcp')
    try:
        for p in sorted((ROOT/'Content/Graphs').glob('*.rxgraph')):
            result=code.call('graph_validate',{'graph':json.loads(p.read_text())})
            check('validate_'+p.stem,result.get('ok'),result)
    finally:code.close()
    m=Mcp()
    try:
        print('loading scene',flush=True)
        loaded=m.call('scene_load',{'path':'Content/Scenes/Main.rxscene'})
        check('mode',loaded.get('mode')=='2d',loaded)
        m.call('scene_checkpoint',{'name':'native-regression-start'})
        m.call('play_enter');m.call('play_pause');m.call('play_step')
        s=state(m);t=transform(s,'CS_State')
        check('new_game_ready',abs(t['translation'][0]-420)<.01 and t['translation'][1:]==[20,0] and t['scale'][0]==0,t)
        command(m,1021);s=state(m);t=transform(s,'CS_State');cell=transform(s,'CS_Cell2')
        check('paid_deployment',abs(t['translation'][0]-340)<.01 and cell['translation'][0]==1 and transform(s,'CS_Tower_2_1')['translation'][0]>-10,{'state':t,'cell':cell})
        command(m,1022);s=state(m)
        check('atomic_occupied',abs(transform(s,'CS_State')['translation'][0]-340)<.01 and transform(s,'CS_Cell2')['translation'][0]==1,transform(s,'CS_State'))
        command(m,1061);command(m,1101);command(m,5000)
        for _ in range(1200):m.call('play_step')
        s=state(m);enemies=[e for n,e in s.items() if n.startswith('CS_Enemy') and e['transform']['translation'][0]>-20]
        check('wave_motion',len(enemies)>0 and transform(s,'CS_State')['translation'][2]==1,{'enemies':len(enemies),'state':transform(s,'CS_State'),'firstEnemy':enemies[0]['transform'] if enemies else None})
        events=m.call('host_events_drain');json.dump(events,(OUT/'native-events.json').open('w'),indent=2)
        ev=events.get('events',[]) if isinstance(events,dict) else events
        bad=[e for e in ev if e.get('event') in ['logic.call_error','logic.unsupported','anim.warn']]
        check('no_runtime_errors',not bad,bad)
        frame=m.call('viewport_frame',{'width':1280,'height':720})
        json.dump({k:v for k,v in frame.items() if 'base64' not in k.lower() and k not in ['image','png','pixelsB64']},(OUT/'frame-metadata.json').open('w'),indent=2)
        data=frame.get('pngBase64') or frame.get('data') or frame.get('base64')
        if data:(OUT/'native-combat.png').write_bytes(base64.b64decode(data))
        elif frame.get('pixelsB64'):
            from PIL import Image
            Image.frombytes('RGBA',(frame['width'],frame['height']),base64.b64decode(frame['pixelsB64'])).save(OUT/'native-combat.png')
        check('native_gpu_frame',frame.get('nonZeroPixels',0)>0 and frame.get('meshFallbacks')==0,{'draws':frame.get('draws'),'device':frame.get('deviceName'),'meshFallbacks':frame.get('meshFallbacks')})
        check('native_combat_kills',transform(s,'CS_State')['scale'][1]>0,transform(s,'CS_State'))
        print('frame',str(frame)[:400],flush=True)
        json.dump(s,(OUT/'native-state.json').open('w'),indent=2)
        m.call('play_exit')
    finally:
        m.close();json.dump(report,(OUT/'native-regression.json').open('w'),indent=2)
if __name__=='__main__':main()
