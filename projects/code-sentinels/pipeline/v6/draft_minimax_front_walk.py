"""Find and show a genuinely repeating alternating-foot source cycle for final visual acceptance."""
from media import *
folder=HERE/'jobs/minimax-s-walk-physical-v4';receipt=read(folder/'video.json');reader=imageio_ffmpeg.read_frames(str(PROJECT/receipt['fileRef']),pix_fmt='rgb24');info=next(reader);frames=[key_green(Image.frombytes('RGB',info['size'],r)) for r in reader];features=[np.asarray(im.resize((96,96)),dtype=np.float32)[58:90,18:78] for im in frames];best=None
for start in range(8,67):
 for period in range(16,35):
  end=start+period
  if end>104:continue
  motion=float(np.abs(features[start+period//2]-features[start]).mean());error=float(np.abs(features[end]-features[start]).mean())
  if motion<.6:continue
  score=error+.02*abs(period-24)
  if best is None or score<best[0]:best=(score,start,end)
if best is None:raise RuntimeError('No complete visible movement cycle')
_,start,end=best;canvas=Image.new('RGB',(8*160,184),(34,42,48));draw=ImageDraw.Draw(canvas)
for col,index in enumerate(np.linspace(start,end-1,8).round().astype(int)):
 im=frames[index].resize((160,160),Image.Resampling.LANCZOS);canvas.paste(im,(col*160,0),im);draw.text((col*160+3,163),str(index),fill='white')
out=HERE/'reviews/minimax-front-walk-cycle.jpg';canvas.save(out,quality=94);proposal={'id':folder.name,'segments':{'walk':[start,end,24,True]},'videoSha256':receipt['sha256']};save(out.with_suffix('.json'),proposal);emit(proposal)
