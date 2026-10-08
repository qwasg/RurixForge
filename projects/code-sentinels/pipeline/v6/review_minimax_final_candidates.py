"""Read-only QA: exact original RGB and delivered crop for remaining MiniMax gates."""
from media import *
doc=read(HERE/'density-staging/minimax.json');atlas=Image.open(HERE/'density-staging/minimax.png').convert('RGBA');report=[]
sheet=Image.new('RGB',(1050,3*550),(30,42,47));draw=ImageDraw.Draw(sheet)
for row,idx in enumerate([163,166,484]):
 action,d,clip=next((a,d,c) for a,dirs in doc['clips'].items() for d,c in dirs.items() if c['start']<=idx<c['endExclusive'])
 folder=HERE/'jobs'/clip['sourceJob'];ex=read(folder/'extracted.json');si=clip['sourceFrameIndices'][idx-clip['start']];ni=ex['clips'][action]['start']+ex['sourceFrameIndices'][action].index(si)
 native=Image.open(folder/'frames-native'/f'{ni:03}.png').convert('RGBA');x,y,w,h=doc['boxes'][idx];tile=atlas.crop((x,y,x+w,y+h))
 reader=imageio_ffmpeg.read_frames(str(PROJECT/read(folder/'video.json')['fileRef']),pix_fmt='rgb24');info=next(reader)
 for fi,raw in enumerate(reader):
  if fi==si:original=Image.frombytes('RGB',info['size'],raw);break
 reader.close()
 for col,im in enumerate([original,native,tile]):
  view=im.copy();view.thumbnail((345,510));sheet.paste(view,(col*350,row*550+30),view if view.mode=='RGBA' else None)
 draw.text((0,row*550+8),f'{idx} {action}/{d} {folder.name} raw {si}; raw / native alpha / delivered',fill='white')
 report.append({'atlasFrame':idx,'action':action,'direction':d,'job':folder.name,'sourceFrame':si,'nativeFrame':ni,'crop':clip['sourceCrop'],'size':[w,h]})
sheet.save(HERE/'reviews/minimax-density-exact-candidates.jpg',quality=96)
folder=HERE/'jobs/minimax-n-attack-v2';reader=imageio_ffmpeg.read_frames(str(PROJECT/read(folder/'video.json')['fileRef']),pix_fmt='rgb24');info=next(reader);sheet=Image.new('RGB',(4*384,420),(30,42,47));draw=ImageDraw.Draw(sheet)
for index,raw in enumerate(reader):
 if index in [23,38,54,61]:
  col=[23,38,54,61].index(index);sheet.paste(Image.frombytes('RGB',info['size'],raw),(col*384,25));draw.text((col*384+5,5),f'original RGB {index}',fill='white')
reader.close();sheet.save(HERE/'reviews/minimax-n-attack-raw-boundary.jpg',quality=98);save(HERE/'reviews/minimax-density-exact-candidates.json',report);emit(report)
