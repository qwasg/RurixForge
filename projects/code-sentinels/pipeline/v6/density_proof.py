"""Measured real512 capture: old full-frame preview versus native-crop delivery at equal world size."""
from media import *
from density_pack import clip_layout,make_tile
id='gemini-w-death-centered-v1';folder=HERE/'jobs'/id;spec=read(folder/'request.json');meta=read(folder/'extracted.json');clip=meta['clips']['death'];indices=np.linspace(clip['start'],clip['endExclusive']-1,10).round().astype(int).tolist()
images=[Image.open(folder/'frames-native'/f'{i:03}.png').convert('RGBA') for i in indices];foot=spec['sourceFootAnchor'];layout=clip_layout(spec,meta['actualSourceSize'],foot,[im.getbbox() for im in images]);native=images[0];legacy=Image.open(folder/'frames'/f'{indices[0]:03}.png').convert('RGBA');corrected=make_tile(native,layout)
def extent(im):
 a=np.asarray(im)[:,:,3];y,x=np.where(a>=128);return [int(x.max()-x.min()+1),int(y.max()-y.min()+1)]
result={'sourceJob':id,'actualSourceSize':list(native.size),'sourceBodyPixels':extent(native),'oldWholeFramePreviewBodyPixels':extent(legacy),'correctedCropBodyPixels':extent(corrected),'oldDeliveredPixelsPerWorldUnit':256/spec['outputPlaneSpan'],'correctedPixelsPerWorldUnit':layout['pixelsPerWorldUnit'],'standard384SourceBaselinePixelsPerWorldUnit':256/2.5456,'oldRelativeToBaseline':(256/spec['outputPlaneSpan'])/(256/2.5456),'newRelativeToBaseline':layout['pixelsPerWorldUnit']/(256/2.5456),'layout':layout,'deliveryDescription':'Fixed2/3 source sampling matches the384-source delivery baseline; this is not1:1 original-pixel delivery.'}
canvas=Image.new('RGB',(1350,430),(33,43,50));draw=ImageDraw.Draw(canvas);items=[(native,[foot[0]/512,foot[1]/512],spec['outputPlaneSpan'],'Original native capture'),(legacy,[foot[0]/512,foot[1]/512],spec['outputPlaneSpan'],'Old full512 to256'),(corrected,layout['pivot'],layout['nativePlaneSpan'],'Corrected native crop')]
for col,(im,pivot,span,label) in enumerate(items):
 size=round(span*128);scaled=im.resize((size,size),Image.Resampling.LANCZOS);px=col*450;cell=Image.new('RGBA',(450,380));cell.paste(scaled,(round(225-pivot[0]*size),round(330-pivot[1]*size)));canvas.paste(cell,(px,0),cell);draw.line((px+217,330,px+233,330),fill=(208,125,80));draw.text((px+12,385),label+' body height='+str(extent(im)[1])+'px',fill='white')
out=HERE/'reviews/pixel-density-proof.jpg';canvas.save(out,quality=96);save(HERE/'pixel-density-proof.json',result);emit(result)
