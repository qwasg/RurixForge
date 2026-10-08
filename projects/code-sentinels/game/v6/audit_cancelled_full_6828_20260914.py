"""Inventory preserved outcomes after Root cancellation; never create run completion."""
from pathlib import Path
import datetime,hashlib,json
ROOT=Path(__file__).resolve().parents[2]
BASE=ROOT/'game/v6/final-balance-6828c5b7-20260914'
OUT=BASE/'root-requested-cancellation'
read=lambda p:json.loads(p.read_text(encoding='utf-8-sig'))
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
ref=lambda p:dict(path=p.relative_to(ROOT).as_posix(),sha256=sha(p),bytes=p.stat().st_size)
FP='6828c5b7f825b7e27b3e9df3c22d133ecefd8195a436f856c0682a33e580d781'


def verify_case(raw,saved,case):
    row=read(raw);state=read(saved)
    assert all(row[k]==case[k]for k in ['index','seed','theme','branches','strategies','swapped','plannerOrder'])
    assert row['originalPair']==case.get('originalPair')and row['simulationSourceSha256']==FP
    assert row['rulesVersion']=='v6.2'and row['planIndex']==case['index']
    assert 0<row['seconds']<=3300 and(row['winner']in[1,2]or row['seconds']==3300)
    assert state['rulesFingerprint']==FP and state['snapshot']['tick']/60==row['seconds']and state['snapshot']['winner']==row['winner']
    assert state['snapshot']['winReason']==row['winReason']and not state['initialAi']and not state['administrativeEvents']
    return row


def main():
    stopped=read(OUT/'process-exit-verification.json');assert stopped['allOwnedProcessesExited']
    before=read(OUT/'pre-stop-record.json');manifest=read(BASE/'measurement-manifest.json')
    reused={r['index']:r for r in read(BASE/'reuse-80-audit.json')['cases']}
    result={}
    for name in ['strategy','branch']:
        plan=read(BASE/name/'execution-plan.json');canonical=read(ROOT/plan['plan']['path']);cases={c['index']:c for c in canonical['cases']}
        valid=[];unreceipted=[];incomplete=[];issues=[]
        for i,case in sorted(cases.items()):
            reuse=name=='branch'and i in reused
            raw=BASE/name/(f'reused-results-{i:03}.jsonl'if reuse else f'results-{i:03}.jsonl')
            saved=BASE/name/f'diagnostics/match-{i}.save.json'
            execution=ROOT/reused[i]['originalExecution']['path']if reuse else BASE/name/f'execution-{i:03}.json'
            invocation=BASE/name/f'invocation-{i:03}.json'
            record=dict(index=i,reused=reuse,raw=ref(raw)if raw.exists()else None,save=ref(saved)if saved.exists()else None,execution=ref(execution)if execution.exists()else None)
            if raw.exists()and raw.stat().st_size>0 and saved.exists():
                try:
                    row=verify_case(raw,saved,case)
                    assert row['planSha256']==plan['plan']['sha256']
                    record.update(seconds=row['seconds'],winner=row['winner'],winReason=row['winReason'],unresolvedAtBudget=row['winner']is None)
                    if execution.exists():
                        ex=read(execution);assert ex['exitCode']==0 and ex['rulesFingerprint']==FP
                        assert ex['raw']['sha256']==sha(raw)and ex['save']['sha256']==sha(saved)
                        assert ex['runner']['sha256']==manifest['runner']['sha256']
                        valid.append(record)
                    else:
                        record['status']='verified-native-outcome-and-Save-without-orchestrator-exit-receipt';unreceipted.append(record)
                except (AssertionError,ValueError,KeyError)as exc:
                    record['status']='preserved-evidence-requires-review-after-root-cancellation';record['observation']=str(exc);issues.append(record);incomplete.append(dict(index=i,status=record['status']))
            else:
                record['status']='started-but-no-complete-native-outcome-at-root-cancellation'if invocation.exists()or raw.exists()else'not-started-before-root-cancellation'
                incomplete.append(record)
        valid_ids={r['index']for r in valid};recorded_ids={r['index']for r in unreceipted}
        result[name]=dict(requiredCanonicalCases=len(cases),validFullyReceiptedCompletedCases=len(valid),validFreshCompletedCases=sum(not r['reused']for r in valid),validReusedCases=sum(r['reused']for r in valid),verifiedOutcomesWithoutExitReceipt=len(unreceipted),fullyReceiptedCompletedCaseIndices=sorted(valid_ids),outcomeOnlyCaseIndices=sorted(recorded_ids),notFullyReceiptedCaseIndices=sorted(set(cases)-valid_ids),noVerifiedCompleteOutcomeCaseIndices=sorted(set(cases)-valid_ids-recorded_ids),validCompletedCases=valid,outcomesWithoutExitReceipt=unreceipted,incompleteCases=incomplete,evidenceReviewIssues=issues,originalExecutionPlan=ref(BASE/name/'execution-plan.json'))
        print(json.dumps(dict(matrix=name,validFullyReceipted=len(valid),fresh=sum(not r['reused']for r in valid),reused=sum(r['reused']for r in valid),outcomeWithoutExitReceipt=len(unreceipted),notCompleted=len(set(cases)-valid_ids-recorded_ids))),flush=True)
    report=dict(recordedAtUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),status='root-requested-cancellation',reason='Stop this6828 full branch validation to correct newly confirmed approved-plan inconsistencies in roomHP area scaling and DC merge capacity conservation. Cancellation was not selected by partial win rates.',originalRulesFingerprint=FP,originalManifest=ref(BASE/'measurement-manifest.json'),preStopEvidence=ref(OUT/'pre-stop-record.json'),stopActions=ref(OUT/'stop-actions.json'),processExitEvidence=ref(OUT/'process-exit-verification.json'),allRecordedSourcesMatchedOriginalBeforeStop=before['allRecordedSourcesMatchOriginal'],allOwnedRunnerAndOrchestratorProcessesExited=True,matrices=result,fullBranchMatrixCompleted=False,finalBalanceAcceptance=False,originalDataModified=False,originalRunCompletionReceiptCreated=False,preservedCompleteStrategyHistory=dict(earlyCompletion=ref(BASE/'strategy/early-review/completion-receipt.json'),analysis=ref(BASE/'strategy/early-review/analysis.json'),strictAudit=ref(BASE/'strategy/early-review/strict-integrity-check.json'),pairedAnalysis=ref(BASE/'strategy/early-review/paired-strategy-analysis.json')),notes=['Only existing exit0 execution receipts plus exact raw/Saved outcome identity count as fully receipted completed cases. No synthetic exit codes or game outcomes are created.','Native outcome/Save pairs lacking an orchestrator exit receipt remain separate pending review, not relabelled success/failure.','Incomplete or not-started cases remain listed. The original shared final completion and final branch analysis are not fabricated.','The complete126 strategy early report and all80 original probe reuse receipts remain valid historical6828 observations; they are not relabelled as future rule results.'])
    assert not(BASE/'completion-receipt.json').exists()
    with(OUT/'cancellation-receipt.json').open('x',encoding='utf-8')as f:json.dump(report,f,ensure_ascii=False,indent=2)
    print(json.dumps(dict(receipt=str(OUT/'cancellation-receipt.json'),sha256=sha(OUT/'cancellation-receipt.json'),sourceMatched=before['allRecordedSourcesMatchOriginal'],allOwnedProcessesExited=True)),flush=True)


if __name__=='__main__':main()
