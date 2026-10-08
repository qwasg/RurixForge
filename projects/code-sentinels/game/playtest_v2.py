"""Full V2 campaigns driven by ordinary UI commands on a fresh native host.
No balances, cooldowns, enemy state or terrain are edited by this test.
"""
import sys,pathlib,os,json,time,math,heapq,hashlib,shutil,base64
from engine_client import Mcp,ROOT,REPO
OUT=ROOT/'game/v2';OUT.mkdir(parents=True,exist_ok=True)
W,H,BASE=24,14,169
COST=[0,65,90,110,100];RATE=[1,.72,1.32,1.24,1.05];ENERGY=[0,3,6,5,4];RANGE=[0,3.8,3.4,4.5,4.1]

def fresh():
    binary=REPO/'target/debug/engine-host.exe';sha=hashlib.sha256(binary.read_bytes()).hexdigest()[:12]
    private=OUT/f'host-{sha}.exe'
    if not private.exists():shutil.copy2(binary,private)
    return Mcp(extra_env={'FORGE_ENGINE_HOST_BIN':str(private),'FORGE_GPU_PARTICLES':'on','FORGE_GAME_SAVE_DIR':str(OUT/'isolated-save')})
def state(m):return {e['name']:e['transform'] for e in m.call('entity_list')['entities']}
def command(m,code):m.call('logic_inject_input',{'action':'cs','value':float(code)});m.call('play_step')
def decode(s):
    tiles=[]
    for r in range(H):
        row=s[f'CS_MapRow{r}'];parts=row['translation']+row['scale'][:1]
        for packed in parts:tiles.extend([(round(packed)>>(c*3))&7 for c in range(6)])
    units=[s[f'CS_Unit{i}']['translation'] for i in range(24)]
    return tiles,units
def neighbors(c):
    r,x=divmod(c,W)
    return [n for n in [c-1 if x else -1,c-W if r else -1,c+1 if x+1<W else -1,c+W if r+1<H else -1]if n>=0]
def distances(tiles,blocked):
    dist=[1e9]*336;dist[BASE]=0;queue=[(0,BASE)]
    while queue:
        cost,c=heapq.heappop(queue)
        if cost>dist[c]:continue
        weight={3:.76,7:.76,4:1.3,6:1.6}.get(tiles[c],1)
        for n in neighbors(c):
            if tiles[n]in[1,2]or n in blocked:continue
            value=cost+weight
            if value<dist[n]:dist[n]=value;heapq.heappush(queue,(value,n))
    return dist
def xy(c):return c%W-11.5,6.5-c//W
def length(a,b):return math.hypot(a[0]-b[0],a[1]-b[1])
def line_of_sight(tiles,c,t):
    if tiles[c]==4:return True
    a=xy(c);b=xy(t);steps=math.ceil(length(a,b)*5)
    for n in range(1,steps):
        x=a[0]+(b[0]-a[0])*n/steps;y=a[1]+(b[1]-a[1])*n/steps
        at=math.floor(7-y)*W+math.floor(x+12)
        if 0<=at<336 and tiles[at]==1:return False
    return True
def candidate(s,kind):
    tiles,units=decode(s);blocked={round(u[2])for u in units if u[0]>0};dist=distances(tiles,blocked);level=round(s['CS_State']['scale'][2])
    spawn={1:[95,263,16],2:[11,167,327],3:[71,287,9,320]}[level]
    paths=[]
    for source in spawn:
        c=source
        for _ in range(336):
            paths.append(c)
            if c==BASE:break
            n=min(neighbors(c),key=lambda n:dist[n])
            if dist[n]>=dist[c]:break
            c=n
    ranked=[]
    for c in range(336):
        if c%W<4 or c%W>10 or tiles[c]not in[0,3,4,7]or c in blocked:continue
        r=RANGE[kind]+(1.25 if tiles[c]==4 else 0)
        score=sum(length(xy(c),xy(p))<r and line_of_sight(tiles,c,p) for p in paths)
        score-=4*sum(length(xy(c),xy(p))<1.5 for p in blocked);score-=c%W*.35;ranked.append((score,c))
    for _,c in sorted(ranked,reverse=True):
        simulated=distances(tiles,blocked|{c})
        if all(simulated[p]<1e9 for p in spawn):return c
    return None
def invest(m,s):
    money=s['CS_Economy']['translation'][0];power=s['CS_Economy']['translation'][1]
    units=[s[f'CS_Unit{i}']['translation']for i in range(24)]
    demand=sum(ENERGY[round(u[0])]/RATE[round(u[0])]for u in units if u[0]>0)
    gpu=[s[f'CS_GPU{i}']for i in range(8)]
    if not any(g['translation'][0]>0 for g in gpu):command(m,4_000_001);return
    if power<demand*1.45+5:
        for i,g in enumerate(gpu):
            if g['translation'][0]==0 and money>=130:command(m,4_000_000+i*10+1);money-=130;break
            cost=g['scale'][1]
            if g['translation'][0]>0 and g['translation'][1]<3 and money>=cost:command(m,4_100_000+i);money-=cost;break
    count=sum(u[0]>0 for u in units);kind=[1,1,3,2,4,1,2,3,4,1,2,3][count%12]
    if count<18 and money>=COST[kind]+(130 if power<demand*1.45+5 else 0):
        c=candidate(s,kind)
        if c is not None:command(m,1_000_000+kind*1000+c);money-=COST[kind]
    if power>demand*1.2:
        for i,u in enumerate(units):
            price=s[f'CS_UnitCost{i}']['translation'][0]
            if u[0]>0 and u[1]<3 and money>price+100:command(m,2_000_000+i);break
def capture(m,name):
    from PIL import Image
    frame=m.call('viewport_frame',{'width':1280,'height':720})
    Image.frombytes('RGBA',(frame['width'],frame['height']),base64.b64decode(frame['pixelsB64'])).save(OUT/name)
    return {k:v for k,v in frame.items() if k!='pixelsB64'}
def run(defeat=False):
    result={'mode':'defeat'if defeat else'campaign','timeline':[]};m=fresh();started=time.time()
    try:
        result['host']=m.call('host_ping');print('fresh host',result['host'],flush=True)
        result['load']=m.call('scene_load',{'path':'Content/Scenes/Main.rxscene'});m.call('play_enter');m.call('play_pause');m.call('play_step')
        s=state(m);assert s['CS_State']['translation']==[0,20,0];assert s['CS_Economy']['translation'][0]==560;assert s['CS_Economy']['scale'][2]==0
        command(m,5_000_000);assert state(m)['CS_Meta']['translation'][0]==30
        command(m,4_000_001);command(m,7_000_000);command(m,7_000_000)
        last=None;complete=0;mutations=[]
        for batch in range(1200):
            s=state(m);main=s['CS_State'];phase=round(main['scale'][0]);level=round(main['scale'][2]);wave=round(main['translation'][2]);stage=round(s['CS_Campaign']['scale'][0]);key=(level,wave,phase,stage)
            if key!=last:
                record={'level':level,'wave':wave,'phase':phase,'terrainStage':stage,'hp':main['translation'][1],'energy':main['translation'][0],'credits':s['CS_Economy']['translation'][0],'gpuPower':s['CS_Economy']['translation'][1],'units':sum(s[f'CS_Unit{i}']['translation'][0]>0 for i in range(24)),'elapsed':time.time()-started}
                result['timeline'].append(record);print(record,flush=True);last=key
                if stage>0:mutations.append(record)
                if phase==2:result[f'level{level}Frame']=capture(m,f'campaign-level{level}.png')
            if phase==3:
                assert defeat,f'Campaign lost: {key}, {main}';result['defeatFrame']=capture(m,'v2-defeat.png');result['pass']=True;break
            if phase==2:
                complete+=1;assert stage==2
                if level==3:result['pass']=not defeat;break
                command(m,5_200_000);continue
            if defeat:
                if phase==0:command(m,5_000_000)
            else:
                invest(m,s);s=state(m);units=sum(s[f'CS_Unit{i}']['translation'][0]>0 for i in range(24))
                if phase==0 and units>=2 and s['CS_State']['translation'][0]>s['CS_Economy']['translation'][2]*.7:command(m,5_000_000)
                if phase==1:
                    tiles,_=decode(s);blocked={round(s[f'CS_Unit{i}']['translation'][2])for i in range(24)if s[f'CS_Unit{i}']['translation'][0]>0};dist=distances(tiles,blocked)
                    threat=[]
                    for name,e in s.items():
                        if name.startswith('CS_Enemy')and e['translation'][0]>-50:
                            x,y=e['translation'][:2];c=math.floor(7-y)*24+math.floor(x+12)
                            if 0<=c<336 and (dist[c]<8 or e['scale'][0]>1.4):threat.append((dist[c],c))
                    if threat:
                        target=min(threat)[1]
                        for i in range(24):
                            u=s[f'CS_Unit{i}'];energy=s['CS_State']['translation'][0]
                            if u['translation'][0]>0 and u['scale'][1]<=0 and energy>=u['scale'][2]+30:command(m,3_000_000+i*1000+target);break
            for _ in range(60):m.call('play_step')
        result['completeLevels']=complete;result['mutations']=mutations;result['final']=state(m)['CS_State'];result['seconds']=time.time()-started
        events=m.call('host_events_drain');events=events.get('events',[])if isinstance(events,dict)else events
        result['errors']=[e for e in events if e.get('event')in['logic.call_error','logic.unsupported','anim.warn']]
        assert result.get('pass') and not result['errors'],result.get('final')
        m.call('play_exit')
    finally:
        m.close();(OUT/('native-defeat.json'if defeat else'native-campaign.json')).write_text(json.dumps(result,ensure_ascii=False,indent=2),encoding='utf8')
        print('FINAL',result.get('pass'),result.get('final'),'elapsed',time.time()-started,flush=True)
if __name__=='__main__':run('--defeat'in sys.argv)
