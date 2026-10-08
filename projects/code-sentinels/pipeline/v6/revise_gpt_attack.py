from media import *
old=HERE/'jobs/gpt-n-attack-v2';receipt=read(old/'video.json')
save(old/'source-review.json',{'id':old.name,'approved':False,'reviewedAt':stamp(),'reason':'Full-source contact shows GPT turning torso/head toward side during one-arm shot, so it is not accepted as strict rear-facing attack.','videoSha256':receipt['sha256'],'replacement':'gpt-n-attack-v3'})
folder=HERE/'jobs/gpt-n-attack-v3';spec=read(old/'request.json');spec['id']=folder.name
spec['prompt']=('STRICT SYMMETRIC REAR VIEW of the EXACT white-haired purple-winged dragon game character in this first frame. Her spine stays vertical and symmetrical on the screen center line. '
 'Only the back of the head, long white hair, horns and backs of both wings are visible. NEVER expose any cheek, eye, nose, face, chest or side profile, even briefly. '
 'Perform ONE compact basic shot using BOTH arms symmetrically: move both elbows out a little, push both hands straight AWAY from the viewer along the center line, pulse once, then return hands to the starting pose. '
 'Her torso and head stay perfectly rear-facing and symmetric; do NOT twist the waist or shoulder, do NOT turn to demonstrate the hands. The shot travels AWAY from viewer, not sideways. '
 'This is a small quick basic attack, no large circle, spell aura, elaborate raising of arms or recovery animation beyond returning idle. Start immediately, finish the thrust within two seconds, then hold calm rear-facing idle. '
 'Keep same source size and foot location, full silhouette within central two-thirds. Fixed elevated orthographic camera, no zoom/pan, pure flat chroma GREEN RGB0,255,0. No environment, text, other people, changing costume, walking or falling.')
spec['priorityFront']=True;spec['revisionReason']='Observed side-turn in v2 is replaced by an explicitly symmetric two-arm rear shot; v2 successful video/receipt retained.'
save(folder/'request.json',spec);submit(folder);overrides=read(HERE/'action-overrides.json');overrides['gpt/n/attack']=folder.name;save(HERE/'action-overrides.json',overrides);status()
