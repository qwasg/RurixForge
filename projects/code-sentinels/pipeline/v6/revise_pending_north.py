"""Surgically cancel only confirmed unstarted owned prompts, preserving all attempt evidence."""
from media import *
mapping=read(HERE/'action-overrides.json') if (HERE/'action-overrides.json').exists() else {}
changes=[]
for char in CHARACTERS:
 for action in ['attack','cast']:
  old=HERE/'jobs'/f'{char}-n-{action}-v1';spec=read(old/'request.json');attempt=read(old/'attempt.json');task=attempt['promptId']
  revised=HERE/'jobs'/f'{char}-n-{action}-v2'
  if (revised/'attempt.json').exists():mapping[f'{char}/n/{action}']=revised.name;continue
  q=get('/queue');pending={r[1] for r in q['queue_pending']};running={r[1] for r in q['queue_running']}
  if task in running or history(task) or task not in pending:
   changes.append({'job':old.name,'status':'not-cancelled-no-confirmed-unstarted-prompt'});continue
  save(old/'pending-before-revision.json',{'at':stamp(),'promptId':task,'confirmedPending':True,'confirmedNotRunning':True,'confirmedNoHistory':True})
  response=requests.post(BASE+'/queue',json={'delete':[task]},timeout=45);response.raise_for_status()
  if in_queue(task) or history(task):
   changes.append({'job':old.name,'status':'not-replaced-task-moved-to-execution'});continue
  save(old/'cancelled.json',{'at':stamp(),'promptId':task,'reason':'Known north-facing quality correction from observed composite video; explicit rear-only constraints added before sampling.','cancelledBeforeExecution':True,'api':'POST /queue delete exact one owned pending ID','oldReceiptsRetained':True,'replacement':revised.name})
  spec['id']=revised.name;spec['priorityFront']=True
  spec['prompt']='STRICT REAR VIEW ONLY. This character ALWAYS faces away from the viewer. Only back of head and back of clothing are visible; her face, eyes, nose, mouth and front clothes are NEVER shown. The action is aimed AWAY from viewer toward the top of frame. The head and torso NEVER turn sideways or toward the camera. '+spec['prompt']+' Stay in the exact initial rear-facing orientation while moving arms; never turn to demonstrate the gesture.'
  spec['revisionReason']='Replace confirmed cancelled-before-execution north gesture with strict rear-only direction after successful rear-walk proof.'
  save(revised/'request.json',spec);submit(revised);mapping[f'{char}/n/{action}']=revised.name;changes.append({'job':old.name,'status':'confirmed-cancelled-before-execution','replacement':revised.name})
save(HERE/'action-overrides.json',mapping);save(HERE/'north-pending-revision.json',{'at':stamp(),'changes':changes,'globalQueueClearUsed':False,'runningTaskInterrupted':False,'receiptsDeleted':False});status()
