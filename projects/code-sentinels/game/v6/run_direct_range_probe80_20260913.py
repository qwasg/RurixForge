"""Execute exactly the Root-approved preregistered80 using its frozen candidate."""
from pathlib import Path
import concurrent.futures
import datetime
import hashlib
import json
import os
import subprocess
import sys
import time
import build_direct_range_probe_runner_20260913 as build

ROOT=build.ROOT;OUT=build.OUT;RUN=OUT/'candidate'
read=build.read;sha=build.sha;write=build.write;ref=build.ref;now=build.now


def compiled_sources():return [r for r in build.sources()if '/tests/'not in r['path']]


def main():
    prereg=read(OUT/'pre-registration.json');receipt=read(OUT/'runner-build-receipt.json')
    assert receipt['exitCode']==0 and receipt['sourceUnchanged']
    assert receipt['preRegistration']['sha256']==sha(OUT/'pre-registration.json')
    assert len(prereg['cases'])==80 and set(c['index']for c in prereg['cases'])==set(prereg['indices'])
    fp=receipt['rulesFingerprint'];assert build.fingerprint()==fp
    exe=ROOT/receipt['artifact']['path'];assert sha(exe)==receipt['artifact']['sha256']
    plan=ROOT/prereg['canonicalPlan']['path'];assert sha(plan)==prereg['canonicalPlan']['sha256']
    source_before=compiled_sources();expected=[r for r in read(ROOT/receipt['sourceAfter']['path'])if '/tests/'not in r['path']];assert source_before==expected
    write(RUN/'compiled-source-before.json',source_before)
    snapshot=RUN/'orchestrator-source.py'
    with snapshot.open('xb')as f:f.write(Path(__file__).read_bytes())
    start=dict(startedAtUtc=now(),pid=os.getpid(),scope='Approved fixed80 paired CPU probe only; no final balance or performance approval.',rulesFingerprint=fp,rulesVersion='v6.2',preRegistration=ref(OUT/'pre-registration.json'),runnerBuild=ref(OUT/'runner-build-receipt.json'),runner=receipt['artifact'],canonicalPlan=prereg['canonicalPlan'],orchestrator=ref(snapshot),maxWorkers=16,requiredCases=80,simulatedMinutesBudget=55,rootAuthorizedConcurrentNativeCorrectnessSuite=True)
    write(RUN/'start-receipt.json',start);print(json.dumps(dict(type='probe-start',**start)),flush=True)

    def run(binding):
        i=binding['index'];case=binding['case'];raw=RUN/f'results-{i:03}.jsonl';saved=RUN/f'diagnostics/match-{i}.save.json'
        command=[str(exe),'--plan',str(plan),'--first',str(i),'--limit','1','--minutes','55','--source-hash',fp,'--stop-on-failure','true','--out',str(raw),'--diagnostics',str(RUN/'diagnostics')]
        invocation=dict(index=i,command=command,cwd=str(ROOT),startedAtUtc=now(),rulesFingerprint=fp,runner=receipt['artifact'],baseline=binding['originalRaw'])
        write(RUN/f'invocation-{i:03}.json',invocation)
        began=time.monotonic()
        with(RUN/f'run-{i:03}.stdout.log').open('x',encoding='utf-8')as stdout,(RUN/f'run-{i:03}.stderr.log').open('x',encoding='utf-8')as stderr:
            proc=subprocess.Popen(command,cwd=ROOT,stdout=stdout,stderr=stderr,creationflags=subprocess.CREATE_NO_WINDOW)
            write(RUN/f'process-{i:03}.json',dict(index=i,pid=proc.pid,startedAtUtc=invocation['startedAtUtc']))
            code=proc.wait()
        execution=dict(**invocation,pid=proc.pid,exitCode=code,endedAtUtc=now(),wallSeconds=time.monotonic()-began,raw=ref(raw)if raw.exists()else None,save=ref(saved)if saved.exists()else None)
        write(RUN/f'execution-{i:03}.json',execution);assert code==0,(i,code)
        rows=[json.loads(l)for l in raw.read_text(encoding='utf-8').splitlines()if l.strip()];assert len(rows)==1;r=rows[0]
        assert all(r[k]==case[k]for k in ['index','seed','theme','branches','strategies','swapped','plannerOrder','originalPair'])
        assert r['planSha256']==prereg['canonicalPlan']['sha256'] and r['planIndex']==i
        assert r['simulationSourceSha256']==fp and r['rulesVersion']=='v6.2'
        s=read(saved);assert s['rulesFingerprint']==fp and s['snapshot']['tick']/60==r['seconds'] and s['snapshot']['winner']==r['winner']
        assert not s['initialAi'] and not s['administrativeEvents'] and 0<r['seconds']<=3300
        assert r['winner']in[1,2]or r['seconds']==3300
        assert sha(ROOT/binding['preservedRaw']['path'])==binding['preservedRaw']['sha256']
        return dict(index=i,seconds=r['seconds'],unresolved=r['winner']is None,wallSeconds=execution['wallSeconds'])

    jobs=iter(prereg['cases']);done_rows=[];failures=[];began=time.monotonic()
    with concurrent.futures.ThreadPoolExecutor(max_workers=16)as pool:
        pending={pool.submit(run,next(jobs)):None for _ in range(16)}
        while pending:
            completed,_=concurrent.futures.wait(pending,timeout=30,return_when=concurrent.futures.FIRST_COMPLETED)
            for future in completed:
                pending.pop(future)
                try:
                    result=future.result();done_rows.append(result);print(json.dumps(dict(type='probe-case-complete',completed=len(done_rows),required=80,**result)),flush=True)
                except BaseException as e:failures.append(repr(e));print(json.dumps(dict(type='probe-case-failed',error=repr(e))),flush=True)
                if not failures:
                    job=next(jobs,None)
                    if job is not None:pending[pool.submit(run,job)]=None
            if not completed:print(json.dumps(dict(type='probe-progress',completed=len(done_rows),required=80,active=len(pending))),flush=True)
    after=compiled_sources();write(RUN/'compiled-source-after.json',after)
    completion=dict(**start,endedAtUtc=now(),elapsedWallSeconds=time.monotonic()-began,completedCases=len(done_rows),failed=failures,sourceUnchanged=after==source_before,rulesFingerprintAfter=build.fingerprint(),cases=done_rows,complete=len(done_rows)==80 and not failures and after==source_before and build.fingerprint()==fp)
    write(RUN/'completion-receipt.json',completion);assert completion['complete'],'Incomplete probe; all raw evidence retained.'
    print(json.dumps(dict(type='probe-complete',cases=80,rulesFingerprint=fp,finalBalanceAcceptance=False)),flush=True)


if __name__=='__main__':main()
