"""Propose clean physical GPT idle/walk windows, with actual alpha contacts for acceptance."""
from media import *
proposals={};canvas=Image.new('RGB',(8*160,4*184),(34,42,48));draw=ImageDraw.Draw(canvas)
for row,(d,action) in enumerate([('s','idle'),('nw','idle'),('s','walk'),('nw','walk')]):
 folder=resolve_source('gpt',d,action,read(HERE/'action-overrides.json'));receipt=read(folder/'video.json');reader=imageio_ffmpeg.read_frames(str(PROJECT/receipt['fileRef']),pix_fmt='rgb24');info=next(reader);frames=[key_green(Image.frombytes('RGB',info['size'],b)) for b in reader]
 if action=='idle':start,end=(0,10) if d=='s' else (108,124);count=6
 else:
  features=[np.asarray(im.resize((96,96)),dtype=np.float32)[58:89,18:78] for im in frames];best=None
  for start in range(8,66):
   for period in range(24,42):
    end=start+period
    if end>103:continue
    motion=float(np.abs(features[start+period//2]-features[start]).mean());error=float(np.abs(features[end]-features[start]).mean())
    if motion<.5:continue
    score=error+.02*abs(period-30)
    if best is None or score<best[0]:best=(score,start,end)
  if best is None:raise RuntimeError('No real walking cycle '+folder.name)
  _,start,end=best;count=24
 segments={action:[start,end,count,action in ['idle','walk']]};proposals[folder.name]={'segments':segments,'videoSha256':receipt['sha256']}
 for col,index in enumerate(np.linspace(start,end-1,8).round().astype(int)):
  im=frames[index].resize((160,160),Image.Resampling.LANCZOS);canvas.paste(im,(col*160,row*184),im);draw.text((col*160+3,row*184+163),f'{d} {action} {index}',fill='white')
out=HERE/'reviews/gpt-physical-selected.jpg';canvas.save(out,quality=94);save(out.with_suffix('.json'),proposals);emit({'review':str(out),'proposals':proposals})
