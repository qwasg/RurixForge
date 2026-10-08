"""Approve the four selected clean actual idle/stride windows after raw and alpha review."""
from media import *
for id,proposal in read(HERE/'reviews/gpt-physical-selected.json').items():
 folder=HERE/'jobs'/id
 if (folder/'source-review.json').exists() and read(folder/'source-review.json').get('approved'):continue
 note=('The complete actual source and selected keyed frames were viewed. This is a complete alternating-foot step cycle with natural opposite arm swing, retained initial direction, and a clean green backdrop.' if 'walk' in id else 'Only the explicitly viewed quiet body-breathing window is used. Earlier/later generated orbit or sparkle phases are excluded by source time selection, never painted away. The selected original pixels have a clean backdrop and preserve the reference posture.')
 save(folder/'source-review.json',{'id':id,'approved':True,'reviewedAt':stamp(),'reviewer':'Codex direct inspection of full raw source contact and gpt-physical-selected.jpg','notes':note,'segments':proposal['segments'],'videoSha256':proposal['videoSha256']});extract(folder,True)
emit({'gptPhysicalSourcesApproved':4})
