"""One explicitly confirmed failed local sampling attempt, replaced with a new immutable job ID."""
from media import *
old=HERE/'jobs'/'glm-nw-death-centered-v1';attempt=read(old/'attempt.json');h=history(attempt['promptId'])
errors=[v for key,v in h['status']['messages'] if key=='execution_error']
if h['status']['status_str']!='error' or not errors or (old/'video.json').exists() or in_queue(attempt['promptId']):raise RuntimeError('Failure no longer unambiguous; retained without resubmission')
error=errors[-1]
if 'hostbuf_file_reader_read failed' not in error.get('exception_message',''):raise RuntimeError('Unexpected failure requires investigation before another submission')
id='glm-nw-death-centered-v2';folder=HERE/'jobs'/id
save(old/'execution-failure.json',{'at':stamp(),'promptId':attempt['promptId'],'failureConfirmed':True,'nodeId':error['node_id'],'exceptionType':error['exception_type'],'message':error['exception_message'],'historySha256':sha(old/'history.json'),'replacement':id,'priorReceiptsRetained':True})
if not (folder/'attempt.json').exists():
 spec=read(old/'request.json');spec['id']=id;spec['priorityFront']=True;spec['revisionReason']='Previous local sampler definitively failed with hostbuf_file_reader_read failed and produced no video; subsequent unrelated local jobs succeeded. New immutable attempt retains full original failure and identical512 capture contract.';save(folder/'request.json',spec);submit(folder)
overrides=read(HERE/'action-overrides.json');overrides['glm/nw/death']=id;save(HERE/'action-overrides.json',overrides);status()
