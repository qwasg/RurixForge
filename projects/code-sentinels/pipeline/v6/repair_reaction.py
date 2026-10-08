"""A specifically inspected failed composite reaction gets its own physical motion video."""
from media import *
char='claude';d='se';action='hit';id=f'{char}-{d}-{action}-body-v1';folder=HERE/'jobs'/id
reference=HERE/'first-frames'/f'{char}-{d}.png';prompt=('Physical motion rehearsal of this exact character, maintaining her exact initial FRONT-RIGHT three-quarter orientation for every frame. '
 'Perform one short natural shoulder flinch: quickly raise shoulders and draw elbows inward, tilt the head down a little, then relax back into the exact original posture. No walking, turning, falling, enlarging or torso lean toward the camera. '
 'Only the real body, hair and clothing move. There are NO lights, circles, swirls, strokes, emissions, particles, graphics, magic, weapons, text or other objects. '
 'The exact flat uniform green RGB0,255,0 backdrop never changes hue or brightness. Preserve the original face, costume, proportions and props. Fixed matte lighting and locked elevated orthographic camera, no pan, zoom, cuts or reframing. '
 'The flinch and recovery finish in the first two seconds, then hold the initial pose for the remaining video. Full silhouette remains in its original small centered safety area.')
save(folder/'request.json',{'id':id,'category':'character','character':char,'direction':d,'action':action,'firstFrame':str(reference.relative_to(PROJECT)).replace('\\','/'),'lastFrame':str(reference.relative_to(PROJECT)).replace('\\','/'),'prompt':prompt,'frames':124,'width':384,'height':384,'fps':24,'segments':{'hit':[0,66,16,False]},'priorityFront':True,'revisionReason':'Actual Claude-SE composite reaction contains a large opaque ink/green swirl; a physical flinch replaces that known bad segment, not a static frame.'})
submit(folder);mapping=read(HERE/'action-overrides.json');mapping[f'{char}/{d}/{action}']=id;save(HERE/'action-overrides.json',mapping);status()
