"""Build the actual Forge 2D scene, graph adapters and geometry textures.

This does not simulate the game. Native sentinels.rs is compiled by the engine.
Run again after the video atlas pipeline finishes to adopt the verified clips.
"""
import json, pathlib, uuid, shutil, math
from PIL import Image, ImageDraw, ImageFont

ROOT = pathlib.Path(__file__).resolve().parents[1]
CONTENT = ROOT / 'Content'
TEX = CONTENT / 'Textures'
SPR = CONTENT / 'Sprites'
GR = CONTENT / 'Graphs'
MODULE = 'Content/Scripts/sentinels.rs'
NS = uuid.UUID('8dd5d0b9-3c5c-4f1e-aef3-6e4f12571587')
LANES = [2.6, 0, -2.6]
COLS = [-6.3,-3.5,-.7,2.1]

def guid(path): return str(uuid.uuid5(NS, str(path.relative_to(ROOT)).replace('\\','/')))
def meta(path, kind, origin='project-source'):
    existing=path.with_suffix(path.suffix+'.meta')
    if existing.exists():
        try:
            return json.loads(existing.read_text(encoding='utf-8-sig'))['guid']
        except (ValueError,KeyError):pass
        for line in existing.read_text(encoding='utf-8-sig').splitlines():
            if line.startswith('guid:'):return line.split(':',1)[1].strip()
    path.with_suffix(path.suffix+'.meta').write_text(f'guid: {guid(path)}\ntype: {kind}\nimporter: {"png" if kind == "texture" else kind}\nprovenance:\n  origin: {origin}\nbuild_state: current\n',encoding='utf8')
    return guid(path)
def json_file(path, obj):
    path.parent.mkdir(parents=True,exist_ok=True);path.write_text(json.dumps(obj,ensure_ascii=False,indent=2),encoding='utf8')
def texture(name, img):
    p=TEX/name;img.save(p);return meta(p,'texture')
def sprite_doc(name, image_path, frames, clips, pivot=(.5,1)):
    tx=meta(image_path,'texture');p=SPR/(name+'.rxsprite')
    json_file(p,{'version':1,'texture':tx,'pivot':list(pivot),'frames':frames,'clips':clips})
    return meta(p,'sprite')
def single_sprite(name,path,pivot=(.5,1)):
    with Image.open(path) as im: w,h=im.size
    return sprite_doc(name,path,{'frame_0':{'bbox':[0,0,w,h]}},{'idle':{'frames':['frame_0'],'fps':1,'loop':True}},pivot),h
def world(x,y):return ((x+10.6666667)*100,(6-y)*100)

def geometry_assets():
    im=Image.new('RGBA',(2133,1200),(0,0,0,0));d=ImageDraw.Draw(im)
    colors=['#59ddd9','#73baff','#d4a5ff']
    for lane,ly in enumerate(LANES):
        pts=[]
        for n in range(161):
            x=-9.35+n*19.6/160;y=ly-.52+max(0,1-abs(x/2.7))*(-.28 if lane==1 else .28);pts.append(world(x,y))
        d.line(pts,fill='#101e35',width=78,joint='curve');d.line(pts,fill='#28445c',width=64,joint='curve')
        d.line(pts,fill='#172c42',width=58,joint='curve')
        for n in range(2,158,8):
            x,y=pts[n];d.polygon([(x+7,y-7),(x-3,y),(x+7,y+7),(x+3,y)],fill=colors[lane])
        for col,x in enumerate(COLS):
            cx,cy=world(x,ly+.08)
            d.line([(cx,cy+25),(cx,cy+54)],fill=colors[lane],width=5)
            d.rounded_rectangle((cx-73,cy-31,cx+73,cy+31),radius=18,fill='#122038',outline='#36677a',width=3)
            d.line([(cx-26,cy),(cx+26,cy)],fill='#2e6173',width=3);d.line([(cx,cy-15),(cx,cy+15)],fill='#2e6173',width=3)
            for dx in [-58,58]:d.rectangle((cx+dx-3,cy-12,cx+dx+3,cy+12),fill=colors[lane])
        for x,colr in [(-9.35,'#4ff7bc'),(10.02,'#ff678f')]:
            px,py=world(x,ly-.52);d.ellipse((px-38,py-53,px+38,py+53),fill='#13263d',outline=colr,width=5)
            d.ellipse((px-23,py-34,px+23,py+34),outline=colr,width=2)
    texture('circuit-lanes.png',im)
    # Bug packets are code-native geometric enemies with two movement frames.
    bug=Image.new('RGBA',(8*128,128),(0,0,0,0));bd=ImageDraw.Draw(bug)
    palettes=[('#ff688b','#551e44'),('#ffbb65','#58361e'),('#a492ff','#353255'),('#ff416b','#451a39')]
    for kind,(accent,body) in enumerate(palettes):
        for frame in range(2):
            ox=(kind*2+frame)*128;lift=frame*4
            cx=ox+64;cy=67-lift
            for side in [-1,1]:
                for yy in [-22,0,22]:bd.line([(cx+side*26,cy+yy),(cx+side*42,cy+yy+frame*7-3),(cx+side*50,cy+yy+11)],fill=accent,width=5)
            bd.rounded_rectangle((cx-34,cy-38,cx+34,cy+36),radius=14,fill=body,outline=accent,width=5)
            if kind==2:bd.rectangle((cx-41,cy-21,cx+41,cy+19),outline=accent,width=5)
            if kind==3:bd.polygon([(cx-30,cy-38),(cx-20,cy-54),(cx,cy-42),(cx+22,cy-54),(cx+32,cy-36)],fill=accent)
            bd.rectangle((cx-22,cy-13,cx-7,cy-4),fill='#e7f9ff');bd.rectangle((cx+7,cy-13,cx+22,cy-4),fill='#e7f9ff')
            bd.line([(cx-13,cy+18),(cx+13,cy+18)],fill=accent,width=4)
    p=TEX/'bug-packets.png';bug.save(p)
    bugs=sprite_doc('BugPackets',p,{f'frame_{i}':{'bbox':[i*128,0,128,128]} for i in range(8)},{'idle':{'frames':[f'frame_{i}' for i in range(8)],'fps':1,'loop':True}})
    bolts=Image.new('RGBA',(4*64,64),(0,0,0,0));bd=ImageDraw.Draw(bolts)
    for i,c in enumerate(['#72c6ff','#a7f279','#72e3ff','#d5b5ff']):
        x=i*64;bd.polygon([(x+6,32),(x+27,18),(x+54,25),(x+60,32),(x+54,39),(x+27,46)],fill=c)
        bd.line([(x+24,32),(x+53,32)],fill='white',width=5)
    p=TEX/'code-pulses.png';bolts.save(p)
    pulses=sprite_doc('CodePulses',p,{f'frame_{i}':{'bbox':[i*64,0,64,64]} for i in range(4)},{'idle':{'frames':[f'frame_{i}' for i in range(4)],'fps':1,'loop':True}},(.5,.5))
    beam=Image.new('RGBA',(2000,100),(0,0,0,0));bd=ImageDraw.Draw(beam)
    for y,w,c in [(50,20,'#79e6df'),(50,7,'#e3ffff')]:bd.line([(0,y),(2000,y)],fill=c,width=w)
    texture('compile-beam.png',beam)
    return bugs,pulses

def hero_sprite(id,concept):
    # The pipeline owns accepted .rxsprite files; never overwrite them.
    accepted=SPR/(id+'.rxsprite')
    if accepted.exists():
        doc=json.loads(accepted.read_text(encoding='utf-8-sig'))
        return meta(accepted,'sprite'),max(fr['bbox'][3] for fr in doc['frames'].values())
    path=CONTENT/'Concepts'/concept
    if path.exists():return single_sprite(id+'-preview',path)
    raise FileNotFoundError(f'Missing researched character artwork: {path}')

class Graph:
    def __init__(self,name):self.name=name;self.nodes=[];self.edges=[];self.props=[]
    def n(self,kind,**inputs):
        nid='n'+str(len(self.nodes));self.nodes.append({'id':nid,'type':kind,'pos':[0,0],'inputs':inputs});return nid
    def p(self,node,pin='result'):return {'node':node,'pin':pin}
    def c(self,value):return {'const':value}
    def r(self,name):return {'ref':name}
    def prop(self,name,value):self.props.append({'name':name,'kind':'F32','default':value})
    def link(self,a,b,pin='exec'):self.edges.append({'from':[a,pin],'to':[b,'exec']})
    def call(self,fn,args):return self.n('call.call_function',module=self.c(MODULE),fn=self.c(fn),args=args)
    def emit(self):
        p=GR/(self.name+'.rxgraph');json_file(p,{'version':1,'id':self.name,'name':self.name,'exposedProps':self.props,'nodes':self.nodes,'edges':self.edges});meta(p,'graph')

def graph_adapters():
    g=Graph('GameController');start=g.n('event.on_start');reset=g.call('cs_reset',g.c([]));g.link(start,reset)
    upd=g.n('event.on_update');tick=g.call('cs_tick',g.p(upd,'dt'));g.link(upd,tick)
    inp=g.n('event.on_input');save=g.n('var.set',name=g.p(inp,'action'),value=g.p(inp,'value'));g.link(inp,save)
    get=g.n('var.get',name=g.c('cs'));cmd=g.call('cs_input',g.p(get,'out'));g.link(save,cmd)
    clear=g.n('var.set',name=g.c('cs'),value=g.c(0.));g.link(cmd,clear);g.emit()
    for mode in ['Packet','Actor','Hero']:
        animated=mode!='Packet'
        g=Graph('Render'+mode);g.prop('visualId',0.);g.prop('baseScale',1.)
        upd=g.n('event.on_update');tail=upd;out=[]
        for fn in ['cs_visual_x','cs_visual_y','cs_visual_scale']:
            call=g.call(fn,g.r('visualId'));g.link(tail,call);tail=call;out.append(call)
        v=g.n('math.vec3',x=g.p(out[0]),y=g.p(out[1]),z=g.c(0.))
        # Sprite pixelsPerUnit fixes base dimensions; renderer scale is the gameplay pulse.
        s=g.n('math.vec3',x=g.p(out[2]),y=g.p(out[2]),z=g.c(1.))
        tr=g.n('transform.compose',translation=g.p(v,'out'),scale=g.p(s,'out'))
        move=g.n('entity.set_transform',entity=g.c('$self'),transform=g.p(tr,'out'));g.link(tail,move);tail=move
        if not animated:
            fr=g.call('cs_visual_frame',g.r('visualId'));g.link(tail,fr)
            frame=g.n('sprite.set_frame',entity=g.c('$self'),index=g.p(fr));g.link(fr,frame)
        if mode=='Hero':
            attack=g.call('cs_visual_attacking',g.r('visualId'));g.link(tail,attack)
            set_attack=g.n('animator.set_bool',entity=g.c('$self'),param=g.c('attacking'),value=g.p(attack));g.link(attack,set_attack)
        g.emit()
    g=Graph('PublishState');up=g.n('event.on_update');tail=up;reads=[]
    for name in ['x','y','z','sx','sy','sz']:
        g.prop(name,0.);n=g.call('cs_get',g.r(name));g.link(tail,n);tail=n;reads.append(n)
    trv=g.n('math.vec3',**{k:g.p(n) for k,n in zip(['x','y','z'],reads[:3])})
    scv=g.n('math.vec3',**{k:g.p(n) for k,n in zip(['x','y','z'],reads[3:])})
    tr=g.n('transform.compose',translation=g.p(trv,'out'),scale=g.p(scv,'out'))
    n=g.n('entity.set_transform',entity=g.c('$self'),transform=g.p(tr,'out'));g.link(tail,n);g.emit()

def scene_build():
    for p in [TEX,SPR,GR]:p.mkdir(parents=True,exist_ok=True)
    bugs,pulses=geometry_assets();graph_adapters()
    icon=TEX/'vscode.png';shutil.copy2(ROOT/'references/software/vscode-app.png',icon);vsc,h=single_sprite('VSCode',icon,(.5,.5))
    # PyCharm is an official icon; render SVG through cairosvg if the PNG is absent.
    py=ROOT/'references/software/pycharm.png'
    if not py.exists():
        try:
            import cairosvg
            cairosvg.svg2png(url=str(ROOT/'references/software/pycharm.svg'),write_to=str(py),output_width=256,output_height=256)
        except Exception:
            raise RuntimeError('Official PyCharm SVG needs PNG conversion before build (provide references/software/pycharm.png).')
    local_py=TEX/'pycharm.png';shutil.copy2(py,local_py)
    pyc,ph=single_sprite('PyCharm',local_py,(.5,.5))
    ds,dh=hero_sprite('deepseek','deepseek-maid.png');gp,gh=hero_sprite('gpt','gpt-dragon.png')
    sprites=[(vsc,h/1.1),(pyc,ph/1.1),(ds,dh/1.8),(gp,gh/1.8)]
    es=[]
    def ent(name,x=0.,y=0.,components=None,scale=(1.,1.,1.),z=0.):
        eid=len(es)+1;es.append({'id':eid,'name':name,'transform':{'translation':[x,y,z],'scale':list(scale),'rotation':[0.,0.,0.,1.]},'components':components or []});return eid
    def comp(ty,props):return {'type':ty,'enabled':True,'props':props}
    def script(name,props=None):return comp('Script',{'graphRef':f'Content/Graphs/{name}.rxgraph','module':'','props':props or {}})
    def sprite(texture='',sp='',ppu=100.,order=0,clip='idle'):
        return comp('Sprite',{'texture':texture,'sprite':sp,'clip':clip if sp else '', 'frame':0.,'tint':[1.,1.,1.,1.],'flipX':False,'flipY':False,'pixelsPerUnit':float(ppu),'sortingOrder':float(order)})
    ent('CS_Controller',components=[script('GameController')])
    ent('Camera',z=10.,components=[comp('Camera',{'projection':'orthographic','orthoSize':6.,'near':.1,'far':100.,'fov':60.})])
    bg=TEX/'server-garden.png'
    with Image.open(bg) as im: bw,bh=im.size
    ent('ServerGarden',components=[sprite(texture=meta(bg,'texture'),ppu=100.,order=-100)],scale=(2133.333/bw,1200/bh,1.))
    ent('CircuitLanes',components=[sprite(texture=guid(TEX/'circuit-lanes.png'),order=-90)])
    for slot in range(12):
        for kind,(sp,ppu) in enumerate(sprites,1):
            ent(f'CS_Tower_{slot}_{kind}',-100.,-100.,components=[sprite(sp=sp,ppu=ppu,order=10-slot//4,clip='' if kind>=3 else 'idle'),script('RenderHero' if kind>=3 else 'RenderActor',{'visualId':float(slot*4+kind-1)})])
    for i in range(36):ent(f'CS_Enemy{i}',-100.,-100.,components=[sprite(sp=bugs,ppu=100.,order=12),script('RenderPacket',{'visualId':float(100+i)})])
    for i in range(40):ent(f'CS_Pulse{i}',-100.,-100.,components=[sprite(sp=pulses,ppu=190.,order=25),script('RenderPacket',{'visualId':float(200+i)})])
    ent('CS_CompileBeam',-100.,-100.,components=[sprite(texture=guid(TEX/'compile-beam.png'),order=30),script('RenderActor',{'visualId':300.})])
    def state(name,keys):ent(name,components=[script('PublishState',dict(zip(['x','y','z','sx','sy','sz'],map(float,keys))))])
    state('CS_State',[0,1,2,3,4,5]);state('CS_StateAux',[6,7,8,9,10,11]);state('CS_Meta',[12,13,14,15,16,17]);state('CS_Lanes',[16,17,18,19,14,15])
    for i in range(12):state(f'CS_Cell{i}',[100+i*10,101+i*10,108+i*10,109+i*10,107+i*10,106+i*10])
    p=CONTENT/'Scenes/Main.rxscene';json_file(p,{'name':'Code Sentinels · 编译防线','mode':'2d','gravity':[0,0,0],'next_id':len(es)+1,'entities':es});meta(p,'scene')
    print(json.dumps({'scene':str(p),'entities':len(es),'graphs':5,'spriteBindings':sprites},ensure_ascii=False))

if __name__=='__main__':scene_build()
