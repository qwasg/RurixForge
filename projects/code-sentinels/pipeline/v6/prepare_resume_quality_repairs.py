"""Six visually rejected sources get new requests; successful provider tasks are never replayed."""
from media import *
repairs=[
 ('gpt','w','cast','gpt-w-cast-body-v1','left-side profile continuously facing screen left','Raise both hands smoothly above the shoulders with open palms, hold briefly, then lower both hands to the exact original relaxed pose.'),
 ('kimi','sw','attack','kimi-sw-attack-body-v1','front-left three-quarter view continuously facing lower-left','Extend the original pen-holding forearm a short distance forward, then retract it. Keep the original small book close to the torso.'),
 ('kimi','nw','attack','kimi-nw-attack-body-v1','rear-left three-quarter view continuously facing upper-left, with the back of her head visible','Make one short forward forearm extension at chest level with the original tiny pen, then retract. Keep the original book against the torso.'),
 ('minimax','s','attack','minimax-s-attack-body-v1','front view continuously facing the viewer','Lift the original pink microphone slightly toward the mouth, extend the free hand a short distance forward, then return both hands to their exact initial positions.'),
 ('glm','sw','cast','glm-sw-cast-body-v1','front-left three-quarter view continuously facing lower-left','Open the original book at chest height, raise the free open palm just above the book, hold briefly, then close the book and lower the hand back to the original pose.'),
 ('glm','se','attack','glm-se-attack-body-v1','front-right three-quarter view continuously facing lower-right','Only the original pen-holding forearm moves: extend it forward a short distance, then retract. Head, torso and feet retain their original heading throughout.')]
overrides=read(HERE/'action-overrides.json');records=[]
for char,d,action,old_id,facing,motion in repairs:
 old=HERE/'jobs'/old_id;receipt=read(old/'video.json');id=f'{char}-{d}-{action}-greenstage-v2';folder=HERE/'jobs'/id
 if not (folder/'request.json').exists():
  spec=read(old/'request.json');spec['id']=id;spec['priorityFront']=False
  spec['prompt']=(f'A single physical movement of the exact illustrated person on the SAME BRIGHT GREEN CANVAS as the reference. Every background pixel remains saturated RGB0,255,0 at unchanged brightness for the entire video. Keep the {facing}. '+motion+
   ' Start at half a second, complete the movement and return by three seconds, then remain in the original pose. Only the named arm and hand movement, and natural small hair/fabric responses, occur. Preserve the exact face, hair, clothes, body proportions and existing accessories. Her outline changes naturally with the moving arms. The feet remain on the original marks and the camera stays fixed elevated orthographic at the original distance. The entire background remains the same flat pure green as the supplied image, including the corners and all empty space around her.')
  spec['lastFrame']=spec['firstFrame'];spec['revisionReason']='Original completed source was visually rejected for opaque changing backdrop during the required gesture.' if not (char=='glm' and d=='se') else 'Original attack turns its back to the viewer during the gesture instead of preserving the front-right direction.'
  save(folder/'request.json',spec)
 if (old/'source-review.json').exists() and not (old/'source-review-before-resume-repair.json').exists():shutil.copy2(old/'source-review.json',old/'source-review-before-resume-repair.json')
 save(old/'source-review.json',{'id':old_id,'approved':False,'reviewedAt':stamp(),'videoSha256':receipt['sha256'],'reason':read(folder/'request.json')['revisionReason'],'replacement':id,'providerSucceededButVisualQualityRejected':True})
 overrides[f'{char}/{d}/{action}']=id;records.append({'old':old_id,'new':id,'submitted':(folder/'attempt.json').exists()})
save(HERE/'action-overrides.json',overrides);save(HERE/'resume-quality-repairs.json',{'at':stamp(),'repairs':records,'priorReceiptsDeleted':False,'successfulPromptIdsReplayed':False});emit({'preparedQualityRepairRequests':len(records)})
