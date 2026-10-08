"""Flag raw source body-edge contacts during proposed death windows for visual followup."""
from media import *
char=sys.argv[1];results=[];overrides=read(HERE/'action-overrides.json')
for d in DIRS:
 if d=='n':continue
 folder=resolve_source(char,d,'death',overrides)
 if not (folder/'video.json').exists():results.append({'direction':d,'sourceJob':folder.name,'pending':True});continue
 spec=read(folder/'request.json');receipt=read(folder/'video.json');reader=imageio_ffmpeg.read_frames(str(PROJECT/receipt['fileRef']),pix_fmt='rgb24');info=next(reader);bad=[];count=0;check_start=195 if spec.get('frames',124)>124 and spec.get('action')!='physical-states' else 0
 for i,raw in enumerate(reader):
  count+=1
  if i<check_start:continue
  im=Image.frombytes('RGB',info['size'],raw);alpha=np.asarray(key_green(im))[:,:,3];edge=np.concatenate([alpha[0],alpha[-1],alpha[:,0],alpha[:,-1]])
  if int((edge>150).sum())>2:bad.append(i)
 results.append({'direction':d,'sourceJob':folder.name,'sourceFrames':count,'checkedFromFrame':check_start,'opaqueEdgeFrames':bad})
save(HERE/'reviews'/f'{char}-raw-death-edge-flags.json',{'results':results,'purpose':'flags need visual interpretation; never silently accepts raw truncation'});emit(results)
