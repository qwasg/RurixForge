"""Combine three independently generated real I2V clips into a native sprite atlas."""
import argparse
import json
from pathlib import Path
from PIL import Image

HERE=Path(__file__).resolve().parent
PROJECT=HERE.parents[1]
OUT=PROJECT/'Content/Animations/v5/buildings'

def read(path): return json.loads(path.read_text(encoding='utf-8-sig'))
def merge(slug):
    cell,pad,columns,rows=256,2,10,13
    width,height=columns*cell+(columns+1)*pad,rows*cell+(rows+1)*pad
    atlas=Image.new('RGBA',(width,height))
    boxes=[]
    clips={}
    provenance=[]
    source_bounds={}
    source_meta=[]
    cursor=0
    for state,count,fps,loop in [('land',32,16,False),('work',48,16,True),('destroy',48,24,False)]:
        meta=read(OUT/f'{slug}-{state}.json')
        source=Image.open(OUT/f'{slug}-{state}.png').convert('RGBA')
        if meta['frameCount']!=count: raise ValueError('State frame contract mismatch')
        clips[state]={'start':cursor,'endExclusive':cursor+count,'fps':fps,'loop':loop}
        for box in meta['boxes']:
            x,y,w,h=box
            frame=source.crop((x,y,x+w,y+h))
            dx=pad+(cursor%columns)*(cell+pad)
            dy=pad+(cursor//columns)*(cell+pad)
            atlas.paste(frame,(dx,dy))
            boxes.append([dx,dy,cell,cell])
            cursor+=1
        provenance.append(meta['provenance'])
        source_meta.append({'state':state,'sourceFrameCount':meta['sourceFrameCount'],'sourceFps':meta['sourceFps'],
                            'sourceDurationSec':meta['sourceDurationSec'],'selectedSourceFrames':meta['selectedSourceFrames'],
                            'uniqueFrames':meta['uniqueFrames'],'minimumSourceMarginsLTRB':meta['minimumSourceMarginsLTRB'],
                            'clippedSourceFrames':meta['clippedSourceFrames'],
                            'alphaProcessing':{'chromaProcessing':meta.get('chromaProcessing'),'edgeFeather':meta.get('edgeFeather'),
                                               'sourceDerivedGarbageMatte':meta.get('sourceDerivedGarbageMatte'),
                                               'sourceDerivedInnerMatte':meta.get('sourceDerivedInnerMatte')}})
        extraction=read(HERE/'jobs'/f'{slug}-{state}'/'extraction.json')
        source_bounds[state]=extraction['sourceFrames']
    if cursor!=128: raise ValueError('Expected exactly 128 actual video frames')
    geometry=read(OUT/f'{slug}-work.json')['referenceGeometry']
    meta={'id':slug,'image':slug+'.png','width':width,'height':height,'frameSize':[256,256],
          'frames':boxes,'boxes':boxes,'frameCount':128,'fps':16,'pivot':[.5,.5],'clips':clips,
          'crop':'none','chromaKey':'magenta','alphaMode':'straight','recommendedBlend':'alpha',
          'referenceGeometry':geometry,'normalizationSpan':max(geometry['fittedSize'])/1024,
          'groundBaseFraction':geometry['groundBaseY']/1024,'sourceFrameBounds':source_bounds,
          'sourceClips':source_meta,'provenance':provenance}
    atlas.save(OUT/f'{slug}.png',optimize=True)
    (OUT/f'{slug}.json').write_text(json.dumps(meta,ensure_ascii=False,indent=2),encoding='utf-8')
    print(json.dumps({'id':slug,'frames':128,'atlasSize':[width,height],'normalizationSpan':meta['normalizationSpan'],
                      'taskIds':[p['taskId'] for p in provenance],'atlas':str(OUT/f'{slug}.png')},ensure_ascii=True))

if __name__=='__main__':
    parser=argparse.ArgumentParser()
    parser.add_argument('building')
    merge(parser.parse_args().building)
