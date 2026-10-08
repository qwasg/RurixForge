"""Import verified project-agent I2V frame atlases as native .rxsprite assets."""
import json,math
from PIL import Image
from build_native import ROOT,TEX,SPR,guid,json_file,scene_build

def import_actor(actor):
    src=ROOT/'public/assets/characters'
    doc=json.loads((src/f'{actor}.json').read_text(encoding='utf-8-sig'))
    boxes=doc['boxes'];assert len(boxes)>=2
    assert len({(b[2],b[3]) for b in boxes})==1,'Union crop frame dimensions must match'
    assert doc['provenance']['method']=='image-to-video-extracted-frames'
    texture=TEX/f'{actor}-video-atlas.png'
    with Image.open(src/f'{actor}.png') as im:
        assert im.mode=='RGBA';assert im.getextrema()[3][0]==0
        frame_hashes={hash(im.crop((x,y,x+w,y+h)).tobytes()) for x,y,w,h in boxes}
        assert len(frame_hashes)>=2,'Actual extracted frames must differ'
        assert all(x>=0 and y>=0 and x+w<=im.width and y+h<=im.height for x,y,w,h in boxes)
        # Runtime sprites are 180 px tall. Keep 512 px on the largest frame
        # dimension and repack within 4096; the original video atlas is retained.
        factor=min(1.,512/max(boxes[0][2:]));fw=max(1,round(boxes[0][2]*factor));fh=max(1,round(boxes[0][3]*factor))
        columns=6;runtime=Image.new('RGBA',((fw+2)*columns+2,(fh+2)*math.ceil(len(boxes)/columns)+2))
        runtime_boxes=[]
        for i,(x,y,w,h) in enumerate(boxes):
            frame=im.crop((x,y,x+w,y+h)).resize((fw,fh),Image.Resampling.LANCZOS)
            dx=2+(i%columns)*(fw+2);dy=2+(i//columns)*(fh+2);runtime.paste(frame,(dx,dy));runtime_boxes.append([dx,dy,fw,fh])
        assert max(runtime.size)<=4096;runtime.save(texture)
    boxes=runtime_boxes
    meta={'guid':guid(texture),'type':'texture','importer':'png','provenance':{'origin':'gen-video','detail':doc['provenance']},'build_state':'current'}
    json_file(texture.with_suffix('.png.meta'),meta)
    sprite=SPR/f'{actor}.rxsprite';names=[f'frame_{i}' for i in range(len(boxes))]
    idle=list(range(24))+list(range(22,0,-1)) if actor=='gpt' else list(range(32))+list(range(30,0,-1))
    cast=list(range(24,32)) if actor=='gpt' else list(range(8,20))
    json_file(sprite,{'version':1,'texture':guid(texture),'pivot':doc.get('pivot',[.5,1]),
        'frames':{name:{'bbox':box} for name,box in zip(names,boxes)},
        'clips':{'idle':{'frames':[names[i] for i in idle],'fps':doc['fps'],'loop':True},
                 'cast':{'frames':[names[i] for i in cast],'fps':doc['fps'],'duration':.65,'loop':False,'onFinish':'hold'}},
        'animator':{'defaultState':'idle','parameters':{'attacking':'bool'},
            'states':{'idle':{'clip':'idle'},'cast':{'clip':'cast'}},
            'transitions':[{'from':'idle','to':'cast','when':[{'param':'attacking','eq':True}]},
                           {'from':'cast','to':'idle','hasExitTime':True}]}})
    json_file(sprite.with_suffix('.rxsprite.meta'),{'guid':guid(sprite),'type':'sprite','importer':'sprite','provenance':{'origin':'gen-video','detail':doc['provenance']},'build_state':'current'})
    return {'actor':actor,'frames':len(boxes),'differentFrames':len(frame_hashes),'fps':doc['fps'],'frameSize':boxes[0][2:],'atlasSize':list(runtime.size),'idleFrames':len(idle),'castFrames':len(cast),'spriteGuid':guid(sprite),'sourceVideo':doc['provenance']['videoFileRef']}

if __name__=='__main__':
    out=[import_actor(actor) for actor in ['deepseek','gpt']]
    json_file(ROOT/'game/native/video-animation-import.json',out)
    scene_build();print(json.dumps(out,ensure_ascii=False))
