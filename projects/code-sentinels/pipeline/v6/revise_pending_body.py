"""Replace only owned not-yet-sampled gestures after successful clean body-only pilots."""
from media import *
facing={
 's':'Keep the exact FRONT view, facing directly toward viewer with both eyes visible. No turn to profile or back.',
 'sw':'Keep the exact initial FRONT-LEFT three-quarter view, facing screen lower-left. Do not turn fully front or back.',
 'w':'Keep the exact LEFT side profile looking screen-left. Do not turn toward viewer or show back.',
 'nw':'Keep the exact REAR-LEFT three-quarter view looking screen upper-left; primarily back of head and back clothing. Never turn to a full side or front view.',
 'ne':'Keep the exact REAR-RIGHT three-quarter view looking screen upper-right; primarily back of head and back clothing. Never turn to a full side or front view.',
 'e':'Keep the exact RIGHT side profile looking screen-right. Do not turn toward viewer or show back.',
 'se':'Keep the exact initial FRONT-RIGHT three-quarter view facing screen lower-right. Do not turn fully front or back.'}
motion={
 'kimi':{'attack':'Perform one brief forearm-extension rehearsal in the direction already faced: extend the right forearm with the exact tiny pen and retract. Keep original book and pen; no larger tool or blade.','cast':'Perform one upright arm-stretch rehearsal: spread both upper arms to shoulder height with palms extended toward the direction already faced, hold briefly and lower. Keep knees, spine, head height and body scale stationary.'},
 'claude':{'attack':'Perform one brief physical palm-pushing exercise along the current body-facing direction: extend one forearm, then retract it.','cast':'Perform one symmetric arm-stretch: spread both upper arms outward to shoulder level, bend elbows with palms facing forward, hold the open-arm posture briefly then lower.'},
 'gpt':{'attack':'Perform one compact symmetric pushing exercise: bend both elbows near the waist, extend both forearms forward along the current facing, then retract both hands.','cast':'Perform one broad upright arm-stretch: raise both arms outward and upward into a gentle V, open palms, hold briefly, then lower slowly. Keep wings in their original compact shape.'},
 'deepseek':{'attack':'Perform one quick open-palm forearm extension toward the direction already faced, then retract. The whale tail gently counterbalances.','cast':'Perform one broad arm rehearsal while staying upright: extend both arms forward, move hands apart in a smooth arc, hold briefly, then return hands to neutral. Only arms and existing whale tail move.'},
 'gemini':{'attack':'Lift both elbows slightly and extend both forearms straight toward the current facing direction, pointing two fingers on each hand, then retract.','cast':'Perform an upright arm stretch: raise one hand above head height while extending the other forearm toward the direction already faced, hold briefly, then lower both. Torso and head remain stationary.'},
 'minimax':{'attack':'Lift the exact existing microphone closer to the mouth and make one compact forward hand gesture with the free hand, then lower to original position.','cast':'Raise the existing microphone while extending the free arm broadly outward and upward, hold the pose briefly, then return both hands to original positions. No extra props.'},
 'glm':{'attack':'Point the existing small pen forward with a quick forearm extension along the current body-facing direction, then retract.','cast':'Open the existing book close to the chest and raise the other open palm to shoulder height, hold briefly, then close the book and return hands to their starting positions.'}}
overrides=read(HERE/'action-overrides.json');changes=[]
# Confirmed completed Claude/DeepSeek base clips allow the first complete operators to stream sooner.
for char in ['gemini','gpt','kimi','minimax','glm','deepseek','claude']:
 for d in [v for v in DIRS if v!='n']:
  for action in ['attack','cast']:
   old=HERE/'jobs'/f'{char}-{d}-{action}-v1';new=HERE/'jobs'/f'{char}-{d}-{action}-body-v1'
   if (new/'attempt.json').exists():overrides[f'{char}/{d}/{action}']=new.name;continue
   attempt=read(old/'attempt.json');task=attempt['promptId'];q=get('/queue')
   if task not in {r[1] for r in q['queue_pending']} or history(task):changes.append({'job':old.name,'status':'retained-not-confirmed-unstarted'});continue
   save(old/'pending-before-revision.json',{'at':stamp(),'promptId':task,'confirmedPending':True,'confirmedNoHistory':True})
   response=requests.post(BASE+'/queue',json={'delete':[task]},timeout=45);response.raise_for_status()
   if in_queue(task) or history(task):changes.append({'job':old.name,'status':'retained-moved-to-execution'});continue
   save(old/'cancelled.json',{'at':stamp(),'promptId':task,'cancelledBeforeExecution':True,'replacement':new.name,'reason':'Observed unwanted props/effect backgrounds/zoom in gesture source. Validated clean body-only first+last-frame physical rehearsal replaces unstarted owned task.','oldReceiptsRetained':True})
   spec=read(old/'request.json');spec['id']=new.name
   spec['prompt']=('Physical animation rehearsal of the EXACT illustrated character in the supplied first frame. '+facing[d]+' '+motion[char][action]+' Begin the motion within half a second, complete one repetition in the first three seconds, then hold the exact initial neutral posture. '
    'Keep the head position, torso, knees, feet and body screen scale absolutely stationary; no crouching, leaning toward camera, jumping, running, body turn or approaching the viewer. Only the specified arm/hand movement and natural small hair/clothing response are allowed. '
    'Preserve original hairstyle, costume, face, proportions, all ornaments and exact existing small props. Do not add or enlarge objects. '
    'Every pixel outside the silhouette remains exactly flat uniform chroma GREEN RGB0,255,0 for the entire video. Fixed matte lighting and fixed elevated orthographic camera. No zoom, pan, cuts, perspective change, graphics, emitted lights, particles, circles, rays, auras, weapons, magic, trails, text or background color changes. '
    'This is a plain physical motion reference on a green stage. The full silhouette stays inside the original central two-thirds safety area.')
   spec['lastFrame']=spec['firstFrame'];spec['priorityFront']=char in ['claude','deepseek'];spec['revisionReason']='Clean physical choreography validated on real body-only pilots; separate verified VFX provide gameplay effects.'
   save(new/'request.json',spec);submit(new);overrides[f'{char}/{d}/{action}']=new.name;changes.append({'job':old.name,'status':'cancelled-before-execution','replacement':new.name})
   save(HERE/'action-overrides.json',overrides)
save(HERE/'body-pending-revision.json',{'at':stamp(),'changes':changes,'globalQueueClearUsed':False,'runningInterrupted':False,'receiptsDeleted':False});status()
