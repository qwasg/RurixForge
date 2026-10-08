from media import *
old=HERE/'jobs/kimi-n-cast-body-v1';receipt=read(old/'video.json');spec=read(old/'request.json')
save(old/'source-review.json',{'id':old.name,'approved':False,'reviewedAt':stamp(),'reason':'Full-source contact: stationary lunge instruction causes significant apparent scale increase and body clipping. Replace with upright arm-only choreography; actual dash translation is gameplay movement.','videoSha256':receipt['sha256']})
id='kimi-n-cast-body-v2';folder=HERE/'jobs'/id;spec['id']=id
spec['prompt']=('Physical arm-stretch rehearsal of this EXACT small illustrated character, seen strictly from behind. '
 'She stands at precisely the same height and location from first to last frame. Her knees, feet, spine, head position and torso remain absolutely stationary; no bending, crouching, leaning, jumping, walking, growing or approaching the viewer. '
 'ONLY MOVE THE ARMS: slowly spread both upper arms sideways up to shoulder height, open both hands, hold the wide symmetric pose briefly, then slowly lower both arms to the exact initial pose. '
 'Keep back of head facing viewer; no face, cheek, eyes or side profile. Preserve exact original costume, hairstyle and small existing props. '
 'Every pixel outside the silhouette is perfectly flat uniform chroma green RGB0,255,0 for the entire video. Fixed matte lighting and locked orthographic camera: no zoom, pan, lens movement, reframing or change in screen scale. '
 'No visual effects, emitted light, graphics, rings, lines, particles, weapons or other objects. This is a plain physical arm exercise. One repetition within first three seconds, then hold the original standing pose. Full body remains in the exact original small central silhouette area.')
spec['priorityFront']=True;save(folder/'request.json',spec);submit(folder);mapping=read(HERE/'action-overrides.json');mapping['kimi/n/cast']=id;save(HERE/'action-overrides.json',mapping);status()
