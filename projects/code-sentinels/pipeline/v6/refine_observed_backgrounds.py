"""Two specifically observed source scene failures get new local IDs, preserving old videos."""
from media import *
jobs=[
 ('kimi-s-hit-body-v1','kimi-s-hit-body-v2',
  'A saturated flat GREEN RGB0,255,0 canvas stays exactly unchanged on every frame. On it, the pictured small blue-haired woman faces directly toward the viewer. She makes one quick physical startle: close the eyes, pull elbows and hands inward toward her chest, lower the chin and bend both knees slightly. Then straighten the knees and return fully to her original upright relaxed pose. Begin this movement at half a second, complete the recovery by two seconds, and then stand quietly. Only her body, clothes and hair move. Preserve her exact costume, face, hair, tiny existing book and pen. Fixed elevated orthographic camera at the original distance and body size. The entire background is the same pure green color as the supplied reference throughout the video.'),
 ('minimax-nw-physical-states-v3','minimax-nw-physical-states-v4',
  'Use the exact bright saturated GREEN RGB0,255,0 canvas from the supplied reference on EVERY FRAME. Its brightness and color remain constant for the whole video. The single small pink-and-white singer stays in the original rear-left three-quarter view and at the original size. 0-1 second: stand quietly. 1-4 seconds: take four clear alternating steps in place, visibly lifting each knee/foot and swinging the opposite arm. 4-6 seconds: close the eyes and recoil the shoulders with hands pulled inward, then return fully upright. 6-9 seconds: lower onto the knees, then lower the torso to lie completely on the side, and remain lying. Preserve her exact dark pink hair, pink/white outfit and existing small pink microphone. The fixed elevated orthographic camera observes only these physical body, clothing and hair movements. The empty areas around her are the identical featureless pure green of the reference at all times. Keep every body part inside the image.')]
overrides=read(HERE/'action-overrides.json')
for old_id,id,prompt in jobs:
 old=HERE/'jobs'/old_id;folder=HERE/'jobs'/id;receipt=read(old/'video.json')
 save(old/'source-review.json',{'id':old_id,'approved':False,'reviewedAt':stamp(),'reason':'Directly viewed raw source changes its backdrop to opaque magenta during the main reaction.' if old_id.startswith('kimi') else 'Directly viewed raw source changes the initial green backdrop to opaque black for almost the entire motion, unsuitable for preserving dark hair with the keyed character pipeline.','videoSha256':receipt['sha256'],'replacement':id})
 if not (folder/'attempt.json').exists():
  spec=read(old/'request.json');spec['id']=id;spec['prompt']=prompt;spec['priorityFront']=True;spec['revisionReason']='Observed color-changing background in the actual completed video; new positive static-green stage instruction and physical choreography, same reference and spatial contract.';save(folder/'request.json',spec);submit(folder)
 spec=read(folder/'request.json')
 for action in spec['segments']:overrides[f'{spec["character"]}/{spec["direction"]}/{action}']=id
 save(HERE/'action-overrides.json',overrides)
status()
