"""Independently measure real MP4 first-frame body size across384/512 captures."""
from media import *
records=[];issues=[]
for char,d in [('glm','ne'),('gemini','e')]:
 measurements=[]
 for id in [f'{char}-{d}-{ACTION_VERSION}',f'{char}-{d}-death-centered-v1']:
  folder=HERE/'jobs'/id;spec=read(folder/'request.json');receipt=read(folder/'video.json');reader=imageio_ffmpeg.read_frames(str(PROJECT/receipt['fileRef']),pix_fmt='rgb24');info=next(reader);im=Image.frombytes('RGB',info['size'],next(reader));reader.close()
  alpha=np.asarray(key_green(im))[:,:,3];ys,xs=np.where(alpha>128);box=[int(xs.min()),int(ys.min()),int(xs.max()+1),int(ys.max()+1)]
  span=spec.get('outputPlaneSpan',2.5456);world_height=(box[3]-box[1])/info['size'][1]*span
  measurements.append({'sourceJob':id,'actualSourceSize':list(info['size']),'opaqueBodyBox':box,'worldHeight':world_height,'planeSpan':span,'firstFrameSha256':sha(folder/'first-frame.png'),'videoSha256':receipt['sha256']})
 ratio=measurements[1]['worldHeight']/measurements[0]['worldHeight']
 if not .975<=ratio<=1.025:issues.append(f'{char}/{d}: source actor world height drift {ratio:.4f}')
 records.append({'character':char,'direction':d,'measurements':measurements,'centeredVsStandingRatio':ratio})
save(HERE/'capture-scale-verification.json',{'at':stamp(),'checks':records,'issues':issues,'pass':not issues,'scope':'actual video first-frame world-size continuity, not native display acceptance'});emit({'checks':len(records),'issues':issues})
