"""Approve viewed DeepSeek independent gesture sources, preserving proper directional identity."""
from media import *
overrides=read(HERE/'action-overrides.json')
for d in DIRS:
 for action in ['attack','cast']:
  folder=HERE/'jobs'/overrides.get(f'deepseek/{d}/{action}',f'deepseek-{d}-{action}-v1');receipt=read(folder/'video.json')
  notes='Viewed full eight-direction raw source contact for this action, retaining blue maid hair/headdress/whale tail and original footprint. '
  notes+=('Compact palm/forearm motion and tail counterbalance, with at most a small localized blue pulse; no invented physical weapon or changed background.' if action=='attack' else 'Wider arm gesture than ordinary pulse and complete return. Strict north retains the separately reviewed broad data-wave motion and no visible face; other directions have clean body movement with external runtime VFX.')
  save(folder/'source-review.json',{'id':folder.name,'approved':True,'reviewer':'Codex visual inspection of deepseek-attack-current.jpg/deepseek-cast-current.jpg and dense north source contacts','reviewedAt':stamp(),'notes':notes,'segments':{action:[0,100 if '-body-' in folder.name else 116,24 if action=='cast' else 16,False]},'videoSha256':receipt['sha256']});extract(folder,True)
emit({'deepseekGestureSourcesApproved':16})
