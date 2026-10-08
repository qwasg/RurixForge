"""Prove video frames and combat cast transitions in native PIE and GPU output."""
import json,base64
from PIL import Image,ImageChops
from engine_client import Mcp,ROOT
OUT=ROOT/'game/native'
def command(m,c):m.call('logic_inject_input',{'action':'cs','value':float(c)});m.call('play_step')
def capture(m,name):
    frame=m.call('viewport_frame',{'width':1280,'height':720})
    assert frame['meshFallbacks']==0
    im=Image.frombytes('RGBA',(frame['width'],frame['height']),base64.b64decode(frame['pixelsB64']))
    im.save(OUT/name);return im
def main():
    m=Mcp();result={}
    try:
        result['host']=m.call('host_ping');m.call('scene_load',{'path':'Content/Scenes/Main.rxscene'})
        m.call('play_enter');m.call('play_pause');m.call('play_step')
        command(m,1023);command(m,1034)
        es=m.call('entity_list')['entities'];ids={e['name']:e['id'] for e in es}
        def pose(name):
            c=m.call('component_get',{'id':ids[name],'type':'Sprite'})
            return c.get('component',c).get('props',c)
        result['initial']={name:pose(name) for name in ['CS_Tower_2_3','CS_Tower_3_4']}
        a=capture(m,'native-animation-a.png')
        for _ in range(35):m.call('play_step')
        result['later']={name:pose(name) for name in result['initial']}
        b=capture(m,'native-animation-b.png')
        result['pixelDifference']=ImageChops.difference(a,b).convert('RGB').getbbox()
        assert result['pixelDifference'] is not None
        for name in result['initial']:assert result['initial'][name]['frame']!=result['later'][name]['frame']
        command(m,5000);seen={name:set() for name in result['initial']}
        for tick in range(1000):
            m.call('play_step')
            if tick%10==0:
                for name in seen:seen[name].add(pose(name)['clip'])
        result['clips']={k:sorted(v) for k,v in seen.items()}
        assert all({'idle','cast'} <= v for v in seen.values()),seen
        capture(m,'native-animation-combat.png')
        events=m.call('host_events_drain');events=events.get('events',[]) if isinstance(events,dict) else events
        result['errors']=[e for e in events if e.get('event') in ['logic.call_error','logic.unsupported','anim.warn']]
        assert not result['errors'];result['pass']=True;m.call('play_exit')
    finally:
        m.close();(OUT/'native-animation-test.json').write_text(json.dumps(result,indent=2),encoding='utf8');print(json.dumps(result,ensure_ascii=False),flush=True)
if __name__=='__main__':main()
