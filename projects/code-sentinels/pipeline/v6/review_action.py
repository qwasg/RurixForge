"""Visual review of every actual direction for one independently generated gesture."""
from media import *
char=sys.argv[1];action=sys.argv[2];overrides=read(HERE/'action-overrides.json');canvas=Image.new('RGB',(8*176,8*198),(24,29,34));draw=ImageDraw.Draw(canvas);records=[]
for row,d in enumerate(DIRS):
 folder=resolve_source(char,d,action,overrides);receipt=read(folder/'video.json');reader=imageio_ffmpeg.read_frames(str(PROJECT/receipt['fileRef']),pix_fmt='rgb24');info=next(reader);frames=[Image.frombytes('RGB',info['size'],raw) for raw in reader]
 indices=[0,12,24,40,56,72,96,len(frames)-1]
 review=read(folder/'source-review.json') if (folder/'source-review.json').exists() else {}
 if read(folder/'request.json').get('action')!=action and action in review.get('segments',{}):
  start,end=review['segments'][action][:2];indices=np.linspace(start,min(end-1,len(frames)-1),8).round().astype(int).tolist()
 for col,index in enumerate(indices):
  index=min(index,len(frames)-1);canvas.paste(frames[index].resize((176,176)),(col*176,row*198));draw.text((col*176+2,row*198+178),f'{d} {index/24:.2f}s',fill='white')
 records.append({'direction':d,'job':folder.name,'videoSha256':receipt['sha256'],'shownSourceFrames':indices})
out=HERE/'reviews'/f'{char}-{action}-current.jpg';canvas.save(out,quality=94);save(out.with_suffix('.json'),records);emit({'review':str(out),'directions':8})
