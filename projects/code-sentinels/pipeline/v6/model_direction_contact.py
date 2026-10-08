"""Contact sheet of unmodified real Blender bakes for geometry/direction review."""
from media import *
id=sys.argv[1];canvas=Image.new('RGB',(1024,552),(34,42,48));draw=ImageDraw.Draw(canvas)
for i,d in enumerate(DIRS):
 im=Image.open(PROJECT/'Content/UI/v6/model-bakes'/id/f'{d}.png').convert('RGBA').resize((256,256),Image.Resampling.LANCZOS);x=i%4*256;y=i//4*276;canvas.paste(im,(x,y),im);draw.text((x+5,y+258),f'{id} {d}',fill='white')
out=HERE/'reviews'/f'{id}-model-directions.jpg';canvas.save(out,quality=94);emit({'review':str(out)})
