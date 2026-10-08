"""Apply the independent visual auditor's exact observed Gemini flinch/recovery windows."""
from media import *
windows={'e':[10,44],'ne':[0,38],'nw':[12,46],'se':[4,44],'sw':[8,46]}
for d,(start,end) in windows.items():
 folder=resolve_source('gemini',d,'hit',read(HERE/'action-overrides.json'));receipt=read(folder/'video.json')
 if folder.name!=f'gemini-{d}-hit-body-v1':raise RuntimeError('Reviewed source changed: '+folder.name)
 if (folder/'source-review.json').exists() and read(folder/'source-review.json').get('approved'):continue
 notes='Independent visual audit inspected the complete original contact and dense actual MP4 frames, confirming384x384/124frames/24fps and source SHA. The first response visibly lowers the chin, pulls shoulders/elbows inward and then fully recovers. Initial body heading remains fixed, including SE/SW where a bowed head briefly hides the face. No new objects, particles, rays, body truncation or dirty scene. Window boundaries have about2 source frames of visual timing tolerance.'
 save(folder/'source-review.json',{'id':folder.name,'approved':True,'reviewedAt':stamp(),'reviewer':'Codex independent read-only asset_contract_audit, full RGB and dense-frame visual inspection','notes':notes,'segments':{'hit':[start,end,24,False]},'videoSha256':receipt['sha256']});extract(folder,True)
emit({'geminiIndependentReactionsApproved':5})
