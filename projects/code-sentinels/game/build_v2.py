"""Build V2 native scenes, terrain layers, real I2V effects and state adapters.

Terrain pixels are baked from the same compiled Rust tile getter the game uses.
The bitmap only depicts topology; pathfinding remains entirely native.
"""
import ctypes,hashlib,json,math,pathlib,shutil,subprocess
from PIL import Image,ImageDraw,ImageFont,ImageEnhance,ImageFilter,ImageOps
import build_native as b

ROOT=b.ROOT;TEX=b.TEX;SPR=b.SPR;GR=b.GR
b.MODULE='Content/Scripts/sentinels_v2.rs'
MODULE=b.MODULE
W,H=24,14
COLORS={0:'#202d36',1:'#49515f',2:'#245166',3:'#605d53',4:'#777765',5:'#233f42',6:'#795658',7:'#a18b60'}
EFFECTS=['deepseek-tide','gpt-nova','pycharm-matrix']

def dump(path,obj):b.json_file(path,obj)
def compile_core():
    source=ROOT/MODULE;sha=hashlib.sha256(source.read_bytes()).hexdigest()[:16]
    dll=ROOT/'game/v2'/f'preview-{sha}.dll';dll.parent.mkdir(parents=True,exist_ok=True)
    if not dll.exists():subprocess.run(['rustc','--crate-type','cdylib','--edition','2021',str(source),'-C','opt-level=2','-o',str(dll)],check=True)
    api=ctypes.CDLL(str(dll));api.cs_map_template.argtypes=[ctypes.c_float];api.cs_map_template.restype=ctypes.c_float
    api.cs_spawn_cell.argtypes=[ctypes.c_float];api.cs_spawn_cell.restype=ctypes.c_float
    return api

def terrain_assets(api):
    paths=[];layouts=[]
    tile_size=100
    for level in range(1,4):
        for stage in range(3):
            tiles=[int(api.cs_map_template(level*10000+stage*1000+i)) for i in range(W*H)]
            spawns=[int(api.cs_spawn_cell(level*10+i))for i in range(4)if api.cs_spawn_cell(level*10+i)>=0]
            layouts.append({'level':level,'stage':stage,'tiles':tiles,'spawnCells':spawns})
            base_ground=Image.open(TEX/'open-battlefield.png').convert('RGBA')
            im=ImageOps.fit(base_ground,(2400,1400),method=Image.Resampling.LANCZOS)
            materials=Image.open(TEX/'terrain-tiles.png').convert('RGBA')
            # Connected terrain masks keep shores and ridges natural; the tile
            # getter is still the sole source for every walkable/blocked cell.
            material_index={1:0,2:1,3:2,4:3,7:4,6:5}
            for terrain_type,index in material_index.items():
                patch=materials.crop(((index%3)*512,(index//3)*512,(index%3+1)*512,(index//3+1)*512)).resize((320,320),Image.Resampling.LANCZOS)
                surface=Image.new('RGBA',im.size)
                for yy in range(0,1400,320):
                    for xx in range(0,2400,320):surface.paste(patch,(xx,yy))
                mask=Image.new('L',im.size);md=ImageDraw.Draw(mask)
                for row in range(H):
                    for col in range(W):
                        if tiles[row*W+col]!=terrain_type:continue
                        x,y=col*100,row*100;pad=4 if terrain_type not in [3,7] else 9
                        md.rounded_rectangle((x+pad,y+pad,x+100-pad,y+100-pad),radius=14 if terrain_type!=7 else 3,fill=255)
                        for dc,dr in [(1,0),(0,1)]:
                            nc,nr=col+dc,row+dr
                            if nc<W and nr<H and tiles[nr*W+nc]==terrain_type:
                                if dc:md.rectangle((x+40,y+pad,x+160,y+100-pad),fill=255)
                                else:md.rectangle((x+pad,y+40,x+100-pad,y+160),fill=255)
                mask=mask.filter(ImageFilter.GaussianBlur(2.3));im=Image.composite(surface,im,mask)
            d=ImageDraw.Draw(im)
            for row in range(H):
                for col in range(W):
                    t=tiles[row*W+col];x,y=col*100,row*100
                    if t==4 and (row==H-1 or tiles[(row+1)*W+col]!=4):
                        d.line([(x+12,y+93),(x+88,y+93)],fill='#b2b49d',width=5)
                    elif t==5:
                        d.rectangle((x,y,x+100,y+100),fill='#17353b')
                        d.rectangle((x+6,y+6,x+94,y+94),outline='#397c80',width=3)
            # Core and empty GPU sockets are native terrain decorations.
            for slot in range(8):
                gx=85+(slot%4)*78;gy=605+(slot//4)*105
                d.rounded_rectangle((gx-35,gy-25,gx+35,gy+25),radius=6,fill='#182936',outline='#72c6ab',width=3)
                d.line([(gx-10,gy),(gx+10,gy)],fill='#53736e',width=2)
            cx,cy=150,825;d.regular_polygon((cx,cy,40),6,fill='#205c56',outline='#88ffe0',width=5);d.ellipse((cx-16,cy-16,cx+16,cy+16),fill='#b0ffe9')
            for spawn in spawns:
                sx,sy=(spawn%W)*100+50,(spawn//W)*100+50
                d.regular_polygon((sx,sy,31),6,outline='#e684a5',width=5)
                d.ellipse((sx-19,sy-19,sx+19,sy+19),outline='#ffbfd7',width=3)
            p=TEX/f'v2-terrain-{level}-{stage}.png';im.save(p);b.meta(p,'texture');paths.append(p)
    dump(ROOT/'Content/Data/maps-v2.json',{'width':W,'height':H,'maps':layouts})
    return paths

def gpu_assets():
    catalog=json.loads((ROOT/'references/sources-gpu.json').read_text(encoding='utf8'))['gpus']
    names=['5060','5070','5080','5090','PRO 6000','A100','H200']
    im=Image.new('RGBA',(7*192,128));d=ImageDraw.Draw(im)
    try:font=ImageFont.truetype('C:/Windows/Fonts/arialbd.ttf',18)
    except OSError:font=ImageFont.load_default()
    for i,(entry,name) in enumerate(zip(catalog,names)):
        x=i*192
        d.rounded_rectangle((x+3,3,x+188,125),radius=8,fill='#162b31',outline='#6aa79c',width=3)
        source=ROOT/'references'/entry['image']
        with Image.open(source) as raw:
            photo=raw.convert('RGBA');photo.thumbnail((176,94),Image.Resampling.LANCZOS)
            im.alpha_composite(photo,(x+(192-photo.width)//2,7+(94-photo.height)//2))
        d.text((x+96,112),name,font=font,fill='#e4f5ee',anchor='mm')
    p=TEX/'v2-gpu-models.png';im.save(p)
    frames={f'frame_{i}':{'bbox':[i*192,0,192,128]} for i in range(7)}
    result=b.sprite_doc('V2GPUs',p,frames,{'idle':{'frames':list(frames),'fps':1,'loop':True}},(.5,.5))
    dump(p.with_suffix('.png.meta'),{'guid':b.guid(p),'type':'texture','importer':'png','build_state':'current','provenance':{'origin':'user-import','detail':{'method':'proportional atlas of verified vendor photographs','sources':catalog}}})
    dump(ROOT/'game/v2/gpu-photo-import.json',{'frames':7,'generatedHardware':False,'sources':[{'frame':i,'model':e['model'],'picturedProduct':e['picturedProduct'],'sourceImage':e['image'],'sourcePage':e['sourcePage']}for i,e in enumerate(catalog)]})
    return result

def effects_asset():
    sheet=Image.new('RGBA',(12*258+2,12*258+2));frames={};sources=[]
    for k,name in enumerate(EFFECTS):
        folder=ROOT/'public/assets/effects';doc=json.loads((folder/f'{name}.json').read_text(encoding='utf-8-sig'))
        boxes=doc.get('boxes') or doc.get('frames');assert len(boxes)==48
        with Image.open(folder/f'{name}.png') as source:
            for n,box in enumerate(boxes):
                if isinstance(box,dict):box=box.get('bbox',box.get('rect'))
                x,y,w,h=box;frame=source.crop((x,y,x+w,y+h)).convert('RGBA');assert frame.size==(256,256)
                i=k*48+n;px=2+(i%12)*258;py=2+(i//12)*258;sheet.paste(frame,(px,py));frames[f'frame_{i}']={'bbox':[px,py,256,256]}
        sources.append({'effect':name,'provenance':doc.get('provenance'),'frames':48})
    p=TEX/'v2-skills-video.png';sheet.save(p);sp=b.sprite_doc('V2Skills',p,frames,{'idle':{'frames':list(frames),'fps':1,'loop':True}},(.5,.5))
    dump(ROOT/'game/v2/skill-import.json',{'atlas':str(p),'dimensions':list(sheet.size),'frames':144,'sources':sources})
    return sp

def enemy_assets():
    names=['null-pointer','memory-leak','race-condition','deadlock','stack-overflow','heap-corruption','deadlock-boss','stack-boss']
    sheet=Image.new('RGBA',(16*256,256))
    for kind,name in enumerate(names):
        art=TEX/'Enemies'/f'{name}.png'
        if not art.exists():raise FileNotFoundError(f'Final enemy art required: {art}')
        with Image.open(art) as raw:
            cut=raw.convert('RGBA');bbox=cut.getbbox()
            if bbox:cut=cut.crop(bbox)
            cut.thumbnail((220,225),Image.Resampling.LANCZOS)
            for frame in range(2):
                index=kind*2+frame
                sheet.paste(cut,(index*256+(256-cut.width)//2,250-cut.height-frame*3),cut)
    p=TEX/'v2-bug-archetypes.png';sheet.save(p);frames={f'frame_{i}':{'bbox':[i*256,0,256,256]} for i in range(16)}
    return b.sprite_doc('V2Bugs',p,frames,{'idle':{'frames':list(frames),'fps':1,'loop':True}})

def graphs():
    g=b.Graph('V2Controller');a=g.n('event.on_start');n=g.call('cs_reset',g.c([]));g.link(a,n)
    a=g.n('event.on_update');n=g.call('cs_tick',g.p(a,'dt'));g.link(a,n)
    a=g.n('event.on_input');n=g.n('var.set',name=g.p(a,'action'),value=g.p(a,'value'));g.link(a,n)
    v=g.n('var.get',name=g.c('cs'));c=g.call('cs_input',g.p(v,'out'));g.link(n,c);n=g.n('var.set',name=g.c('cs'),value=g.c(0.));g.link(c,n);g.emit()
    for mode in ['Actor','Hero','Packet']:
        g=b.Graph('V2Render'+mode);g.prop('visualId',0.)
        tail=g.n('event.on_update');calls=[]
        for fn in ['cs_visual_x','cs_visual_y','cs_visual_scale']:
            n=g.call(fn,g.r('visualId'));g.link(tail,n);tail=n;calls.append(n)
        pos=g.n('math.vec3',x=g.p(calls[0]),y=g.p(calls[1]),z=g.c(0.));scale=g.n('math.vec3',x=g.p(calls[2]),y=g.p(calls[2]),z=g.c(1.))
        tr=g.n('transform.compose',translation=g.p(pos,'out'),scale=g.p(scale,'out'));n=g.n('entity.set_transform',entity=g.c('$self'),transform=g.p(tr,'out'));g.link(tail,n);tail=n
        if mode=='Packet':
            f=g.call('cs_visual_frame',g.r('visualId'));g.link(tail,f);n=g.n('sprite.set_frame',entity=g.c('$self'),index=g.p(f));g.link(f,n)
        if mode=='Hero':
            f=g.call('cs_visual_attacking',g.r('visualId'));g.link(tail,f);n=g.n('animator.set_bool',entity=g.c('$self'),param=g.c('attacking'),value=g.p(f));g.link(f,n)
        g.emit()
    for emitter in [False,True]:
        g=b.Graph('V2PublishEmitter' if emitter else 'V2PublishState');tail=g.n('event.on_update');reads=[]
        keys=['x','y','age','life','kind'] if emitter else ['x','y','z','sx','sy','sz']
        for key in keys:
            g.prop(key,0.);n=g.call('cs_get',g.r(key));g.link(tail,n);tail=n;reads.append(n)
        pos=g.n('math.vec3',x=g.p(reads[0]),y=g.p(reads[1]),z=g.c(0.) if emitter else g.p(reads[2]))
        vals=reads[2:] if emitter else reads[3:];scale=g.n('math.vec3',**{k:g.p(n) for k,n in zip(['x','y','z'],vals)})
        tr=g.n('transform.compose',translation=g.p(pos,'out'),scale=g.p(scale,'out'));n=g.n('entity.set_transform',entity=g.c('$self'),transform=g.p(tr,'out'));g.link(tail,n);g.emit()

def batch_controller(entities):
    bindings=[]
    for e in entities:
        if e['name']=='CS_Controller':continue
        for c in e['components']:
            if c['type']!='Script':continue
            graph=pathlib.Path(c['props']['graphRef']).stem;props=c['props']['props']
            if graph=='V2PublishState':kind=0;data=[int(props[k])for k in ['x','y','z','sx','sy','sz']]
            elif graph=='V2PublishEmitter':kind=4;data=[int(props[k])for k in ['x','y','age','life','kind']]+[0]
            else:kind={'V2RenderActor':1,'V2RenderPacket':2,'V2RenderHero':3}[graph];data=[int(props['visualId'])]+[0]*5
            bindings.append({'entityId':e['id'],'kind':kind,'data':data})
        e['components']=[c for c in e['components']if c['type']!='Script']
    g=b.Graph('V2BatchController')
    def frame(dt):return g.n('call.native_frame',module=g.c(MODULE),fn=g.c('cs_frame'),dt=dt,bindings=g.c(bindings),animatorParam=g.c('attacking'))
    start=g.n('event.on_start');reset=g.call('cs_reset',g.c([]));g.link(start,reset);f=frame(g.c(0.));g.link(reset,f)
    update=g.n('event.on_update');f=frame(g.p(update,'dt'));g.link(update,f)
    inp=g.n('event.on_input');save=g.n('var.set',name=g.p(inp,'action'),value=g.p(inp,'value'));g.link(inp,save)
    get=g.n('var.get',name=g.c('cs'));call=g.call('cs_input',g.p(get,'out'));g.link(save,call)
    clear=g.n('var.set',name=g.c('cs'),value=g.c(0.));g.link(call,clear);g.emit()
    entities[0]['components'][0]['props']['graphRef']='Content/Graphs/V2BatchController.rxgraph'
    dump(ROOT/'game/v2/frame-bindings.json',{'abi':1,'bindings':bindings,'entityUpdatesPerFrame':len(bindings),'nativeCallsPerFrame':1})

def build():
    api=compile_core();terrain=terrain_assets(api);gpu=gpu_assets();fx=effects_asset();enemies=enemy_assets();graphs()
    hero=[]
    for name,height in [('VSCode',1.),('PyCharm',1.),('deepseek',1.45),('gpt',1.45)]:
        p=SPR/f'{name}.rxsprite';doc=json.loads(p.read_text(encoding='utf-8-sig'));fh=max(v['bbox'][3] for v in doc['frames'].values());hero.append((b.meta(p,'sprite'),fh/height))
    pulses=b.meta(SPR/'CodePulses.rxsprite','sprite');es=[]
    def comp(kind,props):return {'type':kind,'enabled':True,'props':props}
    def script(graph,props=None):return comp('Script',{'graphRef':f'Content/Graphs/{graph}.rxgraph','module':'','props':props or {}})
    def sprite(tx='',sp='',ppu=100.,order=0,clip='idle',blend='alpha'):
        return comp('Sprite',{'texture':tx,'sprite':sp,'clip':clip if sp else '', 'frame':0.,'tint':[1.,1.,1.,1.],'flipX':False,'flipY':False,'pixelsPerUnit':float(ppu),'sortingOrder':float(order),'chromaKey':'none','blendMode':blend})
    def ent(name,components=None,pos=(0.,0.,0.),scale=(1.,1.,1.)):
        es.append({'id':len(es)+1,'name':name,'transform':{'translation':list(pos),'rotation':[0.,0.,0.,1.],'scale':list(scale)},'components':components or []})
    ent('CS_Controller',[script('V2Controller')]);ent('Camera',[comp('Camera',{'projection':'orthographic','orthoSize':7.,'near':.1,'far':100.,'fov':60.})],(0.,0.,10.))
    background=TEX/'open-battlefield.png'
    if background:
        with Image.open(background)as im:bw,bh=im.size
        ent('V2Ground',[sprite(tx=b.meta(background,'texture'),order=-100,blend='opaque')],scale=(2488.8889/bw,1400/bh,1.))
    for i,path in enumerate(terrain):ent(f'CS_Terrain{i}',[sprite(tx=b.meta(path,'texture'),order=-80,blend='opaque'),script('V2RenderActor',{'visualId':float(400+i)})],(-100.,-100.,0.))
    for slot in range(24):
        for kind,(sp,ppu)in enumerate(hero,1):
            ent(f'CS_Actor{slot}_{kind}',[sprite(sp=sp,ppu=ppu,order=15,clip='' if kind>=3 else 'idle'),script('V2RenderHero'if kind>=3 else'V2RenderActor',{'visualId':float(slot*4+kind-1)})],(-100.,-100.,0.))
    for i in range(64):ent(f'CS_Enemy{i}',[sprite(sp=enemies,ppu=180.,order=17),script('V2RenderPacket',{'visualId':float(100+i)})],(-100.,-100.,0.))
    for i in range(32):ent(f'CS_Pulse{i}',[sprite(sp=pulses,ppu=220.,order=24),script('V2RenderPacket',{'visualId':float(200+i)})],(-100.,-100.,0.))
    for i in range(8):ent(f'CS_GPUVisual{i}',[sprite(sp=gpu,ppu=245.,order=12),script('V2RenderPacket',{'visualId':float(300+i)})],(-100.,-100.,0.))
    for i in range(64):
        ent(f'CS_VFX{i}',[comp('ParticleEmitter',{}),script('V2PublishEmitter',dict(zip(['x','y','age','life','kind'],[float(6000+i*10+j)for j in range(5)])))],(-100.,-100.,0.))
        ent(f'CS_VFXOverlay{i}',[sprite(sp=fx,ppu=128.,order=28,blend='additive'),script('V2RenderPacket',{'visualId':float(500+i)})],(-100.,-100.,0.))
    def state(name,keys):ent(name,[script('V2PublishState',dict(zip(['x','y','z','sx','sy','sz'],map(float,keys))))])
    state('CS_State',[0,1,2,3,4,5]);state('CS_Economy',[6,7,8,22,23,24]);state('CS_StateAux',[9,10,11,12,13,14]);state('CS_Campaign',[5,26,21,20,14,27]);state('CS_Meta',[15,25,16,17,18,19])
    state('CS_Routes',[28,27,14,18,19,20])
    for i in range(24):
        base=1000+i*20;state(f'CS_Unit{i}',[base+j for j in [0,1,2,3,4,5]]);state(f'CS_UnitAux{i}',[base+j for j in [6,7,8,9,10,11]]);state(f'CS_UnitCost{i}',[base+j for j in [18,17,12,1,13,14]])
    for i in range(8):state(f'CS_GPU{i}',[2000+i*10+j for j in [0,1,2,3,4,5]])
    for i in range(14):state(f'CS_MapRow{i}',[3000+i*10+j for j in [0,1,2,3,4,5]])
    batch_controller(es)
    p=ROOT/'Content/Scenes/Main.rxscene';dump(p,{'name':'Code Sentinels V2 — 开放战线','mode':'2d','gravity':[0,0,0],'next_id':len(es)+1,'entities':es});b.meta(p,'scene')
    dump(ROOT/'game/v2/scene-build.json',{'entities':len(es),'particleEmitters':64,'mapSize':[24,14],'module':MODULE,'realVideoFxFrames':144,'cameraOrtho':7})
    print(json.dumps({'scene':str(p),'entities':len(es),'emitters':64,'maps':9},ensure_ascii=False))

if __name__=='__main__':build()
