"""Approve actually viewed complete corrected poses from the previous queued batch."""
from media import *
jobs={
 'gemini-w-idle-body-v1':('idle',[0,124,18,True],'Strict left-profile quiet breathing/hair cycle returns to supplied reference; fixed green backdrop throughout.'),
 'gemini-w-walk-body-v1':('walk',[16,32,16,True],'Clear continuous alternating-foot strides; selected actual complete cycle fromabout0.67 to1.33 seconds preserves profile and in-place center.'),
 'gemini-w-hit-body-v1':('hit',[22,84,24,False],'Physical shoulder/chin contraction begins around1.1 seconds and returns fully upright by3.33; all selected frames are clean and complete.'),
 'gemini-ne-attack-body-v2':('attack',[18,88,32,False],'Actual compact two-arm extension and full return, preserving rear-right heading; large generated rings from the rejected version are absent.'),
 'kimi-s-hit-body-v2':('hit',[12,84,24,False],'Front-facing physical flinch and actual upright recovery by3.1 seconds; old magenta backdrop is absent.'),
 'glm-s-death-centered-v1':('death',[8,56,24,False],'Front girl kneels and lowers to a complete prone rest byabout1.3 seconds; original entire hair/body/clothing stay inside512 capture.')}
for id,(action,segment,note) in jobs.items():
 folder=HERE/'jobs'/id;receipt=read(folder/'video.json')
 if (folder/'source-review.json').exists() and read(folder/'source-review.json').get('approved'):continue
 save(folder/'source-review.json',{'id':id,'approved':True,'reviewedAt':stamp(),'reviewer':'Codex complete24-sample originalRGB contact inspection','notes':note,'segments':{action:segment},'videoSha256':receipt['sha256']});extract(folder,True)
emit({'resumeRepairSourcesApproved':len(jobs)})
