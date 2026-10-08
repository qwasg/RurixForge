"""Extract exact original RGB candidate frames for human boundary classification; no source changes."""
from media import *
requests_to_view=[('gpt-e-death-centered-v1',[6,8,10,12]),('gpt-n-cast-v2',[15,25]),('glm-n-attack-v2',[23,38]),('kimi-nw-actions-v2',[8,15])]
out=HERE/'reviews/raw-edge-candidates';out.mkdir(parents=True,exist_ok=True);records=[]
for id,indices in requests_to_view:
 receipt=read(HERE/'jobs'/id/'video.json');reader=imageio_ffmpeg.read_frames(str(PROJECT/receipt['fileRef']),pix_fmt='rgb24');info=next(reader)
 for index,raw in enumerate(reader):
  if index not in indices:continue
  im=Image.frombytes('RGB',info['size'],raw);im.save(out/f'{id}-{index:03}.png');a=np.asarray(key_green(im))[:,:,3];y,x=np.where((a>=150)&((np.indices(a.shape)[0]==0)|(np.indices(a.shape)[0]==a.shape[0]-1)|(np.indices(a.shape)[1]==0)|(np.indices(a.shape)[1]==a.shape[1]-1)))
  records.append({'job':id,'sourceFrame':index,'opaqueBoundaryPixels':len(x),'coordinates':[[int(px),int(py)] for px,py in zip(x[:16],y[:16])]})
save(out/'index.json',records);emit(records)
