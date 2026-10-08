"""Compact actual generation/review readiness, resolving every action override."""
from media import *
overrides=read(HERE/'action-overrides.json');result={}
for char in CHARACTERS:
 rows={}
 for action in SEGMENTS:
  states={'video':[],'approved':[],'pendingVideo':[],'pendingReview':[]}
  for d in DIRS:
   folder=resolve_source(char,d,action,overrides)
   exists=(folder/'video.json').exists();approved=(folder/'source-review.json').exists() and read(folder/'source-review.json').get('approved') and action in read(folder/'source-review.json').get('segments',{})
   if exists:states['video'].append(d)
   else:states['pendingVideo'].append(d)
   if approved:states['approved'].append(d)
   elif exists:states['pendingReview'].append(d)
  rows[action]=states
 result[char]=rows
save(HERE/'production-inventory.json',{'at':stamp(),'characters':result})
emit({c:{a:{'video':len(s['video']),'approved':len(s['approved']),'missing':s['pendingVideo']} for a,s in actions.items()} for c,actions in result.items()})
