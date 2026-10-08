"""Package only real video-derived VFX frames into bounded GPU textures."""
import argparse
import hashlib
import json
import math
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
PROJECT = HERE.parents[1]
ROOT = PROJECT.parents[1]
sys.path.insert(0, str(HERE.parent / 'python-libs'))
from PIL import Image, ImageDraw, ImageChops


def read(path):
    return json.loads(path.read_text(encoding='utf-8-sig'))


def package(effect):
    spec = read(HERE / f'{effect}.request.json')
    receipt = read(HERE / f'{effect}.video.json')
    extracted = read(HERE / f'{effect}.frames.json')
    source = PROJECT / f'SourceMedia/effects/{effect}.mp4'
    raw = Image.open(PROJECT / extracted['atlas']['fileRef']).convert('RGBA')
    indices = [round(i*(extracted['frameCount']-1)/47) for i in range(48)]
    frames = [raw.crop((x,y,x+w,y+h)) for x,y,w,h in [extracted['boxes'][i] for i in indices]]
    unique = len({hashlib.sha256(frame.tobytes()).hexdigest() for frame in frames})
    nonempty = sum(frame.getchannel('A').getextrema()[1] >= 8 for frame in frames)
    if len(frames) != 48 or unique < 24 or nonempty < 24:
        raise ValueError(f'Actual animation validation failed: total={len(frames)}, distinct={unique}, nonempty={nonempty}')
    cell, padding, columns = 256, 2, 7
    rows = math.ceil(len(frames)/columns)
    width, height = columns*cell+(columns+1)*padding, rows*cell+(rows+1)*padding
    atlas = Image.new('RGBA', (width,height))
    unprocessed = Image.new('RGBA', (width,height))
    feather_width = 25  # 9.765625% of a 256px frame, only the outer border changes.
    feathered = effect in {'deepseek-tide','gpt-nova'}
    matte = Image.new('L',(cell,cell))
    matte.putdata([round(255*(lambda t:t*t*(3-2*t))(min(1,min(x,y,cell-1-x,cell-1-y)/feather_width)))
                   for y in range(cell) for x in range(cell)])
    center_unchanged = True
    outer_alpha_zero = True
    display_frames = []
    visible_hashes = set()
    boxes=[]
    for i,frame in enumerate(frames):
        resized=frame.resize((cell,cell),Image.Resampling.LANCZOS)
        x,y=padding+(i%columns)*(cell+padding),padding+(i//columns)*(cell+padding)
        unprocessed.paste(resized,(x,y))
        if feathered:
            before=resized.copy()
            resized.putalpha(ImageChops.multiply(resized.getchannel('A'),matte))
            # RGB never changes, and the central 80.5% keeps exact original RGBA.
            center=(feather_width,feather_width,cell-feather_width,cell-feather_width)
            center_unchanged &= before.crop(center).tobytes()==resized.crop(center).tobytes()
            center_unchanged &= before.convert('RGB').tobytes()==resized.convert('RGB').tobytes()
        alpha=resized.getchannel('A')
        outer_alpha_zero &= all(alpha.crop(box).getextrema()[1]==0 for box in
                                [(0,0,cell,1),(0,cell-1,cell,cell),(0,0,1,cell),(cell-1,0,cell,cell)])
        display_frames.append(resized)
        visible = resized.copy()
        visible.paste((0,0,0,0),(0,0,cell,cell),resized.getchannel('A').point(lambda a:255 if a==0 else 0))
        visible_hashes.add(hashlib.sha256(visible.tobytes()).hexdigest())
        atlas.paste(resized,(x,y))
        boxes.append([x,y,cell,cell])
    out=PROJECT/'public/assets/effects'
    out.mkdir(parents=True,exist_ok=True)
    path=out/f'{effect}.png'
    atlas.save(path,optimize=True)
    untouched=HERE/'unprocessed'
    untouched.mkdir(exist_ok=True)
    unprocessed.save(untouched/f'{effect}.png',optimize=True)
    (untouched/f'{effect}.json').write_text(json.dumps({'id':effect,'image':f'{effect}.png','boxes':boxes,'frameCount':48,
        'selectedSourceFrames':indices,'stage':'actual video frames after black-key/resize, before edge feather',
        'edgeFeatherApplied':False},indent=2),encoding='utf-8')
    if not center_unchanged or not outer_alpha_zero:
        raise ValueError('Edge matte violated inner RGBA or zero-edge alpha invariants')
    original=receipt['artifacts'][0]
    metadata={'id':effect,'image':f'{effect}.png','width':width,'height':height,'frames':boxes,'boxes':boxes,'frameCount':48,
              'fps':24,'sourceExtractionFps':12,'sourceFrameCount':extracted['frameCount'],'selectedSourceFrames':indices,
              'pivot':[0.5,0.5],'crop':'none','chromaKey':'black','alphaMode':'straight','recommendedBlend':'additive',
              'clips':{'oneshot':{'start':0,'endExclusive':48,'fps':24,'loop':False,'durationSec':2},
                       'idle':{'start':8,'endExclusive':40,'fps':16,'loop':True,'durationSec':2}},
              'uniqueFrames':len(visible_hashes),'sourceUniqueFrames':unique,'nonemptyFrames':nonempty,'gpuRgbaBytes':width*height*4,
              'edgeFeather':{'enabled':feathered,'widthPixels':feather_width if feathered else 0,'widthFraction':feather_width/cell if feathered else 0,
                             'algorithm':'alpha *= smoothstep(0, 25px, minimum distance to frame edge); RGB unchanged',
                             'centerRgbaUnchanged':center_unchanged,'outerAlphaZero':outer_alpha_zero,
                             'unprocessedAtlas':f'pipeline/v2/unprocessed/{effect}.png'},
              'provenance':{'method':'image-to-video-extracted-frames','model':original['meta']['model'],'taskId':original['meta']['taskId'],
                            'video':f'SourceMedia/effects/{effect}.mp4','videoSha256':hashlib.sha256(source.read_bytes()).hexdigest(),
                            'sourceImage':spec['imageRef'],'sourceImageSha256':hashlib.sha256((PROJECT/spec['imageRef']).read_bytes()).hexdigest(),
                            'rawAlphaAtlas':extracted['atlas']['fileRef']}}
    (out/f'{effect}.json').write_text(json.dumps(metadata,ensure_ascii=False,indent=2),encoding='utf-8')
    contact=Image.new('RGB',(1024,576),'#151c30')
    draw=ImageDraw.Draw(contact)
    for panel,index in enumerate([0,6,12,18,24,30,36,47]):
        x,y=(panel%4)*256,(panel//4)*288
        frame=display_frames[index].resize((248,248),Image.Resampling.LANCZOS)
        contact.paste(frame,(x+4,y+4),frame)
        draw.text((x+10,y+265),f'real source frame {index:02d}',fill='#dbe8ff')
    contact.save(HERE/f'{effect}.contact.png')
    terrain=Image.new('RGB',(1024,576))
    ground=ImageDraw.Draw(terrain)
    for row,(base,line) in enumerate([('#172637','#233c48'),('#d8cfb2','#c0b597')]):
        ground.rectangle((0,row*288,1024,(row+1)*288),fill=base)
        for gx in range(0,1024,32):
            ground.line((gx,row*288,gx,(row+1)*288),fill=line)
        for gy in range(row*288,(row+1)*288,32):
            ground.line((0,gy,1024,gy),fill=line)
        for col,index in enumerate([0,18,30,40]):
            frame=display_frames[index].resize((248,248),Image.Resampling.LANCZOS)
            terrain.paste(frame,(col*256+4,row*288+4),frame)
            ground.text((col*256+10,row*288+267),('dark' if row==0 else 'light')+f' ground | frame {index:02d}',fill='#f0f5ff' if row==0 else '#253741')
    terrain.save(HERE/f'{effect}.terrain-composite.png')
    evidence={'effect':effect,'taskId':original['meta']['taskId'],'uniqueFrames':len(visible_hashes),'sourceUniqueFrames':unique,'nonemptyFrames':nonempty,'frameCount':48,
              'atlasSize':[width,height],'pngBytes':path.stat().st_size,'gpuRgbaBytes':width*height*4,'videoSha256':metadata['provenance']['videoSha256'],
              'sourceImage':spec['imageRef'],'edgeFeather':metadata['edgeFeather'],'passed':True}
    (HERE/f'{effect}.verification.json').write_text(json.dumps(evidence,ensure_ascii=False,indent=2),encoding='utf-8')
    print(json.dumps(evidence,ensure_ascii=False))


if __name__=='__main__':
    parser=argparse.ArgumentParser()
    parser.add_argument('effect',choices=['deepseek-tide','gpt-nova','pycharm-matrix'])
    package(parser.parse_args().effect)
