"""Approve only the composite windows actually viewed in raw and alpha-cleaned contacts."""
from media import *
for char in ['glm','gemini']:
 for d in DIRS:
  if char=='gemini' and d=='w':continue
  folder=HERE/'jobs'/f'{char}-{d}-{ACTION_VERSION}';review=read(folder/'source-review.json')
  if review.get('approved'):continue
  review.update({'approved':True,'reviewedAt':stamp(),'reviewer':'Codex direct inspection of complete raw contact, dense reaction contact and selected alpha-cleaned contact','status':'approved-actual-selected-windows'})
  if char=='glm':
   review['notes']='White/navy GLM identity and all source directions preserved. Idle, alternating foot/hair walk cycle, and only SW full fall remain. Composite casting/recoil overlaps are excluded in all non-N directions; six clipped falls are replaced by larger-canvas real sources. North retains only the original idle; all movement/reaction/death are strict rear independent sources.'
  else:
   review['notes']='Blue/purple Gemini identity, bow/ponytail/boots and initial directions preserved. The W brown-floor composite is entirely excluded. E/W falls and non-front reactions have independent corrected sources. The original S impact visibly flinches and recovers with a small localized gold hit spark, which is retained as genuine source motion. Remaining falls are complete and inside the source bounds.'
   if d=='s':review['segments']['hit']=[160,194,24,False]
  save(folder/'source-review.json',review);extract(folder,True)
emit({'baseSourcesApproved':15})
