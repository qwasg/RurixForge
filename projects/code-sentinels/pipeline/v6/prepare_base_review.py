"""Prepare alpha-cleaned selected source windows for visual acceptance; never auto-approve."""
from media import *
char=sys.argv[1];draft=read(HERE/'reviews'/f'{char}-base-windows.json');canvas=Image.new('RGB',(13*128,8*150),(35,48,54));draw=ImageDraw.Draw(canvas);overrides=read(HERE/'action-overrides.json')
for row,d in enumerate(DIRS):
 entry=draft[d]
 if entry.get('pending'):
  draw.text((4,row*150+80),d+' actual base video not yet sampled',fill='white');continue
 folder=HERE/'jobs'/entry['job'];segments=entry['segments'].copy()
 for action in list(segments):
  if overrides.get(f'{char}/{d}/{action}',folder.name)!=folder.name:segments.pop(action)
 if not segments:
  draw.text((4,row*150+130),f'{d} replaced by independent state sources',fill='white');continue
 review={'id':folder.name,'approved':False,'reviewedAt':stamp(),'status':'awaiting-view-of-clean-selected-frames','notes':'Original full source and selected actual event windows viewed; cleaned-frame acceptance is a separate step. Composite attack/cast are not used.','segments':segments,'videoSha256':entry['videoSha256']}
 if char=='deepseek' and d in ['e','ne']:review['garbageMatte']='primary-component-bounds';review['garbageMatteReason']='Original source has distant background sparkles, separate from the connected whale-girl. Retain full primary component bounds with6px safety margin; RGB subject pixels are never painted.'
 if char=='kimi' and d=='nw':review['garbageMatte']='primary-component-bounds';review['garbageMatteReason']='Viewed original source has distant colored backdrop dots outside the complete connected character. Keep the whole primary silhouette rectangle with6px safety margin; retain original body RGB and motion. Casting/hit segments are excluded separately.'
 existing=read(folder/'source-review.json') if (folder/'source-review.json').exists() else {}
 if not existing.get('approved'):save(folder/'source-review.json',review);extract(folder,True)
 meta=read(folder/'extracted.json');selected=[(meta['clips']['idle']['start'],'idle')] if 'idle' in meta['clips'] else [(None,'separate')]
 for action in ['walk','hit','death']:
  if action in meta['clips']:
   clip=meta['clips'][action];selected.extend((int(i),action) for i in np.linspace(clip['start'],clip['endExclusive']-1,4).round())
  else:selected.extend([(None,'separate')]*4)
 for col,(index,action) in enumerate(selected):
  if index is not None:
   im=Image.open(folder/'frames'/f'{index:03}.png').convert('RGBA').resize((128,128),Image.Resampling.LANCZOS);canvas.paste(im,(col*128,row*150),im)
  draw.text((col*128+2,row*150+130),f'{d} {action}',fill='white')
out=HERE/'reviews'/f'{char}-base-clean.jpg';canvas.save(out,quality=94);emit({'review':str(out),'approval':False})
