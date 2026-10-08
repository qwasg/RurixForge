"""Visual contact of actual model component motion, without altering source frames."""
from media import *
id,action=sys.argv[1:3];folder=PROJECT/'Content/UI/v6'/('model-attacks' if action=='attack' else 'model-work')/id
meta=read(folder/(action+'.json'));indices=[0,2,5,11] if action=='attack' else [0,3,6,9];canvas=Image.new('RGB',(4*192,8*212),(34,42,48));draw=ImageDraw.Draw(canvas)
for row,d in enumerate(DIRS):
 for col,i in enumerate(indices):
  im=Image.open(folder/f'{d}-{i:02}.png').convert('RGBA').resize((192,192),Image.Resampling.LANCZOS);canvas.paste(im,(col*192,row*212),im);draw.text((col*192+4,row*212+194),f'{id} {d} {i}',fill='white')
out=HERE/'reviews'/f'{id}-{action}-model.jpg';canvas.save(out,quality=94);emit({'review':str(out)})
