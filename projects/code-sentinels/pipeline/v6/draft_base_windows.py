"""Propose real composite idle/walk/reaction/death windows and render a review contact; does NOT approve."""
from media import *
char=sys.argv[1];review={};canvas=Image.new('RGB',(13*128,8*150),(26,30,34));draw=ImageDraw.Draw(canvas);overrides=read(HERE/'action-overrides.json');anchors=read(HERE/'reviews/reference-anchors.json')[char]
for row,d in enumerate(DIRS):
 folder=resolve_source(char,d,'idle',overrides)
 if not (folder/'video.json').exists():
  review[d]={'job':folder.name,'pending':True};draw.text((4,row*150+80),d+' actual base video not yet sampled',fill='white');continue
 spec=read(folder/'request.json');receipt=read(folder/'video.json');reader=imageio_ffmpeg.read_frames(str(PROJECT/receipt['fileRef']),pix_fmt='rgb24');info=next(reader);raws=[Image.frombytes('RGB',info['size'],b) for b in reader]
 keyed=[key_green(im) for im in raws];heights=[]
 for im in keyed:
  alpha=np.asarray(im)[:,:,3];labels,n=ndimage.label(alpha>30);counts=np.bincount(labels.reshape(-1));counts[0]=0
  if len(counts)<2:heights.append(0);continue
  ys,xs=np.where(labels==int(counts.argmax()));heights.append(int(ys.max()-ys.min()+1))
 reference=float(np.median(heights[:12]));ending=float(np.median(heights[-8:]));threshold=ending+.30*(reference-ending)
 physical=spec.get('action')=='physical-states';boundary=len(heights)-1;minimum_death=120 if physical else 150
 while boundary>minimum_death and heights[boundary]<threshold:boundary-=1
 death_start=max(minimum_death,boundary-14)
 foot=spec.get('sourceFootAnchor',anchors[d]['foot']);fx=round(foot[0]/info['size'][0]*96);fy=round(foot[1]/info['size'][1]*96)
 features=[np.asarray(im.resize((96,96)),dtype=np.float32)[max(0,fy-26):min(96,fy+5),max(0,fx-30):min(96,fx+30)] for im in keyed]
 best=None
 for start in range(32,75):
  for period in range(24,41):
   end=start+period
   if end>min(len(raws)-1,90 if physical else 116):continue
   motion=float(np.abs(features[start+period//2]-features[start]).mean());error=float(np.abs(features[end]-features[start]).mean())
   if motion<.4:continue
   score=error+.02*abs(period-28)
   if best is None or score<best[0]:best=(score,start,end,error)
 if best is None and d!='n':raise RuntimeError('No moving walk cycle '+folder.name)
 segments={'idle':[0,20,6,True]}
 if d!='n':segments.update({'walk':[best[1],best[2],24,True],'hit':[84 if physical else 156,max(110,min(death_start,132)) if physical else max(170,min(death_start,194)),20,False],'death':[death_start,len(raws),24,False]})
 review[d]={'job':folder.name,'segments':segments,'referenceHeight':reference,'restingHeight':ending,'deathStartDetected':death_start,'videoSha256':receipt['sha256'],'status':'needs-visual-review'}
 selected=[(0,'idle')]
 for action in ['walk','hit','death']:
  values=segments.get(action,[0,20,4,False]);start,end=values[:2]
  selected.extend((int(i),action) for i in np.linspace(start,end-1,4).round())
 for col,(index,action) in enumerate(selected):
  canvas.paste(raws[index].resize((128,128)),(col*128,row*150));draw.text((col*128+2,row*150+130),f'{d} {action} {index/24:.1f}',fill='white')
out=HERE/'reviews'/f'{char}-base-windows.jpg';canvas.save(out,quality=94);save(out.with_suffix('.json'),review);emit({'review':str(out)})
