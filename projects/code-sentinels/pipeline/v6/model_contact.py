from media import *
paths=sorted((PROJECT/'Content/UI/v6/model-bakes').glob('*/bake.json'));cols=10;w=144;h=170
canvas=Image.new('RGB',(cols*w,math.ceil(len(paths)/cols)*h),(28,33,38));draw=ImageDraw.Draw(canvas)
for i,path in enumerate(paths):
 info=read(path);d='ne' if info['category']=='module' else 'se'
 im=Image.open(path.parent/(d+'.png')).convert('RGBA').resize((w,w),Image.Resampling.LANCZOS);x=i%cols*w;y=i//cols*h;canvas.paste(im,(x,y),im)
 draw.text((x+3,y+w+3),path.parent.name[:23],fill=(215,220,220))
out=HERE/'reviews/models-overview.jpg';out.parent.mkdir(exist_ok=True);canvas.save(out,quality=92);emit({'contact':str(out),'models':len(paths)})
