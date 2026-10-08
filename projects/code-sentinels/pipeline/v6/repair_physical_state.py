"""Separate keyed idle/walk sources for concretely rejected scene-contaminated videos."""
from media import *
char,d,action=sys.argv[1:4]
if action not in ['idle','walk']:raise ValueError('This tool only authors idle or walk')
id=f'{char}-{d}-{action}-body-v1';folder=HERE/'jobs'/id
if (folder/'attempt.json').exists():raise RuntimeError('Existing immutable attempt: '+id)
face={'s':'front-facing toward the viewer','sw':'front-left three-quarter facing down-left','w':'strict left-side profile facing screen left','nw':'rear-left three-quarter facing up-left','n':'strict rear-facing away from the viewer','ne':'rear-right three-quarter facing up-right','e':'strict right-side profile facing screen right','se':'front-right three-quarter facing down-right'}[d]
motion=('Stand calmly with subtle visible chest breathing, a gentle hair sway and relaxed hands. Both feet remain firmly planted on the original marks. No dramatic gestures.' if action=='idle' else 'Walk in place at a steady pace, taking four complete alternating left-right strides with visibly lifted knees and feet, swinging the opposite arm naturally. Feet return to the exact starting stance at the end. Do not translate across the frame or glide without stepping.')
ref=f'pipeline/v6/first-frames/{char}-{d}.png'
prompt=(f'Physical {action} animation of the exact small illustrated character, continuously in {face} orientation. '+motion+
 ' Keep the same full-body silhouette, hairstyle, clothing, face, colors and original accessories. Camera is locked elevated orthographic, with EXACT original body size and screen location. Never turn, grow, shrink, rotate toward viewer, pan, zoom or approach the screen edges. '
 'All motion is physical body, hair and fabric movement. No emitted colors, lights, stars, symbols, particles, graphics, circles, rays, orbits, extra props, auras or shadows. No floor or environment. '
 'Every background pixel must remain perfectly flat uniform pure GREEN RGB0,255,0 throughout the entire video, with unchanged matte studio lighting. This is a keyed animation reference, not a cinematic scene. Complete the brief sequence within4 seconds and return to the exact supplied neutral reference posture.')
save(folder/'request.json',{'id':id,'category':'character','character':char,'direction':d,'action':action,'firstFrame':ref,'lastFrame':ref,'prompt':prompt,'frames':124,'width':384,'height':384,'fps':24,'segments':{action:[0,124,24,True]},'priorityFront':True,'revisionReason':'Actual raw source has persistent colored particles/background contamination throughout its idle/walk. Replace that state with independent physical motion rather than painting it out.'})
submit(folder);overrides=read(HERE/'action-overrides.json');overrides[f'{char}/{d}/{action}']=id;save(HERE/'action-overrides.json',overrides)
