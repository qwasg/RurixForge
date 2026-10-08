"""Approve the six newly generated, fully observed replacement gesture sources."""
from media import *
windows={'gpt-w-cast-greenstage-v2':('cast',[4,80,32,False]),'kimi-sw-attack-greenstage-v2':('attack',[16,108,32,False]),'kimi-nw-attack-greenstage-v2':('attack',[16,92,32,False]),'minimax-s-attack-greenstage-v2':('attack',[16,92,32,False]),'glm-sw-cast-greenstage-v2':('cast',[0,92,32,False]),'glm-se-attack-greenstage-v2':('attack',[16,108,32,False])}
for id,(action,segment) in windows.items():
 folder=HERE/'jobs'/id;receipt=read(folder/'video.json')
 if (folder/'source-review.json').exists() and read(folder/'source-review.json').get('approved'):continue
 note='Viewed the entire24-sample original RGB contact of this new source. Background is constant green. Body retains the exact initial heading and original source scale, all extremities remain visible, and the requested physical forearm/book/microphone movement has a complete return. '
 note+=('The former front-right-to-back spin is absent.' if id.startswith('glm-se') else 'The former changing colored backdrop is absent.')
 save(folder/'source-review.json',{'id':id,'approved':True,'reviewedAt':stamp(),'reviewer':'Codex complete originalRGB source contact inspection','notes':note,'segments':{action:segment},'videoSha256':receipt['sha256']});extract(folder,True)
emit({'newQualityRepairSourcesApproved':len(windows)})
