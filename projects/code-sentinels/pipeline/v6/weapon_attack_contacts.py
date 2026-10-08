"""Review all real weapon attack samples in two manageable contact sheets."""
from media import *
paths=sorted((PROJECT/'Content/UI/v6/model-attacks').glob('*/attack.json'))
for group in range(math.ceil(len(paths)/9)):
 subset=paths[group*9:(group+1)*9];canvas=Image.new('RGB',(4*192,len(subset)*212),(34,42,48));draw=ImageDraw.Draw(canvas)
 for row,path in enumerate(subset):
  meta=read(path)
  for col,i in enumerate([0,2,5,11]):
   im=Image.open(path.parent/f'se-{i:02}.png').convert('RGBA').resize((192,192),Image.Resampling.LANCZOS);canvas.paste(im,(col*192,row*212),im);draw.text((col*192+3,row*212+194),f'{meta["id"]} se {i}',fill='white')
 out=HERE/'reviews'/f'weapon-attack-group-{group+1}.jpg';canvas.save(out,quality=94);emit({'review':str(out)})
