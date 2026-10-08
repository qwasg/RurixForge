"""Observed local sampler durations and remaining compute estimate, excluding queue wait."""
from media import *
groups={};by_prompt={}
for path in (HERE/'jobs').glob('*/attempt.json'):
 folder=path.parent;spec=read(folder/'request.json');attempt=read(path);shape=f'{spec["width"]}x{spec["height"]}/{spec["frames"]}';by_prompt[attempt['promptId']]=(folder.name,shape)
 if not (folder/'history.json').exists() or not (folder/'video.json').exists():continue
 h=read(folder/'history.json');messages=h.get('status',{}).get('messages',[]);starts=[m['timestamp'] for key,m in messages if key=='execution_start'];ends=[m['timestamp'] for key,m in messages if key=='execution_success']
 if starts and ends:
  seconds=(ends[-1]-starts[0])/1000
  if seconds>0:groups.setdefault(shape,[]).append((ends[-1],seconds))
summary={}
for shape,rows in groups.items():
 recent=[r[1] for r in sorted(rows)[-20:]];summary[shape]={'recentSamples':len(recent),'medianSeconds':float(np.median(recent)),'p90Seconds':float(np.quantile(recent,.9))}
q=get('/queue');remaining=[]
for row in q['queue_running']+q['queue_pending']:
 if row[1] not in by_prompt:continue
 id,shape=by_prompt[row[1]];remaining.append({'job':id,'shape':shape,'medianSeconds':summary.get(shape,{}).get('medianSeconds')})
estimate=sum(r['medianSeconds'] or 0 for r in remaining)
result={'at':stamp(),'groups':summary,'remainingExecutionJobs':len(remaining),'estimatedRemainingSamplerHoursAtRecentMedian':estimate/3600,'estimateExcludes':'future quality revisions, visual review, extraction, game testing and any user-blocked native runtime','remaining':remaining};save(HERE/'sampling-metrics.json',result);emit({k:v for k,v in result.items() if k!='remaining'})
