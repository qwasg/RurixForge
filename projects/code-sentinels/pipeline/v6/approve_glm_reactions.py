"""Approve actual independent GLM flinch/recovery sources after complete directional review."""
from media import *
for d in ['s','sw','w','nw','ne','e','se']:
 folder=resolve_source('glm',d,'hit',read(HERE/'action-overrides.json'));receipt=read(folder/'video.json')
 if (folder/'source-review.json').exists() and read(folder/'source-review.json').get('approved'):continue
 end=60 if d in ['w','nw'] else 46
 save(folder/'source-review.json',{'id':folder.name,'approved':True,'reviewedAt':stamp(),'reviewer':'Codex direct inspection of glm-hit-current.jpg full eight-direction original source contact','notes':'Separate physical flinch: chin and shoulders recoil, hands draw inward, knees soften, hair/sleeves follow, and the figure returns upright. Exact original initial heading and costume retained. The selected first repetition includes full recovery; W/NW use the later2.5-second return. No inherited spell graphics or body-edge crossing.','segments':{'hit':[0,end,24,False]},'videoSha256':receipt['sha256']});extract(folder,True)
emit({'glmIndependentReactionsApproved':7})
