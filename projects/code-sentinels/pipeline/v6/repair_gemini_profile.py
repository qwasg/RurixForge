"""Replace the observed brown-floor composite with exact keyed physical state sources."""
from media import *
motions={
 'idle':'Stand calmly with subtle chest breathing and a small natural sway of the hair and twin-star cape. Both feet stay firmly on their original marks. Keep arms relaxed.',
 'walk':'Walk in place facing SCREEN LEFT throughout, four distinct alternating strides. Lift each knee and foot in turn, with opposing natural arm swings. Never translate across the screen or turn toward the viewer. Return to the original stance at the end.',
 'hit':'Perform two small physical startle-recovery repetitions without leaving the ground: bend the knees, draw shoulders back and pull forearms protectively toward the torso, then recover upright. Her boots stay on their original marks. Do not fall, kneel, disintegrate or disappear.'}
overrides=read(HERE/'action-overrides.json')
for action,motion in motions.items():
 id=f'gemini-w-{action}-body-v1';folder=HERE/'jobs'/id;ref='pipeline/v6/first-frames/gemini-w.png'
 prompt=('Physical animation of this EXACT small blue-purple twin-star girl seen in LEFT-SIDE PROFILE, always facing SCREEN LEFT. '+motion+
  ' Only the original body, hair, clothing and small existing accessories move. Preserve exact source character size, silhouette, face, colors, costume, ornaments and screen position. Do not grow, shrink, turn or approach the camera. '
  'Camera is fixed elevated orthographic. Every background pixel remains pure uniform chroma green RGB0,255,0 throughout. No floor, terrain, shadows, brown planes, smoke, particles, circles, rays, light, symbols or extra objects. '
  'The complete body stays inside the central two thirds of the original frame. Begin within half a second and finish one controlled sequence within four seconds, returning to the exact supplied neutral reference posture.')
 save(folder/'request.json',{'id':id,'category':'character','character':'gemini','direction':'w','action':action,'firstFrame':ref,'lastFrame':ref,'prompt':prompt,'frames':124,'width':384,'height':384,'fps':24,'segments':{action:[0,124,24,action in ['idle','walk']]},'priorityFront':True,'revisionReason':'The actual composite source introduces an opaque brown floor; independent physical motion on the exact green first/last reference replaces it without painting over the generated source.'})
 submit(folder);overrides[f'gemini/w/{action}']=id
save(HERE/'action-overrides.json',overrides);status()
