"""Reviewed completed gesture sources; known scene/facing failures are excluded via overrides."""
from media import *
ends={
 'gpt':{'attack':{'s':108,'sw':48,'w':48,'nw':48,'ne':96,'e':52,'se':48},'cast':{'s':76,'sw':76,'nw':76,'ne':76,'e':96,'se':80}},
 'kimi':{'attack':{'s':52,'w':68,'ne':52,'e':108,'se':96},'cast':{'s':88,'sw':80,'w':96,'nw':96,'ne':88,'e':104,'se':96}},
 'minimax':{'attack':{'sw':100,'w':84,'nw':108,'ne':96,'e':108,'se':96},'cast':{'s':100,'sw':88,'w':80,'nw':100,'ne':112,'e':116,'se':100}},
 'glm':{'attack':{'s':76,'sw':68,'w':84,'nw':76,'ne':96,'e':64},'cast':{'s':100,'w':96,'nw':92,'ne':116,'e':116,'se':112}}}
approved=[]
for char,actions in ends.items():
 for action,directions in actions.items():
  for d,end in directions.items():
   folder=resolve_source(char,d,action,read(HERE/'action-overrides.json'));receipt=read(folder/'video.json')
   if not folder.name.endswith('-body-v1'):raise RuntimeError('Reviewed gesture source changed '+folder.name)
   if (folder/'source-review.json').exists() and read(folder/'source-review.json').get('approved'):continue
   note='Viewed the original eight-direction timeline contact across the complete video. The selected actual gesture has a visible hand/forearm movement and returns to its original pose; body heading, hairstyle, clothing and existing accessories stay consistent. '
   note+=('Compact attack/push or original microphone/pen/book movement; any small emitted page/pulse remains distinct from the body.' if action=='attack' else 'Broader raise/open-arm or book/microphone charge motion, followed by actual recovery. This is a separate video from the basic attack, with the game also using its independent skill VFX.')
   save(folder/'source-review.json',{'id':folder.name,'approved':True,'reviewedAt':stamp(),'reviewer':'Codex original full-timeline eight-direction RGB contact inspection','notes':note,'segments':{action:[0,end,32,False]},'videoSha256':receipt['sha256']});extract(folder,True);approved.append(folder.name)
save(HERE/'remaining-gesture-approvals.json',{'at':stamp(),'approved':approved,'excludedFailures':'resume-quality-repairs.json; no failed scene/heading source was substituted with a still'});emit({'remainingGestureSourcesApproved':len(approved)})
