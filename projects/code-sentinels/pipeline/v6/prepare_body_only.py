"""Known QA fixes: describe physical body motion, with spell visuals handled by actual FX atlases."""
from media import *
JOBS=[('gpt','attack','gpt-n-attack-v3'),('kimi','attack','kimi-n-attack-v2'),('kimi','cast','kimi-n-cast-v2'),('claude','cast','claude-n-cast-v2'),('gemini','attack','gemini-n-attack-v2')]
motions={
 ('gpt','attack'):'Perform one quick physical pushing exercise: bend BOTH elbows bringing palms near the waist, extend both forearms straight forward away from camera at lower-chest height, then return both hands to relaxed position. Keep the movement symmetric.',
 ('kimi','attack'):'Perform one brief arm-extension rehearsal. Keep the exact small pen and book already present, with no new objects. Extend the right forearm straight forward away from camera and retract it. The hand remains open or holds the same tiny reference pen; never introduce a blade or change its size.',
 ('kimi','cast'):'Perform one stationary physical lunge rehearsal: bend knees slightly, lean torso forward a little and extend both arms straight ahead away from camera, hold for half a second, then return upright. Feet stay at the original ground point; body screen scale never changes.',
 ('claude','cast'):'Perform one physical arm-stretch rehearsal: spread both upper arms outward to shoulder level, bend elbows with palms facing away, hold the symmetrical open-arm posture briefly, then lower both hands to neutral.',
 ('gemini','attack'):'Perform one brief double arm-extension rehearsal: lift both elbows slightly, point both hands straight ahead away from viewer at shoulder height, quickly extend then retract both forearms. Keep head and torso rear-facing.'}
overrides=read(HERE/'action-overrides.json')
for char,action,prior in JOBS:
 old=HERE/'jobs'/prior;receipt=read(old/'video.json')
 save(old/'source-review.json',{'id':prior,'approved':False,'reviewedAt':stamp(),'reason':'Actual source has unwanted generated visual effects, prop/pose drift or background changes; replacing with physical body-only choreography. Gameplay effects have separate verified real video atlases.','videoSha256':receipt['sha256']})
 id=f'{char}-n-{action}-body-v1';folder=HERE/'jobs'/id;spec=read(old/'request.json');spec['id']=id
 spec['prompt']=('Physical animation rehearsal of the EXACT illustrated character in this first frame, seen STRICTLY FROM BEHIND for the entire sequence. '
  'Only the back of her head and back of her clothes are visible. No cheek, eye, face or side profile is ever visible; her spine stays on the vertical center line and she never turns. '
  +motions[(char,action)]+' Begin within half a second, complete one movement in the first three seconds, then hold the original neutral posture. '
  'IMPORTANT: Only her actual arms, hands, knees, hair and clothing move. There are NO additional graphics, objects, emissions, particles, circles, rays, weapons, magic, lights, auras, strokes, symbols or trails. '
  'The lighting is fixed matte studio lighting. Every pixel outside the character stays the exact uniform flat GREEN RGB0,255,0; the background never changes color or brightness. '
  'Camera is locked elevated orthographic, no zoom, pan, cuts or perspective change. Preserve exact source character scale, face orientation, costume, hairstyle, original props and foot location. '
  'The entire silhouette stays inside the central two thirds. This is simple physical movement on a chroma-key stage, not a cinematic scene.')
 spec['lastFrame']=spec['firstFrame'];spec['priorityFront']=True;spec['revisionReason']='Body-only physical rehearsal and exact first/last-frame references repair the specifically observed unwanted spell graphics/prop or camera drift.'
 save(folder/'request.json',spec);submit(folder);overrides[f'{char}/n/{action}']=id
save(HERE/'action-overrides.json',overrides);status()
