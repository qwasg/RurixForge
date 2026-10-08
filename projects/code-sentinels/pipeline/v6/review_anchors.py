"""Diagnostic-only reference contact sheet; does not edit source or production images."""
from media import *
canvas=Image.new('RGB',(8*128,7*150),(25,30,35));draw=ImageDraw.Draw(canvas);anchors={}
for row,char in enumerate(CHARACTERS):
 anchors[char]={}
 for col,d in enumerate(DIRS):
  path=HERE/'first-frames'/f'{char}-{d}.png';im=Image.open(path).convert('RGB');rgba=key_green(im);a=np.asarray(rgba)[:,:,3];ys,xs=np.where(a>180);bottom=int(ys.max());feet_xs=xs[ys>=bottom-8];foot=(float(feet_xs.min()+feet_xs.max())/2,float(bottom+1))
  anchors[char][d]={'foot':foot,'normalized':[foot[0]/384,foot[1]/384],'method':'bounding midpoint of solid-alpha pixels in lowest9 reference rows; visual review required'}
  canvas.paste(im.resize((128,128)),(col*128,row*150));cx=col*128+foot[0]/3;cy=row*150+foot[1]/3
  draw.line((cx-5,cy,cx+5,cy),fill=(255,70,70),width=1);draw.line((cx,cy-5,cx,cy+5),fill=(255,70,70),width=1)
  draw.text((col*128+2,row*150+130),char+' '+d,fill='white')
out=HERE/'reviews/reference-anchors.jpg';canvas.save(out,quality=92);save(out.with_suffix('.json'),anchors);emit({'review':str(out)})
