"""Preserve original actor pixels on a larger centered canvas so the real fall has room."""
from media import *
char=sys.argv[1];d=sys.argv[2];id=f'{char}-{d}-death-centered-v1';folder=HERE/'jobs'/id
original=Image.open(HERE/'first-frames'/f'{char}-{d}.png').convert('RGB');foot=read(HERE/'reviews/reference-anchors.json')[char][d]['foot']
offset=[round(256-foot[0]),round(256-foot[1])];canvas=Image.new('RGB',(512,512),(0,255,0));canvas.paste(original,offset)
ref=HERE/'first-frames'/f'{char}-{d}-death-centered512.png';canvas.save(ref);actual_foot=[foot[i]+offset[i] for i in range(2)]
face={'s':'front view facing viewer','sw':'front-left three-quarter view','w':'left-side profile facing screen left','nw':'rear-left three-quarter view','n':'strict rear view facing away','ne':'rear-right three-quarter view','e':'right-side profile facing screen right','se':'front-right three-quarter view'}[d]
prompt=(f'Physical kneel-and-lie-down animation of the exact character in this reference, beginning in the exact {face}. '
 'The supplied wider image contains deliberate empty safety margins. Preserve this EXACT SMALL CHARACTER SCALE and the fixed camera throughout; do not zoom in, enlarge the person, recenter or reframe. '
 'Perform one controlled defeat movement: knees give way, lower onto the knees, then lower the torso to rest on the ground face-down or on the side, eyes closed. Remain lying still through the end. '
 'The feet start at the middle of the picture; keep the complete motion and final body localized around that original point. The head, hair, clothing, hands and feet all remain inside the central ninety percent of the picture even when lying down. '
 'Preserve exact original hairstyle, colors, costume and small existing props. No new props, particles, graphics, emitted light, auras, circles, text, disintegration or body fading. '
 'Pure flat uniform chroma green RGB0,255,0 at every background pixel for the entire video. No visible floor, shadows, landscape or studio background. Fixed elevated orthographic camera and unchanged matte lighting. '
 'Complete the kneel and settling within three seconds, then hold the fallen pose. Do not stand back up.')
save(folder/'request.json',{'id':id,'category':'character','character':char,'direction':d,'action':'death','firstFrame':str(ref.relative_to(PROJECT)).replace('\\','/'),'prompt':prompt,'frames':124,'width':512,'height':512,'fps':24,'sourceFootAnchor':actual_foot,'outputPivot':[.5,.5],'outputPlaneSpan':2.5456*512/384,'segments':{'death':[0,124,24,False]},'priorityFront':True,'revisionReason':'Raw composite fall visibly crosses the source image edge. Larger canvas preserves original actor pixels and adds real fall space; per-clip pivot/span retains the same world foot anchor and apparent standing size.'})
submit(folder);mapping=read(HERE/'action-overrides.json');mapping[f'{char}/{d}/death']=id;save(HERE/'action-overrides.json',mapping);status()
