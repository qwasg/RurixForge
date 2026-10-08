"""Approve explicitly viewed genuine independent body flinches, with complete recovery windows."""
from media import *
for d in ['s','sw','w','nw','ne','e','se']:
 folder=resolve_source('gpt',d,'hit',read(HERE/'action-overrides.json'));receipt=read(folder/'video.json')
 if (folder/'source-review.json').exists() and read(folder/'source-review.json').get('approved'):continue
 end=110 if d in ['ne','e'] else 44
 notes='Viewed complete eight-direction physical hit contact and original first/end poses. Character tucks chin/shoulders and softens knees with sleeves/hair following, then returns upright. Exact initial direction retained, no generated effects, background plane or extra props. '
 notes+=('This direction holds the flinch longer; retain through4.58 seconds so the output ends after actual upright recovery.' if end==110 else 'The first repetition completes its recovery by1.67 seconds, so only the first1.83-second physical response is used.')
 save(folder/'source-review.json',{'id':folder.name,'approved':True,'reviewer':'Codex direct inspection of gpt-hit-current.jpg','reviewedAt':stamp(),'notes':notes,'segments':{'hit':[0,end,24,False]},'videoSha256':receipt['sha256']});extract(folder,True)
emit({'gptIndependentReactionsApproved':7})
