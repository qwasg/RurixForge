"""Approved complete6828 verification:126fresh strategy +920fresh/80reused branch."""
from pathlib import Path
import concurrent.futures,datetime,hashlib,json,os,shutil,subprocess,time
import build_direct_range_probe_runner_20260913 as build
ROOT=build.ROOT
PROBE=ROOT/'game/v6/direct-range-probe80-20260913'
BASE=ROOT/'game/v6/final-balance-6828c5b7-20260914'
FP='6828c5b7f825b7e27b3e9df3c22d133ecefd8195a436f856c0682a33e580d781'
read=build.read;sha=build.sha;ref=build.ref;write=build.write;now=build.now


def sources():return[r for r in build.sources()if '/tests/'not in r['path']]


def main():
    paired=read(PROBE/'paired-analysis.json');assert paired['integrityPassed']and paired['caseCount']==80
    prereg=read(PROBE/'pre-registration.json');receipt=read(PROBE/'runner-build-receipt.json')
    assert receipt['rulesFingerprint']==FP and build.fingerprint()==FP
    runner=ROOT/receipt['artifact']['path'];assert sha(runner)==receipt['artifact']['sha256']
    before=sources();assert before==read(PROBE/'candidate/compiled-source-after.json')
    assert not BASE.exists(),'Preserve an existing full run.'
    BASE.mkdir();(BASE/'artifact').mkdir()
    shutil.copy2(runner,BASE/'artifact/sentinels-v6-balance-runner.exe')
    exe=BASE/'artifact/sentinels-v6-balance-runner.exe';assert sha(exe)==sha(runner)
    write(BASE/'source-before.json',before)
    for item in before:
        dest=BASE/'source'/item['path'];dest.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(ROOT/item['path'],dest);assert sha(dest)==item['sha256']
    for name in ['strategy','branch']:(BASE/name/'diagnostics').mkdir(parents=True)
    strategy_plan=ROOT/'game/v6/strategy-plan.json';branch_plan=PROBE/'canonical-branch-plan.json'
    assert sha(strategy_plan)=='8ab5503d68773260e193af5fa9cf8e669cb199a13358f4dddc05270631b40f44'
    assert sha(branch_plan)==prereg['canonicalPlan']['sha256']
    shutil.copy2(strategy_plan,BASE/'strategy/strategy-plan.json');shutil.copy2(branch_plan,BASE/'branch/branch-plan.json')
    reuse=[]
    for i in prereg['indices']:
        original=PROBE/f'candidate/results-{i:03}.jsonl';saved=PROBE/f'candidate/diagnostics/match-{i}.save.json';execution=PROBE/f'candidate/execution-{i:03}.json'
        row=read(original);case=next(c for c in read(branch_plan)['cases']if c['index']==i)
        assert row['simulationSourceSha256']==FP and all(row[k]==case[k]for k in ['index','seed','theme','branches','strategies','swapped','plannerOrder','originalPair'])
        assert row['planSha256']==sha(branch_plan)and row['planIndex']==i
        ex=read(execution);assert ex['exitCode']==0 and ex['runner']['sha256']==sha(exe) and ex['raw']['sha256']==sha(original) and ex['save']['sha256']==sha(saved)
        raw_copy=BASE/f'branch/reused-results-{i:03}.jsonl';save_copy=BASE/f'branch/diagnostics/match-{i}.save.json'
        shutil.copy2(original,raw_copy);shutil.copy2(saved,save_copy)
        assert sha(raw_copy)==sha(original)and sha(save_copy)==sha(saved)
        reuse.append(dict(index=i,originalRaw=ref(original),preservedRaw=ref(raw_copy),originalSave=ref(saved),preservedSave=ref(save_copy),originalExecution=ref(execution)))
    write(BASE/'reuse-80-audit.json',dict(rulesFingerprint=FP,runnerSha256=sha(exe),planSha256=sha(branch_plan),strictPairedAudit=ref(PROBE/'paired-analysis.json'),cases=reuse,scope='All80 original6828 raw rows/Saves copied byte-identically; original execution receipts retained. No new simulation or relabelling for these cases.'))
    plans={};job_lists={}
    for name,plan_path,required in [('strategy',BASE/'strategy/strategy-plan.json',126),('branch',BASE/'branch/branch-plan.json',1000)]:
        cases=read(plan_path)['cases'];fresh=[c for c in cases if name=='strategy'or c['index']not in prereg['indices']]
        jobs=[]
        for case in fresh:
            i=case['index'];out=BASE/name
            command=[str(exe),'--plan',str(plan_path),'--first',str(i),'--limit','1','--minutes','55','--source-hash',FP,'--stop-on-failure','true','--out',str(out/f'results-{i:03}.jsonl'),'--diagnostics',str(out/'diagnostics')]
            jobs.append(dict(matrix=name,index=i,case=case,command=command))
        plan=dict(matrix=name,requiredTotalCases=required,reusedOriginalCases=0 if name=='strategy' else 80,reusedOriginalInputFiles=0 if name=='strategy' else 80,freshCases=len(fresh),plan=ref(plan_path),maxConcurrentProcesses=16,jobs=jobs)
        write(BASE/name/'execution-plan.json',plan);plans[name]=plan;job_lists[name]=jobs
    manifest=dict(rulesVersion='v6.2',rulesFingerprint=FP,runner=ref(exe),sourceBefore=ref(BASE/'source-before.json'),reuseAudit=ref(BASE/'reuse-80-audit.json'),aggregator=ref(ROOT/'game/v6/summarize_balance.py'),matrices={name:dict(executionPlan=ref(BASE/name/'execution-plan.json'),inputPlan=plan['plan'],requiredCases=plan['requiredTotalCases'],reusedCases=plan['reusedOriginalCases'],freshCases=plan['freshCases'])for name,plan in plans.items()},maxConcurrentProcesses=16,scope='Approved complete rule6828 measurement;80 preserved+920fresh branch,126fresh strategy. No balance/performance/LAN approval.')
    write(BASE/'measurement-manifest.json',manifest)
    with(BASE/'shared-orchestrator-source.py').open('xb')as f:f.write(Path(__file__).read_bytes())
    start=dict(startedAtUtc=now(),pid=os.getpid(),rulesFingerprint=FP,maxGameWorkers=16,freshStrategyCases=126,freshBranchCases=920,reusedBranchCases=80,manifest=ref(BASE/'measurement-manifest.json'),orchestrator=ref(BASE/'shared-orchestrator-source.py'),authorization='Root approved full6828 after80 strict paired audit. One shared16worker queue; no automatic LAN.')
    write(BASE/'start-receipt.json',start);print(json.dumps(dict(type='full-shared-start',**start)),flush=True)
    jobs=[]
    for ordinal in range(920):
        for name in ['strategy','branch']:
            if ordinal<len(job_lists[name]):jobs.append(job_lists[name][ordinal])
    def run(job):
        name=job['matrix'];i=job['index'];out=BASE/name;case=job['case'];began=time.monotonic()
        inv=dict(matrix=name,index=i,command=job['command'],cwd=str(ROOT),startedAtUtc=now(),rulesFingerprint=FP,runner=ref(exe))
        write(out/f'invocation-{i:03}.json',inv)
        with(out/f'run-{i:03}.stdout.log').open('x',encoding='utf-8')as stdout,(out/f'run-{i:03}.stderr.log').open('x',encoding='utf-8')as stderr:
            proc=subprocess.Popen(job['command'],cwd=ROOT,stdout=stdout,stderr=stderr,creationflags=subprocess.CREATE_NO_WINDOW);write(out/f'process-{i:03}.json',dict(index=i,pid=proc.pid,startedAtUtc=inv['startedAtUtc']));code=proc.wait()
        raw=out/f'results-{i:03}.jsonl';saved=out/f'diagnostics/match-{i}.save.json'
        execution=dict(**inv,pid=proc.pid,exitCode=code,endedAtUtc=now(),wallSeconds=time.monotonic()-began,raw=ref(raw)if raw.exists()else None,save=ref(saved)if saved.exists()else None)
        write(out/f'execution-{i:03}.json',execution);assert code==0,(name,i,code)
        row=read(raw);s=read(saved)
        assert all(row[k]==case[k]for k in ['index','seed','theme','branches','strategies','swapped','plannerOrder'])and row['originalPair']==case.get('originalPair')
        assert row['simulationSourceSha256']==FP and row['planSha256']==plans[name]['plan']['sha256']and row['planIndex']==i
        assert s['rulesFingerprint']==FP and s['snapshot']['tick']/60==row['seconds']and s['snapshot']['winner']==row['winner']
        assert not s['initialAi']and not s['administrativeEvents']and 0<row['seconds']<=3300
        assert row['winner']in[1,2]or row['seconds']==3300
        return dict(matrix=name,index=i,seconds=row['seconds'],unresolved=row['winner']is None)
    completed={'strategy':[],'branch':[]};failures=[];iterator=iter(jobs);began=time.monotonic()
    with concurrent.futures.ThreadPoolExecutor(max_workers=16)as pool:
        pending={pool.submit(run,next(iterator)):None for _ in range(16)}
        while pending:
            done,_=concurrent.futures.wait(pending,timeout=30,return_when=concurrent.futures.FIRST_COMPLETED)
            for future in done:
                pending.pop(future)
                try:
                    result=future.result();completed[result['matrix']].append(result)
                    print(json.dumps(dict(type='full-case-complete',strategyCompleted=len(completed['strategy']),branchFreshCompleted=len(completed['branch']),branchReused=80,**result)),flush=True)
                except BaseException as e:failures.append(repr(e));print(json.dumps(dict(type='full-case-failed',error=repr(e))),flush=True)
                if not failures:
                    next_job=next(iterator,None)
                    if next_job is not None:pending[pool.submit(run,next_job)]=None
            if not done:print(json.dumps(dict(type='full-progress',strategyCompleted=len(completed['strategy']),branchFreshCompleted=len(completed['branch']),branchReused=80,active=len(pending))),flush=True)
    after=sources();write(BASE/'source-after.json',after)
    unchanged=before==after and build.fingerprint()==FP
    for name in ['strategy','branch']:
        write(BASE/name/'source-after.json',after)
        write(BASE/name/'completion-receipt.json',dict(**start,matrix=name,endedAtUtc=now(),completedFreshCases=len(completed[name]),requiredFreshCases=len(job_lists[name]),reusedCases=plans[name]['reusedOriginalCases'],unchangedDuringMeasurement=unchanged,failures=failures,completed=completed[name],complete=len(completed[name])==len(job_lists[name])and not failures and unchanged))
    finish=dict(**start,endedAtUtc=now(),elapsedWallSeconds=time.monotonic()-began,completedStrategyCases=len(completed['strategy']),completedFreshBranchCases=len(completed['branch']),reusedBranchCases=80,failures=failures,unchangedDuringMeasurement=unchanged,complete=len(completed['strategy'])==126 and len(completed['branch'])==920 and not failures and unchanged)
    write(BASE/'completion-receipt.json',finish);assert finish['complete']
    print(json.dumps(dict(type='full-shared-complete',strategyCases=126,branchCases=1000,finalBalanceAcceptance=False)),flush=True)


if __name__=='__main__':main()
