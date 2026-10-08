"""Pack actual native-resolution frames with uniform2/3 source-pixel density, never whole512->256."""
import importlib
import media as m
from PIL import Image
import numpy as np
import math,shutil,sys

SAMPLING=2/3
NOMINAL_SPAN=2.5456
def clip_layout(spec,source_size,foot,bounds):
 # Include the reference foot even when a later pose has moved away from it.
 union=[min([b[0] for b in bounds]+[foot[0]]),min([b[1] for b in bounds]+[foot[1]]),max([b[2] for b in bounds]+[foot[0]]),max([b[3] for b in bounds]+[foot[1]])]
 side=int(math.ceil(max(384,union[2]-union[0]+12,union[3]-union[1]+12)/3)*3)
 tile=side*2//3
 if tile>512:raise RuntimeError('Full motion needs a frame larger than512; do not silently shrink or crop')
 origin=[]
 for axis,preference in [(0,.5),(1,.88)]:
  lower=math.ceil(union[axis+2]+6-side);upper=math.floor(union[axis]-6)
  inner_lower=max(lower,0);inner_upper=min(upper,source_size[axis]-side)
  if inner_lower<=inner_upper:lower,upper=inner_lower,inner_upper
  origin.append(max(lower,min(upper,round(foot[axis]-preference*side))))
 source_span=spec.get('outputPlaneSpan',NOMINAL_SPAN)
 unit_per_source_pixel=source_span/source_size[0]
 if not math.isclose(unit_per_source_pixel,NOMINAL_SPAN/384,rel_tol=1e-8):raise RuntimeError('Inconsistent source pixel/world contract '+spec['id'])
 span=side*unit_per_source_pixel
 return {'sourceCrop':[origin[0],origin[1],side,side],'sourceBodyUnion':union,'frameSize':[tile,tile],'pivot':[(foot[i]-origin[i])/side for i in range(2)],'nativePlaneSpan':span,'sourceToAtlasScale':SAMPLING,'pixelsPerWorldUnit':tile/span,'sourceFootAnchor':foot,'sourceSize':source_size,'bakeFitScale':1.}

def make_tile(native,layout):
 x,y,w,h=layout['sourceCrop'];size=layout['frameSize']
 return native.crop((x,y,x+w,y+h)).resize(tuple(size),Image.Resampling.LANCZOS)

def pack_character(character,publish=True):
 overrides=m.read(m.HERE/'action-overrides.json');anchors=m.read(m.HERE/'reviews/reference-anchors.json')[character]
 sources=[];provenance=[];clips={a:{} for a in m.SEGMENTS};boxes=[];placements=[];offset=0;cx=0;cy=0;row_height=0;atlas_width=4096
 for direction in m.DIRS:
  for action in m.SEGMENTS:
   folder=m.resolve_source(character,direction,action,overrides)
   if not (folder/'extracted.json').exists() or not (folder/'source-review.json').exists():return
   review=m.read(folder/'source-review.json');meta=m.read(folder/'extracted.json')
   if not review.get('approved') or meta.get('review')!='approved' or meta.get('sourceReviewSha256')!=m.sha(folder/'source-review.json'):return
   if not meta.get('nativeFrames') or meta['nativeFrames'].get('scaleFromSource')!=1 or meta.get('matteProcessing',{}).get('version',0)<4:return
   clip=meta['clips'][action];desired={'idle':6,'walk':12,'attack':12,'cast':16,'hit':8,'death':10}[action]
   selected=np.linspace(clip['start'],clip['endExclusive']-1,desired).round().astype(int).tolist();spec=m.read(folder/'request.json');foot=spec.get('sourceFootAnchor',anchors[direction]['foot']);bounds=[]
   for index in selected:
    image=Image.open(folder/'frames-native'/f'{index:03}.png')
    if list(image.size)!=meta['actualSourceSize']:raise RuntimeError('Native frame was resized before packing: '+folder.name)
    box=image.getbbox()
    if not box:raise RuntimeError('Empty real character frame '+folder.name)
    bounds.append(box)
   layout=clip_layout(spec,meta['actualSourceSize'],foot,bounds);tile=layout['frameSize'][0]
   for index in selected:
    if cx+tile>atlas_width:cx=0;cy+=row_height;row_height=0
    boxes.append([cx,cy,tile,tile]);placements.append((folder,index,layout,[cx,cy]));cx+=tile;row_height=max(row_height,tile)
   source_indices=meta['sourceFrameIndices'][action]
   clips[action][direction]={'start':offset,'endExclusive':offset+desired,'fps':{'idle':6,'walk':12,'attack':16,'cast':12,'hit':20,'death':12}[action],'loop':action in ['idle','walk'],'sourceJob':folder.name,'sourceFrameIndices':[source_indices[j-clip['start']] for j in selected],**layout}
   offset+=desired
   if not any(p['id']==folder.name for p in provenance):provenance.append(m.read(folder/'video.json'))
 atlas=Image.new('RGBA',(atlas_width,cy+row_height))
 for folder,index,layout,position in placements:
  native=Image.open(folder/'frames-native'/f'{index:03}.png').convert('RGBA');atlas.paste(make_tile(native,layout),tuple(position))
 destination=m.ANIM if publish else m.HERE/'density-staging';destination.mkdir(parents=True,exist_ok=True)
 path=destination/f'{character}.png';temporary=destination/f'{character}.pending.png'
 if publish and path.exists() and (destination/f'{character}.json').exists():
  old=m.read(destination/f'{character}.json')
  if old.get('samplingContract')!='uniform-source-density-v1':
   archive=m.HERE/'obsolete-atlases/pre-density';archive.mkdir(parents=True,exist_ok=True)
   for ext in ['png','json']:
    target=archive/f'{character}.{ext}'
    if not target.exists():shutil.copy2(destination/f'{character}.{ext}',target)
 atlas.save(temporary);temporary.replace(path)
 sizes=sorted({tuple(b[2:]) for b in boxes})
 metadata={'schemaVersion':2,'id':character,'image':character+'.png','width':atlas.width,'height':atlas.height,'frameCount':offset,'frameSize':[256,256],'variableFrameSize':len(sizes)>1 or sizes[0]!=(256,256),'frameSizes':[list(v) for v in sizes],'preservePixelDensity':True,'pixelsPerWorldUnit':256/NOMINAL_SPAN,'samplingContract':'uniform-source-density-v1','spatialContract':'source-crop-ground-anchor-v2','pivot':[.5,.88],'nativePlaneSpan':NOMINAL_SPAN,'directions':m.DIRS,'directionConvention':m.DIRECTION_CONVENTION,'boxes':boxes,'clips':clips,'provenance':provenance,'visualReview':'approved','anchorMeaning':'Original reference foot transformed through a per-clip native-pixel crop. Every source pixel is sampled at exactly2/3 of the384-source baseline; larger action bounds increase frame size instead of reducing character density. This is not1:1 source-pixel delivery.'}
 m.save(destination/f'{character}.json',metadata)
 if publish:
  m.PUBLIC.mkdir(parents=True,exist_ok=True);shutil.copy2(path,m.PUBLIC/path.name);shutil.copy2(destination/f'{character}.json',m.PUBLIC/f'{character}.json')
  manifest=m.read(m.PROJECT/'Content/UI/v6/resource-manifest.json');manifest['characters'][character].update({'ready':True,'qualityStatus':'verified-native-density-and-selected-source-windows','pendingSourceRepairs':[],'nativeAtlas':f'Content/Animations/v6/characters/{character}.png','nativeMetadata':f'Content/Animations/v6/characters/{character}.json','frameCount':offset,'atlasSha256':m.sha(path),'preservePixelDensity':True,'pixelsPerWorldUnit':256/NOMINAL_SPAN,'variableFrameSize':metadata['variableFrameSize']})
  manifest['characterAnimationState']='ready' if all(c.get('ready') for c in manifest['characters'].values()) else 'production-and-visual-review';m.save(m.PROJECT/'Content/UI/v6/resource-manifest.json',manifest);m.save(m.ROOT/'packages/client/public/games/code-sentinels/ui-v6/resource-manifest.json',manifest)
 m.emit({'packed':character,'published':publish,'frames':offset,'atlasSize':atlas.size,'frameSizes':sizes,'pixelsPerWorldUnit':metadata['pixelsPerWorldUnit']});return metadata

if __name__=='__main__':pack_character(sys.argv[1],publish='--staging' not in sys.argv)
