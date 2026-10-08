"""Stage explicitly observed physical-state windows for alpha review, never auto-approve."""
from media import *
for id,proposal in read(HERE/'physical-state-windows.json').items():
 folder=HERE/'jobs'/id;receipt=read(folder/'video.json')
 if (folder/'source-review.json').exists() and read(folder/'source-review.json').get('approved'):continue
 save(folder/'source-review.json',{'id':id,'approved':False,'reviewedAt':stamp(),'notes':proposal['notes'],'segments':proposal['segments'],'videoSha256':receipt['sha256'],'status':'awaiting-alpha-window-inspection'});extract(folder,True);meta=read(folder/'extracted.json');canvas=Image.new('RGB',(8*160,4*184),(34,42,48));draw=ImageDraw.Draw(canvas)
 for row,action in enumerate(['idle','walk','hit','death']):
  if action not in meta['clips']:
   draw.text((4,row*184+80),action+' uses a separate actual source',fill='white');continue
  clip=meta['clips'][action]
  for col,index in enumerate(np.linspace(clip['start'],clip['endExclusive']-1,8).round().astype(int)):
   im=Image.open(folder/'frames'/f'{index:03}.png').convert('RGBA').resize((160,160),Image.Resampling.LANCZOS);canvas.paste(im,(col*160,row*184),im);draw.text((col*160+3,row*184+163),f'{action} {index}',fill='white')
 out=HERE/'reviews'/f'{id}-clean.jpg';canvas.save(out,quality=94);emit({'review':str(out),'approved':False})
