"""Show eight real direction videos together before choosing game clip windows."""
from media import *
char=sys.argv[1];kind=sys.argv[2] if len(sys.argv)>2 else ACTION_VERSION
canvas=Image.new('RGB',(8*176,8*198),(24,27,31));draw=ImageDraw.Draw(canvas);rows=[]
for row,direction in enumerate(DIRS):
 folder=HERE/'jobs'/f'{char}-{direction}-{kind}'
 if not (folder/'video.json').exists():
  draw.text((6,row*198+90),direction+' actual video not yet sampled',fill='white');rows.append({'direction':direction,'pending':True});continue
 receipt=read(folder/'video.json');reader=imageio_ffmpeg.read_frames(str(PROJECT/receipt['fileRef']),pix_fmt='rgb24');info=next(reader);size=info['size'];frames=[Image.frombytes('RGB',size,x) for x in reader]
 if kind==ACTION_VERSION:indices=[0,24,48,96,144,168,204,len(frames)-1]
 else:indices=np.linspace(0,len(frames)-1,8).round().astype(int).tolist()
 for col,index in enumerate(indices):
  index=min(index,len(frames)-1);canvas.paste(frames[index].resize((176,176)),(col*176,row*198));draw.text((col*176+3,row*198+178),f'{direction} {index/24:.2f}s',fill='white')
 rows.append({'direction':direction,'sourceFrames':len(frames),'viewedIndices':indices,'videoSha256':sha(PROJECT/receipt['fileRef'])})
out=HERE/'reviews'/f'{char}-{kind}.jpg';out.parent.mkdir(exist_ok=True);canvas.save(out,quality=92);save(out.with_suffix('.json'),{'character':char,'kind':kind,'rows':rows,'status':'contact-sheet-awaiting-human-visual-review'})
emit({'review':str(out)})
