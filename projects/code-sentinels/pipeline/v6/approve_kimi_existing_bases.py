"""Approve five existing Kimi base sources after raw, temporal and alpha inspection."""
from media import *
for d in ['s','nw','n','ne','e']:
 folder=HERE/'jobs'/f'kimi-{d}-{ACTION_VERSION}';review=read(folder/'source-review.json')
 if review.get('approved'):continue
 review.update({'approved':True,'reviewedAt':stamp(),'reviewer':'Codex direct inspection of kimi-actions-v2.jpg, kimi-base-windows.jpg and kimi-base-clean.jpg','status':'approved-actual-selected-windows','notes':'Original blue/white Kimi identity and source direction retained in actual breathing/alternating-foot walking and full in-bounds falling. Composite hit and all attack/cast windows are excluded. North retains only idle and its separately reviewed rear states. NW removes only the distant colored backdrop dots with the disclosed primary-component-bounds garbage matte; the original full body RGB/motion remains.'})
 save(folder/'source-review.json',review);extract(folder,True)
emit({'kimiExistingBaseSourcesApproved':5})
