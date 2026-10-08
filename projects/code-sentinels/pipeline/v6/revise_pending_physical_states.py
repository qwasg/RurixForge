"""Replace only confirmed unstarted base tasks with focused physical states and fall margins."""
from media import *
facing={'s':'front view facing the viewer','sw':'front-left three-quarter view facing screen lower-left','w':'left-side profile facing screen left','nw':'rear-left three-quarter view facing upper-left','n':'strict rear view with back of head and back of clothing visible','ne':'rear-right three-quarter view facing upper-right','e':'right-side profile facing screen right','se':'front-right three-quarter view facing lower-right'}
overrides=read(HERE/'action-overrides.json');changes=[];anchors=read(HERE/'reviews/reference-anchors.json')
for char,directions in [('minimax',DIRS),('kimi',['sw','w','se'])]:
 for d in directions:
  old=HERE/'jobs'/f'{char}-{d}-{ACTION_VERSION}';id=f'{char}-{d}-physical-states-v3' if d!='n' else f'{char}-{d}-idle-physical-v1';folder=HERE/'jobs'/id
  if (folder/'attempt.json').exists():changes.append({'job':old.name,'status':'existing-replacement-retained','replacement':id});continue
  attempt=read(old/'attempt.json');task=attempt['promptId'];q=get('/queue')
  if task not in {row[1] for row in q['queue_pending']} or history(task):changes.append({'job':old.name,'status':'retained-not-confirmed-unstarted'});continue
  reference=HERE/'first-frames'/f'{char}-{d}.png'
  if d=='n':
   spec={'id':id,'category':'character','character':char,'direction':d,'action':'idle','firstFrame':str(reference.relative_to(PROJECT)).replace('\\','/'),'lastFrame':str(reference.relative_to(PROJECT)).replace('\\','/'),'frames':124,'width':384,'height':384,'fps':24,'segments':{'idle':[0,124,24,True]},'priorityFront':True}
   spec['prompt']='The illustrated person stands quietly, breathing slowly, with one gentle movement of her hair. We see only the back of her head and back of her clothing, as in the supplied picture. Her body stays at the same position and scale with a fixed elevated orthographic camera. Preserve her hairstyle, clothing, colors and all original accessories. She remains facing away. The only scene is this one person on a completely featureless constant green RGB0,255,0 backdrop with fixed matte lighting.'
  else:
   original=Image.open(reference).convert('RGB');foot=anchors[char][d]['foot'];offset=[round(256-foot[0]),round(288-foot[1])];box=key_green(original).getbbox()
   if box[0]+offset[0]<4 or box[1]+offset[1]<4 or box[2]+offset[0]>508 or box[3]+offset[1]>508:raise RuntimeError('Original subject would be cropped on physical state reference '+id)
   canvas=Image.new('RGB',(512,512),(0,255,0));canvas.paste(original,offset);ref=HERE/'first-frames'/f'{char}-{d}-states512.png';canvas.save(ref)
   prompt=(f'The exact small illustrated person in this image performs four physical movements, continuously in the original {facing[d]}. '
    '0 to1 second: stand calmly and breathe. 1 to3.5 seconds: walk in place through four clear alternating steps. Lift each knee and foot visibly, swing the opposite arm, and return both feet to their original marks. The outline changes naturally with the stepping legs and moving fabric. '
    '3.5 to5 seconds: one sudden physical flinch, tuck the chin, recoil the shoulders and draw hands inward, soften knees, then recover fully upright. '
    '5 to9 seconds: lower onto the knees, place a hand down, lower the torso to a complete side-lying or prone rest, and remain down. Keep the entire final body around the initial foot location. '
    'Preserve exactly the face, hairstyle, clothes, colors and original small accessories. Body heading remains constant, and the camera stays fixed elevated orthographic at the exact original scale. The added empty margins remain empty. '
    'The only scene is this single person on a completely featureless, uniform GREEN RGB0,255,0 backdrop. Every background pixel and the matte lighting remain constant throughout. Keep the full hair, head, limbs, clothing and final resting body inside the central ninety percent of the image.')
   spec={'id':id,'category':'character','character':char,'direction':d,'action':'physical-states','firstFrame':str(ref.relative_to(PROJECT)).replace('\\','/'),'frames':216,'width':512,'height':512,'fps':24,'sourceFootAnchor':[foot[i]+offset[i] for i in range(2)],'outputPivot':[.5,.5625],'outputPlaneSpan':2.5456*512/384,'segments':{'idle':[0,24,6,True],'walk':[24,84,30,True],'hit':[84,120,20,False],'death':[120,216,32,False]},'priorityFront':True,'prompt':prompt}
  spec['revisionReason']='Focused physical base movements remove the completed-batch overlap between casting and reactions. Original actor pixels are unchanged on wider real capture space. Separate independent attack/cast tasks remain authoritative.'
  save(folder/'request.json',spec);save(old/'pending-before-physical-revision.json',{'at':stamp(),'promptId':task,'confirmedPending':True,'confirmedNoHistory':True,'replacement':id})
  response=requests.post(BASE+'/queue',json={'delete':[task]},timeout=45);response.raise_for_status()
  if in_queue(task) or history(task):changes.append({'job':old.name,'status':'retained-moved-to-execution'});continue
  save(old/'cancelled.json',{'at':stamp(),'promptId':task,'cancelledBeforeExecution':True,'replacement':id,'reason':spec['revisionReason'],'oldReceiptsRetained':True})
  submit(folder)
  for action in spec['segments']:overrides[f'{char}/{d}/{action}']=id
  save(HERE/'action-overrides.json',overrides);changes.append({'job':old.name,'status':'cancelled-before-execution','replacement':id})
save(HERE/'physical-states-pending-revision.json',{'at':stamp(),'changes':changes,'globalQueueClearUsed':False,'runningInterrupted':False,'receiptsDeleted':False});status()
