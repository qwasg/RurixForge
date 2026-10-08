"""Approve viewed clean Kimi side/rear reactions; the failed front source is excluded."""
from media import *
for d,(start,end) in {'w':(0,44),'nw':(14,57),'ne':(20,60),'e':(56,101)}.items():
 folder=resolve_source('kimi',d,'hit',read(HERE/'action-overrides.json'));receipt=read(folder/'video.json')
 if (folder/'source-review.json').exists() and read(folder/'source-review.json').get('approved'):continue
 note='Complete original direction contact viewed. Clear chin/shoulder contraction, arm draw and full upright recovery, retaining the original profile/rear-three-quarter direction and all existing blue/white costume details. '
 note+=('Use the clean second physical repetition; the first repetition includes small generated white impact lines and is excluded.' if d=='e' else 'The selected first repetition includes its actual complete recovery and clean backdrop.')
 save(folder/'source-review.json',{'id':folder.name,'approved':True,'reviewedAt':stamp(),'reviewer':'Codex original RGB contact visual inspection','notes':note,'segments':{'hit':[start,end,24,False]},'videoSha256':receipt['sha256']});extract(folder,True)
emit({'kimiIndependentReactionsApproved':4})
