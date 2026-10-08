"""Approve the six GPT base sources actually inspected; rejected action states stay excluded."""
from media import *
for d in ['sw','w','n','ne','e','se']:
 folder=HERE/'jobs'/f'gpt-{d}-{ACTION_VERSION}';review=read(folder/'source-review.json')
 if review.get('approved'):continue
 review.update({'approved':True,'reviewedAt':stamp(),'reviewer':'Codex direct inspection of gpt-actions-v2.jpg and gpt-base-clean.jpg','status':'approved-actual-selected-windows','notes':'Retains only clean idle and actual alternating-foot walk in the original directions, plus complete in-bounds NE/SE falls. All composite hit segments, five truncated/polluted falls, and both particle-contaminated S/NW base sources are excluded and replaced with dedicated real keyed motion. White/lavender hair, silhouette and game-adapted boots stay consistent.'})
 save(folder/'source-review.json',review);extract(folder,True)
emit({'gptBaseSourcesApproved':6})
