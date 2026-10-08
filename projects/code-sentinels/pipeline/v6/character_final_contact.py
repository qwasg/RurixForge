from media import *
char=sys.argv[1];directory=HERE/'density-staging' if '--staging' in sys.argv else ANIM;meta=read(directory/f'{char}.json');atlas=Image.open(directory/f'{char}.png').convert('RGBA');actions=list(SEGMENTS)
# Use a shared world-space scale and enough space for every actual clip plane,
# including broad falls. A QA cell must not introduce apparent source clipping.
layouts=[(c.get('pivot',meta['pivot']),round(160*c.get('nativePlaneSpan',meta['nativePlaneSpan'])/2.5456)) for dirs in meta['clips'].values() for c in dirs.values()]
left=math.ceil(max(p[0]*e for p,e in layouts))+6;right=math.ceil(max((1-p[0])*e for p,e in layouts))+6;top=math.ceil(max(p[1]*e for p,e in layouts))+6;bottom=math.ceil(max((1-p[1])*e for p,e in layouts))+6
cw=left+right;ch=top+bottom+22;canvas=Image.new('RGB',(12*cw,8*ch+38),(35,48,54));draw=ImageDraw.Draw(canvas);draw.text((5,4),f'CURRENT PUBLISHED METADATA: {char} | atlas SHA256 {sha(directory/f"{char}.png")} | offline QA, not game screenshot',fill='white')
for row,d in enumerate(DIRS):
 for column,action in enumerate(actions):
  clip=meta['clips'][action][d];chosen=[clip['start'],clip['endExclusive']-1 if action in ['hit','death'] else (clip['start']+clip['endExclusive'])//2]
  for sub,index in enumerate(chosen):
   x,y,w,h=meta['boxes'][index];pivot=clip.get('pivot',meta['pivot']);span=clip.get('nativePlaneSpan',meta['nativePlaneSpan']);extent=round(160*span/2.5456)
   tile=atlas.crop((x,y,x+w,y+h)).resize((extent,extent),Image.Resampling.LANCZOS);px=(column*2+sub)*cw;py=row*ch+38
   cell=Image.new('RGBA',(cw,ch-22));cell.paste(tile,(round(left-pivot[0]*extent),round(top-pivot[1]*extent)));canvas.paste(cell,(px,py),cell)
   draw.line((px+left-3,py+top,px+left+3,py+top),fill=(190,100,85));draw.text((px+2,py+ch-18),f'{d} {action} / frame {index}',fill='white')
out=HERE/'reviews'/f'{char}{"-staged-density" if "--staging" in sys.argv else ""}-final-atlas.jpg';canvas.save(out,quality=94);emit({'preview':str(out),'actualFrames':meta['frameCount'],'previewSpace':'fixed world size with actual per-clip span/pivot, offline QA rather than a game screenshot'})
