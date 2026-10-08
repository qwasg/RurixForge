"""Maintain the global concurrency limit by waiting for the main queue to finish."""
import concurrent.futures
import json
import time
from pathlib import Path
from produce_clip import produce

HERE=Path(__file__).resolve().parent
building=json.loads((HERE/'building-jobs.json').read_text(encoding='utf-8'))
effects=json.loads((HERE/'effect-jobs.json').read_text(encoding='utf-8'))
prior=[clip for clip in building if not clip.startswith(('command-core-','vscode-turret-','pycharm-turret-'))]+effects
next_clips=[f'{slug}-{state}' for slug in ['vscode-turret','pycharm-turret'] for state in ['land','work','destroy']]
for clip in next_clips:
    if not (HERE/'jobs'/clip/'request.json').exists(): raise ValueError('Missing final turret reference/request: '+clip)
deadline=time.monotonic()+7200
old_count=-1
while True:
    completed=sum((HERE/'jobs'/clip/'video.json').exists() or (HERE/'jobs'/clip/'failure.json').exists() for clip in prior)
    if completed!=old_count:
        print(json.dumps({'phase':'waiting-on-existing-main-queue','terminal':completed,'total':len(prior),'newPaidSubmissions':0}),flush=True)
        old_count=completed
    if completed==len(prior): break
    if time.monotonic()>deadline: raise SystemExit('Prior queue still running; no turret task created; safe to resume waiting')
    time.sleep(15)
with concurrent.futures.ThreadPoolExecutor(max_workers=3) as executor:
    futures={executor.submit(produce,clip):clip for clip in next_clips}
    failures=[]
    for future in concurrent.futures.as_completed(futures):
        clip=futures[future]
        try: future.result()
        except Exception as error:
            failures.append(clip)
            print(json.dumps({'id':clip,'status':'stopped','error':str(error)[:500]}),flush=True)
    if failures: raise SystemExit('Unfinished turret clips: '+', '.join(failures))
