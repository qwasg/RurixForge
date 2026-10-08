"""Decode genuine provider video, chroma-key, sample and pack real source frames."""
import argparse
import hashlib
import json
import math
import sys
from datetime import datetime,timezone
from pathlib import Path

HERE=Path(__file__).resolve().parent
PROJECT=HERE.parents[1]
sys.path.insert(0,str(HERE.parent/'python-libs'))
import imageio_ffmpeg
import numpy as np
from PIL import Image, ImageDraw, ImageFilter

def read(path): return json.loads(path.read_text(encoding='utf-8-sig'))
def save(path,value): path.write_text(json.dumps(value,ensure_ascii=False,indent=2),encoding='utf-8')

def key_frame(raw,key,protected_foreground=None,allow_variable_background=False):
    rgb=np.asarray(raw,dtype=np.float32)/255
    if key=='magenta':
        if allow_variable_background:
            # H3 occasionally changes the key plane from magenta to cyan and
            # grey during a landing pulse. Estimate the palette from the outer
            # three pixels, then remove only connected matching background.
            # These are still the generated RGB pixels; this changes alpha and
            # removes background spill, never replaces the animated subject.
            edge=np.concatenate([rgb[:3].reshape(-1,3),rgb[-3:].reshape(-1,3),
                                 rgb[:,:3].reshape(-1,3),rgb[:,-3:].reshape(-1,3)])
            colors,counts=np.unique(np.round(edge*32).astype(np.int16),axis=0,return_counts=True)
            palette=colors[counts>=max(5,len(edge)*.008)].astype(np.float32)/32
            median_edge=np.median(edge,axis=0)
            cyan_plane=bool(len(palette) and np.max(np.minimum(palette[:,1],palette[:,2])-palette[:,0])>.3)
            grey_plane=np.ptp(median_edge)<.3
            orange_plane=bool(median_edge[0]-median_edge[2]>.3 and .18<median_edge[1]<.85 and median_edge[2]<.4)
            mixed_plane=len(palette)>1 and np.max(np.ptp(palette,axis=0))>.03 and (cyan_plane or grey_plane)
            if len(palette) and (orange_plane or mixed_plane):
                distance=np.full(rgb.shape[:2],10,dtype=np.float32)
                bg=np.zeros_like(rgb)
                for color in palette:
                    delta=np.max(np.abs(rgb-color),axis=-1)
                    closer=delta<distance;distance=np.minimum(distance,delta);bg[closer]=color
                matte=np.clip((distance-(3/255 if orange_plane else 8/255))/(.5 if orange_plane else 18/255),0,1)
                for low,high1,high2 in [(0,1,2),(1,0,2)]:
                    if np.max(np.minimum(palette[:,high1],palette[:,high2])-palette[:,low])>.3:
                        excess=np.minimum(rgb[:,:,high1],rgb[:,:,high2])-rgb[:,:,low]
                        matte=np.minimum(matte,1-np.clip((excess-8/255)/.25,0,1))
                candidate=(matte<.99).astype(np.uint8)*255
                canvas=Image.new('L',(raw.width+2,raw.height+2),255)
                canvas.paste(Image.fromarray(candidate,'L'),(1,1));ImageDraw.floodfill(canvas,(0,0),128)
                outside=np.asarray(canvas.crop((1,1,raw.width+1,raw.height+1)))==128
                alpha=np.where(outside,matte,1)
                # Magenta key pockets enclosed by arms or smoke rings are
                # still empty background, just as in the flat-key path below.
                magenta=np.minimum(rgb[:,:,0],rgb[:,:,2])-rgb[:,:,1]
                alpha=np.minimum(alpha,1-np.clip((magenta-10/255)/.7,0,1))
                if protected_foreground is not None:alpha=np.maximum(alpha,protected_foreground)
                color=np.clip((rgb-(1-alpha[:,:,None])*bg)/np.maximum(alpha[:,:,None],1/255),0,1)
                color[:,:,1]=np.where(alpha<.98,np.minimum(color[:,:,1],np.maximum(color[:,:,0],color[:,:,2])),color[:,:,1])
                rgba=np.concatenate([color,alpha[:,:,None]],axis=-1);rgba[alpha<=1/255]=0
                return Image.fromarray(np.round(rgba*255).astype(np.uint8),'RGBA')
        # Some provider frames drift to a different flat chroma color. Sample
        # only the clear corners and key only the exterior-connected chroma
        # region, preserving colored lights enclosed by solid building panels.
        corners=np.concatenate([rgb[:12,:12].reshape(-1,3),rgb[:12,-12:].reshape(-1,3),
                                rgb[-12:,:12].reshape(-1,3),rgb[-12:,-12:].reshape(-1,3)])
        background=np.median(corners,axis=0)
        ranked=np.argsort(background)
        low,high1,high2=ranked
        strength=background[high2]-background[low]
        if strength<.3:
            # A few source videos briefly change to flat white instead of the
            # requested magenta. Background-connected distance key preserves
            # the cream painted blades and isolated white specular highlights.
            distance=np.max(np.abs(rgb-background[None,None,:]),axis=-1)
            candidate=(distance<24/255).astype(np.uint8)*255
            matte=np.clip((distance-3/255)/(15/255),0,1)
        else:
            if background[high1]-background[low]>.7*strength:
                excess=np.minimum(rgb[:,:,high1],rgb[:,:,high2])-rgb[:,:,low]
                strength=min(background[high1],background[high2])-background[low]
            else:
                # Handles provider hue drifts through peach/orange where only
                # one primary is strong. The earlier two-primary assumption
                # misclassified compression blocks as foreground.
                excess=rgb[:,:,high2]-rgb[:,:,low]
            candidate=(excess>10/255).astype(np.uint8)*255
            matte=1-np.clip((excess-10/255)/max(strength-25/255,.1),0,1)
        small=Image.fromarray(candidate,'L').resize((128,128),Image.Resampling.NEAREST)
        connected=Image.new('L',(130,130),255)
        connected.paste(small,(1,1))
        ImageDraw.floodfill(connected,(0,0),128)
        outside=connected.crop((1,1,129,129)).point(lambda v:255 if v==128 else 0)
        outside=outside.resize(raw.size,Image.Resampling.NEAREST).filter(ImageFilter.MaxFilter(3))
        external=np.asarray(outside)>0
        if low==1 and min(background[0],background[2])-background[1]>.3:
            # No building in this set contains magenta paint. Also key enclosed
            # magenta pockets between a smoke ring and the actual structure.
            external=np.ones_like(external)
        alpha=np.where(external,matte,1)
        if protected_foreground is not None:
            # A reference-derived inner matte protects the fixed machine's
            # orange paint when the provider makes its background orange too.
            alpha=np.maximum(alpha,protected_foreground)
        spill=(1-alpha)[:,:,None]*background[None,None,:]
        color=np.clip((rgb-spill)/np.maximum(alpha[:,:,None],1/255),0,1)
        if low==1 and min(background[0],background[2])-background[1]>.3:
            # Colored lighting on the virtual key plane is still background.
            # Recover its actual dimming as a neutral contact shadow instead
            # of allowing blue/purple spill to become an opaque rectangle.
            shadow=(rgb[:,:,1]<12/255)&(np.minimum(rgb[:,:,0],rgb[:,:,2])>40/255)
            ratio=(rgb[:,:,0]+rgb[:,:,2])/max(background[0]+background[2],.1)
            alpha=np.where(shadow,np.clip(1-ratio,0,1),alpha)
            color=np.where(shadow[:,:,None],0,color)
    elif key=='black':
        peak=rgb.max(axis=-1)
        alpha=np.where(peak<=3/255,0,peak)
        color=np.clip(rgb/np.maximum(alpha[:,:,None],1/255),0,1)
    else: raise ValueError('Explicit magenta or black key required')
    rgba=np.concatenate([color,alpha[:,:,None]],axis=-1)
    rgba[alpha<=1/255]=0
    return Image.fromarray(np.round(rgba*255).astype(np.uint8),'RGBA')

def extract(clip):
    folder=HERE/'jobs'/clip
    spec=read(folder/'request.json')
    receipt=read(folder/'video.json')
    if receipt['meta']['mode']!='image2video': raise ValueError('Receipt is not genuine I2V')
    source=PROJECT/receipt['fileRef']
    digest=hashlib.sha256(source.read_bytes()).hexdigest()
    if digest!=receipt['sha256']: raise ValueError('Saved original video differs from provider receipt')
    reader=imageio_ffmpeg.read_frames(str(source),pix_fmt='rgb24')
    info=next(reader)
    width,height=info['size']
    source_size=[width,height]
    crop_box=None
    window=None
    if receipt['meta'].get('provider') in {'starframe','comfyui-minimax-h3'}:
        review=folder/'source-review.json'
        if not review.exists(): raise ValueError('Review full provider source contact sheet and record complete action window before extraction')
        window=read(review)
        if not window.get('completeActionVisible') or not window.get('centralCropPreservesSubject'):
            raise ValueError('Source action and crop require visual production review')
        side=min(width,height)
        crop_box=[(width-side)//2,(height-side)//2,(width+side)//2,(height+side)//2]
        width=height=side
    protected=None
    if clip in {'coal-power-work','hydro-power-work'}:
        original=Image.open(PROJECT/spec['sourceImage']).convert('RGBA')
        geometry=spec['referenceGeometry']
        a=original.getchannel('A').crop(tuple(geometry['originalAlphaBbox']))
        a=a.resize(tuple(geometry['fittedSize']),Image.Resampling.LANCZOS)
        c=Image.new('L',(1024,1024))
        c.paste(a,tuple(geometry['groundTopLeft']))
        c=c.resize((width,height),Image.Resampling.LANCZOS).filter(ImageFilter.MinFilter(3))
        protected=np.asarray(c,dtype=np.float32)/255
    decoded=[]
    details=[]
    source_indices=[]
    total_source_frames=0
    for index,raw in enumerate(reader):
        total_source_frames=index+1
        if window and not (window['startSec']<=index/info['fps']<=window['endSec']): continue
        image=Image.frombytes('RGB',tuple(source_size),raw)
        if crop_box: image=image.crop(tuple(crop_box))
        variable_background=receipt['meta'].get('provider')=='comfyui-minimax-h3' and spec['key']=='magenta'
        frame=key_frame(image,spec['key'],protected,variable_background)
        mask=frame.getchannel('A').point(lambda v:255 if v>=32 else 0)
        box=mask.getbbox()
        margins=[box[0],box[1],width-box[2],height-box[3]] if box else [width,height,width,height]
        resized=frame.resize((256,256),Image.Resampling.LANCZOS)
        # Only the outer six pixels taper particles that the provider sent
        # beyond its requested safe margin. Inner 244x244 pixels are untouched.
        yy,xx=np.mgrid[:256,:256]
        edge=np.minimum.reduce([xx,yy,255-xx,255-yy]).astype(np.float32)
        t=np.clip(edge/6,0,1)
        taper=t*t*(3-2*t)
        a=np.asarray(resized.getchannel('A'),dtype=np.float32)
        resized.putalpha(Image.fromarray(np.round(a*taper).astype(np.uint8),'L'))
        decoded.append(resized)
        source_indices.append(index)
        details.append({'frame':index,'foregroundBbox':box,'marginsLTRB':margins})
    if len(decoded)<spec['outputFrames']: raise ValueError('Insufficient real decoded frames')
    count=spec['outputFrames']
    positions=[round(i*(len(decoded)-1)/(count-1)) for i in range(count)]
    selected=[decoded[i] for i in positions]
    indices=[source_indices[i] for i in positions]
    stable_mask=None
    if clip in {'hydro-power-work','coal-power-work','mobile-relay-land'}:
        # H3 introduced colored macroblock noise in the empty background of
        # this loop. The hydropower machinery is fixed and its water stays
        # inside the supplied channels, so a source-derived soft garbage matte
        # safely removes distant background noise without changing motion.
        original=Image.open(PROJECT/spec['sourceImage']).convert('RGBA')
        geometry=spec['referenceGeometry']
        alpha=original.getchannel('A').crop(tuple(geometry['originalAlphaBbox']))
        alpha=alpha.resize(tuple(geometry['fittedSize']),Image.Resampling.LANCZOS)
        canvas=Image.new('L',(1024,1024))
        canvas.paste(alpha,tuple(geometry['groundTopLeft']))
        if clip=='mobile-relay-land':
            air=Image.new('L',(1024,1024));position=list(geometry['groundTopLeft']);position[1]+=geometry['airOffsetY'];air.paste(alpha,tuple(position))
            canvas=Image.fromarray(np.maximum(np.asarray(canvas),np.asarray(air)).astype(np.uint8),'L')
        dilation=41 if clip=='mobile-relay-land' else 13
        stable_mask=canvas.resize((256,256),Image.Resampling.LANCZOS).filter(ImageFilter.MaxFilter(dilation)).filter(ImageFilter.GaussianBlur(1.5))
        if clip=='coal-power-work':
            # The fixed coal plant has vertical chimney plumes. A source-alpha
            # horizontal envelope keeps all plume height while excluding the
            # distant corner macroblocks from the old provider's orange plane.
            bbox=canvas.resize((256,256),Image.Resampling.LANCZOS).getbbox()
            stable_mask=Image.new('L',(256,256));ImageDraw.Draw(stable_mask).rectangle((bbox[0]-12,0,bbox[2]+12,bbox[3]+10),fill=255)
            stable_mask=stable_mask.filter(ImageFilter.GaussianBlur(3))
        matte=np.asarray(stable_mask,dtype=np.float32)/255
        for frame in selected:
            alpha=np.asarray(frame.getchannel('A'),dtype=np.float32)
            frame.putalpha(Image.fromarray(np.round(alpha*matte).astype(np.uint8),'L'))
    if clip=='mobile-relay-land':
        # The original vehicle has no magenta surfaces. Clear the thin magenta
        # remnant at the boundary where H3's cyan pulse meets the key plane.
        for frame in selected:
            pixels=np.asarray(frame).astype(np.float32)
            spill=np.minimum(pixels[:,:,0],pixels[:,:,2])-pixels[:,:,1]
            alpha=pixels[:,:,3]*(1-np.clip((spill-8)/32,0,1))
            frame.putalpha(Image.fromarray(np.round(alpha).astype(np.uint8),'L'))
    effect_fit=None
    if clip=='power-arc':
        # One fixed union crop for the whole real clip gives the small generated
        # discharge a readable sprite footprint. No per-frame position changes,
        # synthesized motion or replacement pixels are introduced.
        bounds=[frame.getchannel('A').point(lambda v:255 if v>=8 else 0).getbbox() for frame in selected]
        bounds=[box for box in bounds if box]
        union=[max(0,min(b[0] for b in bounds)-6),max(0,min(b[1] for b in bounds)-6),
               min(256,max(b[2] for b in bounds)+6),min(256,max(b[3] for b in bounds)+6)]
        w,h=union[2]-union[0],union[3]-union[1];scale=168/max(w,h)
        fitted=(round(w*scale),round(h*scale));offset=((256-fitted[0])//2,(256-fitted[1])//2)
        normalized=[]
        for frame in selected:
            canvas=Image.new('RGBA',(256,256));canvas.paste(frame.crop(tuple(union)).resize(fitted,Image.Resampling.LANCZOS),offset);normalized.append(canvas)
        selected=normalized
        effect_fit={'method':'one fixed union crop across real source frames','cropAt256':union,'fittedSize':list(fitted),'offset':list(offset)}
    emission_gain={'kinetic-hit':2.0,'power-arc':3.0,'repair':1.5}.get(clip,1.0)
    if emission_gain!=1:
        for frame in selected:
            alpha=np.asarray(frame.getchannel('A'),dtype=np.float32)
            frame.putalpha(Image.fromarray(np.round(np.minimum(alpha*emission_gain,255)).astype(np.uint8),'L'))
    fade_out_frames=(8 if count==48 else 4) if spec['category']=='effect' else 0
    if fade_out_frames:
        # One-shot effects must finish transparently, even when the generated
        # source leaves dust or rubble on its last image. This is an alpha-only
        # editorial fade over actual video frames, not fabricated animation.
        for index in range(count-fade_out_frames,count):
            t=(index-(count-fade_out_frames))/(fade_out_frames-1)
            gain=1-t*t*(3-2*t)
            alpha=np.asarray(selected[index].getchannel('A'),dtype=np.float32)
            selected[index].putalpha(Image.fromarray(np.round(alpha*gain).astype(np.uint8),'L'))
    unique=len({hashlib.sha256(f.tobytes()).hexdigest() for f in selected})
    if unique<max(16,count//2): raise ValueError(f'No adequate real source motion: {unique}/{count} unique frames')
    cell,padding,columns=256,2,8
    rows=math.ceil(count/columns)
    size=(columns*cell+(columns+1)*padding,rows*cell+(rows+1)*padding)
    atlas=Image.new('RGBA',size)
    boxes=[]
    for index,frame in enumerate(selected):
        x=padding+(index%columns)*(cell+padding)
        y=padding+(index//columns)*(cell+padding)
        atlas.paste(frame,(x,y))
        boxes.append([x,y,cell,cell])
    target=PROJECT/'Content/Animations/v5'/('buildings' if spec['category']=='building' else 'effects')
    target.mkdir(parents=True,exist_ok=True)
    atlas.save(target/f'{clip}.png',optimize=True)
    minimum=[min(d['marginsLTRB'][i] for d in details) for i in range(4)]
    meta={'id':clip,'image':f'{clip}.png','width':size[0],'height':size[1],'frames':boxes,'boxes':boxes,
          'frameCount':count,'fps':spec['playbackFps'],'loop':spec['loop'],'pivot':[.5,.5],
          'frameSize':[256,256],'crop':'none','chromaKey':spec['key'],'alphaMode':'straight',
          'effectUnionFit':effect_fit,'emissionGain':emission_gain,'fadeOutFrames':fade_out_frames,
          'chromaProcessing':'per-frame flat-corner estimate, exterior-connected color key, foreground structure protected',
          'variableBackgroundPalette':bool(variable_background),
          'edgeFeather':{'widthPixels':6,'frameSize':256,'innerPixelsUnchanged':244,'algorithm':'alpha-only smoothstep'},
          'sourceDerivedGarbageMatte':{'enabled':stable_mask is not None,'purpose':('remove distant key-plane artifacts around the supplied landing silhouettes, retaining a 20px dust margin' if clip=='mobile-relay-land' else 'remove distant background macroblocks around fixed machinery; vertical chimney plumes remain uncropped' if clip=='coal-power-work' else 'remove empty-background macroblock noise around fixed hydro machinery') if stable_mask is not None else None},
          'sourceDerivedInnerMatte':{'enabled':protected is not None,'purpose':'preserve fixed machine panels when provider background hue overlaps original paint'},
          'sourceFrameCount':total_source_frames,'sourceFps':info['fps'],'sourceDurationSec':info['duration'],
          'selectedSourceFrames':indices,'uniqueFrames':unique,'sourceSize':source_size,
          'sourceCropBox':crop_box,'sourceActionWindowSec':([window['startSec'],window['endSec']] if window else None),
          'minimumSourceMarginsLTRB':minimum,'clippedSourceFrames':sum(min(d['marginsLTRB'])<1 for d in details),
          'referenceGeometry':spec.get('referenceGeometry'),
          'provenance':{'method':'image-to-video-extracted-frames','model':receipt['meta']['model'],
                        'taskId':receipt['meta']['taskId'],'video':receipt['fileRef'],'videoSha256':digest,
                        'sourceImage':spec.get('sourceImage'),'request':f'pipeline/v5/jobs/{clip}/request.json'}}
    if spec['category']=='effect':
        meta['clips']={'oneshot':{'start':0,'endExclusive':count,'fps':spec['playbackFps'],'loop':False}}
        meta['recommendedBlend']='alpha' if clip in {'heavy-impact','collapse-explosion'} else 'additive'
    save(target/f'{clip}.json',meta)
    save(folder/'extraction.json',{**meta,'sourceFrames':details})
    failure=folder/'extraction-failure.json'
    if failure.exists():
        previous=read(failure)
        previous['resolvedAt']=datetime.now(timezone.utc).isoformat()
        previous['resolution']='Same original provider video reprocessed with per-frame flat-background extraction; no new paid submission'
        save(folder/'extraction-recovery.json',previous)
        failure.unlink()
    contact=Image.new('RGB',(1024,568),'#253038')
    draw=ImageDraw.Draw(contact)
    for panel,index in enumerate([round(i*(count-1)/7) for i in range(8)]):
        x,y=(panel%4)*256,(panel//4)*284
        contact.paste(selected[index],(x,y),selected[index])
        draw.text((x+8,y+262),f'{clip} {index}/{indices[index]}',fill='#f2f0e9')
    contact.save(folder/'contact.png')
    print(json.dumps({'id':clip,'taskId':receipt['meta']['taskId'],'frames':count,'uniqueFrames':unique,
                      'sourceFrames':total_source_frames,'windowFrames':len(decoded),'minimumMarginsLTRB':minimum,'atlas':str(target/f'{clip}.png')},ensure_ascii=True))

if __name__=='__main__':
    parser=argparse.ArgumentParser()
    parser.add_argument('clip')
    extract(parser.parse_args().clip)
