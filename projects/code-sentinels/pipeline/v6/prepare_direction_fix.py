"""Submit a separately justified directional action after actual source review."""
from media import *
char=sys.argv[1];direction=sys.argv[2];action=sys.argv[3]
if direction!='n' or action not in ['walk','hit','death']:raise SystemExit('This reviewed correction targets strict rear-view locomotion and reactions')
id=f'{char}-{direction}-{action}-v1';folder=HERE/'jobs'/id;ref=HERE/'first-frames'/f'{char}-{direction}.png'
motion={'walk':'The character walks AWAY from the viewer in place on an invisible treadmill. Alternate left and right steps naturally in one-second repeated walking cycles, heel lifting and arms swinging slightly. ONLY repeat this rear-view walking cycle for five seconds. No attack, cast, hit or death.',
 'hit':'Perform ONE short hit reaction: shoulders visibly hunch forward, arms flinch outward slightly, head lowers from the impact, then the body recovers to the initial rear-facing stance. Complete the recoil and recovery within two seconds then hold idle. Never walk, cast, turn or fall.',
 'death':'Perform ONE defeat animation: from the rear-facing standing pose the knees give way, she sinks to her knees and then falls forward face-down, AWAY from the viewer. Settle naturally and remain down. Do not roll toward viewer or turn around. Keep full fallen silhouette inside central two-thirds. No gore, no disintegration, no teleportation.'}[action]
prompt=(f'Animate the EXACT {char} chibi character in this reference. STRICT REAR VIEW ONLY for the ENTIRE video. '
 'Her BACK faces the viewer. We only see the BACK OF HER HEAD and BACK OF HER CLOTHES. Her face, eyes, nose, mouth, chest and front costume are NEVER visible. '
 'Remain perfectly centered at the same small scale. '+motion+' Maintain precisely the starting rear-facing yaw for every frame. '
 'NO turning of torso or head, no rotation, no walking toward camera, no side-facing poses. '
 'Fixed elevated orthographic isometric camera, no pan or zoom. Preserve exact hairstyle, rear ornaments, back clothing and colors. '
 'Pure flat chroma GREEN RGB0,255,0 background with no floor, environment, effects, extra people or text. '
 'Full silhouette remains inside central two-thirds of frame.')
save(folder/'request.json',{'id':id,'category':'character','character':char,'direction':direction,'action':action,'firstFrame':str(ref.relative_to(PROJECT)).replace('\\','/'),'prompt':prompt,'frames':124,'width':384,'height':384,'fps':24,'segments':{action:[0,124,24,action=='walk']},'priorityFront':True,'revisionReason':'Actual Claude north composite source turns toward side; strict rear-only single actions prevent the observed composite-direction failure. No mirrored or static stand-in.'})
submit(folder);status()
