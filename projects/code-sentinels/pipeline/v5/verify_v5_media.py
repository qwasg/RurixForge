"""Real decoded-video provenance and final atlas contract checks; no generation."""
import argparse
import hashlib
import json
import sys
from datetime import datetime,timezone
from pathlib import Path
HERE=Path(__file__).resolve().parent
PROJECT=HERE.parents[1]
sys.path.insert(0,str(HERE.parent/'python-libs'))
import imageio_ffmpeg
import numpy as np
from PIL import Image

def read(path):return json.loads(path.read_text(encoding='utf-8-sig'))
def check_source(folder,decode=True):
 spec=read(folder/'request.json');receipt=read(folder/'video.json');source=PROJECT/receipt['fileRef']
 assert source.is_file(), 'missing MP4'
 assert hashlib.sha256(source.read_bytes()).hexdigest()==receipt['sha256'],'source checksum'
 assert receipt['meta']['mode']=='image2video','not I2V receipt'
 result={'id':folder.name,'provider':receipt['meta']['provider'],'taskId':receipt['meta']['taskId'],'bytes':source.stat().st_size,'sha256Matched':True}
 if receipt['meta']['provider']=='comfyui-minimax-h3':
  dest=PROJECT/receipt['meta']['receipts'];graph=read(dest/'workflow.json');history=read(dest/'history.json')
  assert history['status']['status_str']=='success','local prompt not successful'
  assert graph['6']['class_type']=='MiniMaxH3ImageToVideo' and 'first_frame' in graph['6']['inputs'],'no genuine I2V input'
  assert graph['11']['class_type']=='SamplerCustomAdvanced','no diffusion sampling'
  assert all(n['class_type'] not in {'RepeatImageBatch','RepeatLatentBatch'} for n in graph.values()),'repeated stills in workflow'
  assert hashlib.sha256((dest/'workflow.json').read_bytes()).hexdigest()==receipt['meta']['workflowSha256'],'workflow checksum'
  assert all(read(p)['verifiedSameBytes'] for p in dest.glob('*.local-upload.json')),'reference upload checksum'
  result['nativeSamplingAndReferenceVerified']=True
 if decode:
  reader=imageio_ffmpeg.read_frames(str(source),pix_fmt='rgb24');info=next(reader)
  hashes=set();count=0;deltas=[];previous=None
  for raw in reader:
   hashes.add(hashlib.sha256(raw).hexdigest());count+=1
   pixels=np.frombuffer(raw,dtype=np.uint8).reshape(info['size'][1],info['size'][0],3)[::8,::8].astype(np.float32)
   if previous is not None:deltas.append(float(np.abs(pixels-previous).mean()))
   previous=pixels
  assert count>=spec['outputFrames'],'not enough actual source frames'
  assert len(hashes)>=max(16,spec['outputFrames']//2),'insufficient actual source variation'
  result.update({'decodedFrames':count,'uniqueDecodedFrames':len(hashes),'fps':info['fps'],'size':info['size'],'durationSec':info['duration'],
                 'averageAdjacentPixelDelta':float(np.mean(deltas)),'maxAdjacentPixelDelta':max(deltas)})
 return result
def check_atlas(path,kind,spec=None):
 meta=read(path);atlas=Image.open(path.with_suffix('.png')).convert('RGBA')
 count=128 if kind=='building' else spec['outputFrames']
 assert atlas.size==(meta['width'],meta['height']),'atlas dimension metadata'
 assert meta['frameCount']==count and len(meta['boxes'])==count and len(meta['frames'])==count,'frame count'
 assert meta['pivot']==[.5,.5] and meta['frameSize']==[256,256],'pivot/frame size'
 assert meta['frames']==meta['boxes'],'frame aliases differ'
 corners=[];corner_coverage=[];corner_frame_coverage=[];hashes=set();visible=[]
 for x,y,w,h in meta['boxes']:
  assert all(isinstance(v,int) for v in [x,y,w,h]) and w==h==256 and x>=0 and y>=0 and x+w<=atlas.width and y+h<=atlas.height,'invalid frame box'
  frame=atlas.crop((x,y,x+w,y+h));alpha=np.asarray(frame.getchannel('A'))
  corners.append(int(max(alpha[0,:].max(),alpha[-1,:].max(),alpha[:,0].max(),alpha[:,-1].max())))
  patch_coverage=[float((patch>32).mean()) for patch in [alpha[:24,:24],alpha[:24,-24:],alpha[-24:,:24],alpha[-24:,-24:]]]
  corner_coverage.extend(patch_coverage);corner_frame_coverage.append(sum(patch_coverage)/4)
  hashes.add(hashlib.sha256(frame.tobytes()).hexdigest());visible.append(int((alpha>16).sum()))
 assert max(corners)==0,'opaque outer edge'
 # A localized particle or expanding ring may enter one corner; an unwanted
 # background plate occupies most of the combined four empty corner regions.
 assert max(corner_frame_coverage)<=.45,'opaque background plate across the empty frame corners'
 assert max(visible)>64,'atlas is invisible'
 if kind=='building':
  assert atlas.size==(2582,3356),'building packing'
  assert abs(meta['normalizationSpan']-.640625)<.003,'building normalization'
  expected={'land':(0,32,16,False),'work':(32,80,16,True),'destroy':(80,128,24,False)}
 else:
  expected={'oneshot':(0,count,spec['playbackFps'],False)}
  assert meta['recommendedBlend']==('alpha' if spec['id'] in {'heavy-impact','collapse-explosion'} else 'additive'),'effect blend'
 for key,values in expected.items():
  clip=meta['clips'][key];assert tuple(clip[k] for k in ['start','endExclusive','fps','loop'])==values,'clip mapping'
 return {'id':meta['id'],'kind':kind,'frameCount':count,'atlasSize':list(atlas.size),'uniqueAtlasFrames':len(hashes),'outerEdgeAlpha':max(corners),'maximumCombinedCornerCoverage':max(corner_frame_coverage),'maximumIndividualCornerCoverage':max(corner_coverage),'visibleFrames':sum(v>64 for v in visible),'contractVerified':True}
def main():
 parser=argparse.ArgumentParser();parser.add_argument('--require-all',action='store_true');parser.add_argument('--skip-decode',action='store_true');args=parser.parse_args()
 specs=[read(p) for p in sorted((HERE/'jobs').glob('*/request.json'))]
 report={'at':datetime.now(timezone.utc).isoformat(),'expectedSources':len(specs),'sources':[],'atlases':[],'missingSources':[],'missingAtlases':[],'errors':[],'generationSubmissions':0}
 for spec in specs:
  folder=HERE/'jobs'/spec['id']
  if not (folder/'video.json').exists():report['missingSources'].append(spec['id']);continue
  try:report['sources'].append(check_source(folder,not args.skip_decode))
  except Exception as exc:report['errors'].append({'id':spec['id'],'phase':'source','error':str(exc)})
 buildings=sorted(set(s['building'] for s in specs if s['category']=='building'))
 for slug in buildings:
  path=PROJECT/'Content/Animations/v5/buildings'/(slug+'.json')
  if not path.exists():report['missingAtlases'].append(slug);continue
  try:report['atlases'].append(check_atlas(path,'building'))
  except Exception as exc:report['errors'].append({'id':slug,'phase':'atlas','error':str(exc)})
 for spec in specs:
  if spec['category']=='building':continue
  path=PROJECT/'Content/Animations/v5/effects'/(spec['id']+'.json')
  if not path.exists():report['missingAtlases'].append(spec['id']);continue
  try:report['atlases'].append(check_atlas(path,'effect',spec))
  except Exception as exc:report['errors'].append({'id':spec['id'],'phase':'atlas','error':str(exc)})
 report['complete']=not report['errors'] and not report['missingSources'] and not report['missingAtlases']
 (HERE/'media-verification.json').write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding='utf-8')
 print(json.dumps({'verifiedSources':len(report['sources']),'verifiedAtlases':len(report['atlases']),'missingSources':len(report['missingSources']),'missingAtlases':len(report['missingAtlases']),'errors':report['errors'],'complete':report['complete']},ensure_ascii=True))
 if report['errors'] or (args.require_all and not report['complete']):raise SystemExit(1)
if __name__=='__main__':main()
