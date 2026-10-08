"""Show dense actual composite reaction frames; no source approval or frame changes."""
from media import *
char=sys.argv[1];indices=list(range(156,195,3));canvas=Image.new('RGB',(len(indices)*112,8*132),(27,34,40));draw=ImageDraw.Draw(canvas)
for row,d in enumerate(DIRS):
 folder=HERE/'jobs'/f'{char}-{d}-{ACTION_VERSION}';receipt=read(folder/'video.json');reader=imageio_ffmpeg.read_frames(str(PROJECT/receipt['fileRef']),pix_fmt='rgb24');info=next(reader)
 for j,b in enumerate(reader):
  if j not in indices:continue
  col=indices.index(j);im=Image.frombytes('RGB',info['size'],b).resize((112,112));canvas.paste(im,(col*112,row*132));draw.text((col*112+2,row*132+114),f'{d} {j} {j/24:.2f}',fill='white')
out=HERE/'reviews'/f'{char}-dense-reactions.jpg';canvas.save(out,quality=94);emit({'review':str(out)})
