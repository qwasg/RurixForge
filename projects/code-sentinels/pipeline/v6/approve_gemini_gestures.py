"""Approve complete inspected independent Gemini gestures, excluding the noisy NE basic pulse."""
from media import *
overrides=read(HERE/'action-overrides.json')
for d in [d for d in DIRS if d!='n']:
 for action in ['attack','cast']:
  if d=='ne' and action=='attack':continue
  folder=resolve_source('gemini',d,action,overrides);receipt=read(folder/'video.json')
  if (folder/'source-review.json').exists() and read(folder/'source-review.json').get('approved'):continue
  end=({'s':48,'sw':92,'w':76,'nw':92,'e':54,'se':94}[d] if action=='attack' else 112)
  notes='Viewed all original direction contact frames. Exact twin-star blue/purple identity, initial body heading and original screen scale retained. '
  notes+=('Compact physical forearm/finger extension and actual return; no invented physical weapon or scene change.' if action=='attack' else 'Larger hand/arm charge motion and full neutral return distinct from the short pulse source. Front view has two genuine lift-and-crouch charge motions; other directions use the raised hand and extended forearm. Background remains keyed and complete body stays visible.')
  save(folder/'source-review.json',{'id':folder.name,'approved':True,'reviewedAt':stamp(),'reviewer':'Codex direct full original gemini-attack-current/gemini-cast-current contact inspection','notes':notes,'segments':{action:[0,end,32,False]},'videoSha256':receipt['sha256']});extract(folder,True)
emit({'geminiNewGestureSourcesApproved':13})
