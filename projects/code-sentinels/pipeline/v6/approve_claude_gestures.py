"""Claude eight-direction independent gestures and corrected SE recoil viewed in full contacts."""
from media import *
overrides=read(HERE/'action-overrides.json')
for d in DIRS:
 for action in ['attack','cast']:
  folder=HERE/'jobs'/overrides.get(f'claude/{d}/{action}',f'claude-{d}-{action}-v1');receipt=read(folder/'video.json')
  note=('Viewed all eight actual directional source videos for this action. '+('Compact forearm/palm push and retraction; original face/hair/costume retained, no extra weapon or effect background.' if action=='attack' else 'Broader open-arm barrier posture and return, visibly different from compact shot; no camera scale jump or changed backdrop.')+' The source begins and finishes in the intended directional pose; strict north face remains hidden.')
  review={'id':folder.name,'approved':True,'reviewer':'Codex visual inspection of claude-attack-current.jpg / claude-cast-current.jpg plus dense north contacts','reviewedAt':stamp(),'notes':note,'segments':{action:[0,100 if '-body-' in folder.name else 116,24 if action=='cast' else 16,False]},'videoSha256':receipt['sha256']}
  save(folder/'source-review.json',review);extract(folder,True)
folder=HERE/'jobs/claude-se-hit-body-v1';receipt=read(folder/'video.json')
save(folder/'source-review.json',{'id':folder.name,'approved':True,'reviewer':'Codex visual inspection of complete24-frame original source contact','reviewedAt':stamp(),'notes':'Corrected physical SE shoulder/head flinch with no ink ring or background graphics. Source reaction is delayed: actual1.25-4.5s interval includes onset, bend and complete recovery.','segments':{'hit':[30,108,20,False]},'videoSha256':receipt['sha256']});extract(folder,True)
emit({'claudeGestureSourcesApproved':17})
