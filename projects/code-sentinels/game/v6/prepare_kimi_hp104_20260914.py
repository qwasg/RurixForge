"""Prepare the fixed Kimi durability comparison; never compile or run native code."""
from pathlib import Path
from collections import Counter
import hashlib
import json
import shutil
import sys

ROOT = Path(__file__).resolve().parents[2]
ORIGIN = ROOT / 'game/v6/logistics-probe200-20260914-a'
REUSED = ORIGIN / 'candidate-attempt-01'
OUT = ROOT / 'game/v6/kimi-hp104-20260914-a'
BASELINE_FP = '864456623cd05308893eee82474c0e246d190ce10c3c449894f3726593c85edb'
VARIANT_TOKEN = 'REQUIRED_FUTURE_FROZEN_KIMI_HP357_RULES_FINGERPRINT'
sys.path.insert(0, str(REUSED / 'harness-v2'))
import probe200_support as prior


def sha(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream,'sha256').hexdigest()


def ref(path):
    path=path.resolve()
    assert path.is_relative_to(ROOT)
    return dict(path=path.relative_to(ROOT).as_posix(),sha256=sha(path),bytes=path.stat().st_size)


def read(path):
    return json.loads(path.read_text(encoding='utf-8-sig'))


def write(path,value):
    with path.open('x',encoding='utf-8',newline='\n') as stream:
        json.dump(value,stream,ensure_ascii=False,indent=2,allow_nan=False)
        stream.write('\n')


def main():
    assert not OUT.exists(), 'Existing registration must not be overwritten.'
    origin_registration=ORIGIN/'pre-registration.json'
    origin_plan=ORIGIN/'canonical-branch-plan.json'
    assert sha(origin_registration)=='8e426f289c819fda9a57f081639672acbfbaa5f674db0ecd0cc6d72680f7df69'
    assert sha(origin_plan)=='cae20640404e86e10dc49dc68983a79e2852cd42a04df6ac64edba09dcc39eb8'
    old,plan=read(origin_registration),read(origin_plan)
    origin_cases=prior.validate_selection(old,plan)
    selected=[binding['case'] for binding in origin_cases if 'speed' in binding['case']['branches'] or binding['case']['seed']==1000]
    speed=[case for case in selected if 'speed' in case['branches']]
    controls=[case for case in selected if 'speed' not in case['branches']]
    assert len(selected)==len({case['index'] for case in selected})==104
    assert len(speed)==80 and len(controls)==24 and all(case['seed']==1000 for case in controls)
    assert selected==[case for case in plan['cases'] if case['index'] in {c['index'] for c in selected}]
    assert [case['index'] for case in controls]==[280,281,320,321,360,361,440,441,520,521,560,561,640,641,680,681,760,761,840,841,880,881,920,921]
    glm=ROOT/'game/v6/glm-support-fix-20260914-a'
    frozen=read(glm/'source-freeze-blocked-green-receipt.json')
    assert frozen['compiledRulesFingerprint']==BASELINE_FP
    assert frozen['sourceManifest']['sha256']=='214a93290b2dec24d19271fdd4cf14ab6bd5258ed485ed2e6b043c87dc4bfa07'
    source_manifest=ROOT/frozen['sourceManifest']['path']
    assert sha(source_manifest)==frozen['sourceManifest']['sha256']
    native_source_check=prior.verify_native_manifest(read(source_manifest))
    assert prior.fingerprint()==BASELINE_FP, 'Freeze the exact GLM-only source, not58b or the proposed HP variant.'
    OUT.mkdir()
    shutil.copy2(origin_plan,OUT/'canonical-branch-plan.json')
    assert sha(OUT/'canonical-branch-plan.json')==sha(origin_plan)
    before=prior.sources(compiled_only=True)
    write(OUT/'baseline-compiled-source.json',before)
    for record in before:
        destination=OUT/'baseline-source'/record['path']
        destination.parent.mkdir(parents=True,exist_ok=True)
        shutil.copy2(ROOT/record['path'],destination)
        assert sha(destination)==record['sha256']
    registration=dict(schemaVersion=1,experimentId='kimi-hp104-glm864456-v1',recordedAtUtc=prior.now(),
        status='preregistered-execution-held',scope='Kimi baseHP420to357 single-parameter hypothesis, following a validated GLM-only baseline; not final balance acceptance.',
        original200Registration=ref(origin_registration),canonicalPlan=ref(OUT/'canonical-branch-plan.json'),
        selector="Original200 in canonical order where speed in branches OR seed==1000; classify the non-speed seed1000 cases as controls.",
        caseCountPerGeneration=104,primarySpeedCaseCount=80,nonSpeedControlCaseCount=24,plannedNativeCaseInvocations=208,
        indices=[case['index'] for case in selected],speedIndices=[case['index'] for case in speed],controlIndices=[case['index'] for case in controls],
        cases=[dict(index=case['index'],cohort='speed-effect' if 'speed' in case['branches'] else 'non-speed-control',case=case) for case in selected],
        baseline=dict(rulesVersion='v6.2',rulesFingerprint=BASELINE_FP,sourceManifest=ref(source_manifest),
            compiledSource=ref(OUT/'baseline-compiled-source.json'),sourceValidation=native_source_check,
            sourceFreezeEvidence=ref(glm/'source-freeze-blocked-green-receipt.json'),focusedGreenHistoricalBlock=ref(glm/'green-blocked-receipt.json'),
            focusedGreenExecutedAtThisFreeze=0,focusedGreenPassedAtThisFreeze=False,validatedForMeasurement=False,
            requiredNextEvidence='Actual focused GREEN at exact864456 plus applicable native validation, actual matching host/runner identities, and later explicit Root START.'),
        variant=dict(rulesVersion='v6.2',rulesFingerprint=None,compiledSource=None,runner=None,
            onlyPermittedProductionChange=dict(unit='kimi',field='T2 base HP',before=420,after=357,factor=0.85),
            preserved=['range16','baseSpeed4.8','vision26','burst and dash mechanics','price and compute costs','tier scaling','all other units','GLM-only bot correction','projectile physics','logistics','victory rules'],
            requiredIdentity='A real future native source freeze and compiled/observed fingerprint; never infer or fill from58b,864456, or a proposed patch.'),
        simulation=dict(gameStepHz=60,budgetMinutes=55,budgetTicks=198000,maxConcurrentGameWorkersAcrossAllGenerations=16,
            first='baseline104',then='variant104 only after baseline has naturally completed/drained and the one-parameter source/binary is frozen',
            originalSeedsBranchesPlannerAndOrder=True,initialAi=False,administrativeEvents=[],injectedResources=False,
            forceWinner=False,unresolved='Keep winner null at exact3300 simulatedseconds; preserve raw/Save and do not silently extend or replace.'),
        gatesHeld=['Focused864456 GREEN actual execution still required','Actual system trust/execution availability','Matching approved native/runner source and identity','Later Root build/start authorization'],
        noBuildPerformed=True,noNativeIdentityPerformed=True,noSimulationPerformed=True,finalBalanceAcceptance=False)
    write(OUT/'pre-registration.json',registration)
    source_names=['shared_balance_harness_logistics200_20260914.py','probe200_support.py','build_freeze_logistics_probe200.py',
                  'balance_dispatch_control_d339_20260914.py','balance_dispatch_queue_d339_20260914.py']
    harness_refs=[ref(REUSED/'harness-v2'/name) for name in source_names]
    assert next(r['sha256'] for r in harness_refs if r['path'].endswith('balance_dispatch_control_d339_20260914.py'))=='529e3bb4a3b63f539cdfbb64e46bad539bf2d88e5134bde6a7f4373c28d24503'
    assert next(r['sha256'] for r in harness_refs if r['path'].endswith('balance_dispatch_queue_d339_20260914.py'))=='071bdab0a7dbfd14b2fa39fc8d6b94ecdf2e5bcb6b2351ad4ddc83e694743fcd'
    write(OUT/'harness-reuse-contract.json',dict(recordedAtUtc=prior.now(),existingVerifiedHarness=harness_refs,
        originalReal200Completion=ref(REUSED/'completion-receipt.json'),originalStrictAudit=ref(REUSED/'strict-review/strict-integrity-check.json'),
        controlQueueTests=ref(ROOT/'game/v6/dispatch-control-d339-tests-20260914-a/receipt.json'),
        real175RepositoryRootRegression=ref(REUSED/'offline-guard-tests-v2.json'),
        controlSemantics='Exact existing controller/queue modules: pauseDispatch drains and waits, resume same cursor, stopAfterActive latches and cancels only unstarted jobs; immutable control byte/UTC/SHA history and per-dispatch binding.',
        evidenceRoots=dict(projectRoot=str(ROOT),native175SourceInventoryRoot=native_source_check['repositoryRoot']),
        minimalExperimentAdapter=['104 counts and this exact selector; no global200-to104 replacement',
            'Baseline864456 versus future explicit variant FP and frozen manifests',
            'These unique generation output paths and later real START authorization binding',
            'Baseline source exact864456; variant production diff only catalog Kimi HP420to357 against the exact baseline source',
            'Freeze adapter SHA and run only identity/selection pure Python guards before any authorized native invocation'],
        schedulerReimplementation=False,oldFrozenFilesModified=False,
        warning='The archived200 main/support have hardcoded200/58b identities and are provenance/reuse inputs, not directly runnable104 entrypoints. Only the small experiment identity/count adapter is pending the later build/start freeze; scheduling and repo-root logic remain unchanged.',
        canonicalArithmetic='The full1000 plan still uses index//200 and(index//40)%5. Keep it byte-identical; do not pass a renumbered104 subset to native --plan.'))
    executions={}
    for generation,fp in [('baseline',BASELINE_FP),('variant',VARIANT_TOKEN)]:
        destination=OUT/f'{generation}-attempt-01'
        destination.mkdir()
        executable=destination/'artifact/sentinels-v6-balance-runner.exe'
        jobs=[]
        for entry in registration['cases']:
            index=entry['index']
            jobs.append(dict(generation=generation,index=index,cohort=entry['cohort'],case=entry['case'],
                command=[str(executable),'--plan',str(OUT/'canonical-branch-plan.json'),'--first',str(index),'--limit','1',
                         '--minutes','55','--source-hash',fp,'--stop-on-failure','true',
                         '--out',str(destination/'branch'/f'results-{index:03}.jsonl'),'--diagnostics',str(destination/'branch/diagnostics')],
                requiredActualEvidence=dict(invocation=str(destination/'branch'/f'invocation-{index:03}.json'),
                    process=str(destination/'branch'/f'process-{index:03}.json'),execution=str(destination/'branch'/f'execution-{index:03}.json'),
                    raw=str(destination/'branch'/f'results-{index:03}.jsonl'),save=str(destination/'branch/diagnostics'/f'match-{index}.save.json'))))
        assert [job['index'] for job in jobs]==registration['indices'] and len(jobs)==104
        execution=destination/'execution-template.json'
        write(execution,dict(generation=generation,templateOnly=True,launchAuthorized=False,requiredFreshCases=104,reusedCasesNow=0,
            expectedRulesFingerprint=fp if generation=='baseline' else None,runnerSha256=None,sourceBefore=None,sourceAfter=None,
            completeCanonicalPlan=registration['canonicalPlan'],preRegistration=ref(OUT/'pre-registration.json'),jobs=jobs,
            maxGameWorkers=16,identityCommand=[str(executable),'--source-hash',fp,'--maps','true','--seeds','1','--out',str(destination/'identity/map-audit.json')],
            firstIdentity='One actual original frozen runner invocation per generation after later START. Any OS launch refusal is retained and halts; no automatic renamed/recompiled/path/argument workaround.',
            perCaseVerification='Exit0; positive PID; exact original metadata and full plan SHA; actual compiled/Save FP; Save tick/60==row seconds; native outcome matches; no initialAI/admin; control snapshot, runnerSHA, raw/Save/execution hashes retained.'))
        executions[generation]=ref(execution)
        write(destination/'control-initial-template.json',dict(schemaVersion=1,revision=0,command='pauseDispatch',requestedAtUtc=prior.now(),
             reason='Preregistration only. Native validation/system execution and later Root START are pending.'))
    reuse=dict(recordedAtUtc=prior.now(),policy='Preserve original bytes and identities; no cross-rule or cross-case substitutions.',
        reusableNow=['Full canonical plan bytes and original metadata','Existing exact controller/queue and verified repo-root resolver logic','Historical sources and reports strictly as historical context'],
        nonReusableAs864456OrHpVariant=['All d339,58b,6828,0ac raw/Save/execution files','Any alleged equivalent24 control results from another fingerprint','Synthetic fixtures, runtime continuations, renamed indices, shortened budgets, or altered result headers'],
        sameRuleReuseLater='Only after actual validated execution exists: exact full FP, original canonical index/metadata/plan SHA,55min/60Hz rules, full source/runner identity and actual invocation/Save/exit evidence; byte-verify original rows, keep original references/metadata, count once, and obtain explicit Root approval for the receiving matrix.',
        plannedFreshDefault='Both generations currently have0 eligible rows; execute104+104 fresh cases. Never reuse baseline864456 control rows as variant rows even if expected behavior is unchanged.',
        afterPause='Resume the same live cursor; preserve completed/in-flight cases. StopAfterActive records unstarted indices without fabricated failures/results; an exited attempt is not silently restarted.',
        sameFingerprintDifferentBinary='No automatic equivalence based on source alone; actual runner SHA and execution provenance require an explicit identity audit and Root approval.',
        compareControls='Retain both original Save/raw bytes. Compare actual24 paired controls separately; any optional normalized comparison must state that only outer rulesFingerprint and raw wallSeconds/simulationSourceSha256 are excluded. Never relabel original artifacts or hide unexpected Kimi deployment/control differences.',
        finalUse='104 is a structured diagnostic, not an equal-weight five-branch matrix. If a future final1000 reuses eligible same-rule rows, it still requires exactly1000 unique canonical cases and all126 strategies on the accepted final rules.')
    write(OUT/'reuse-policy.json',reuse)
    owner_counts=Counter((case['branches'][owner-1],owner) for case in selected for owner in [1,2])
    cohorts=dict(caseCount=104,speedCases=80,controlCases=24,seeds=dict(Counter(case['seed'] for case in selected)),
                 themes=dict(Counter(case['theme'] for case in selected)),swaps=dict(Counter(str(case['swapped']) for case in selected)),
                 playerObservations=dict(Counter(branch for case in selected for branch in case['branches'])),
                 playerSideCounts={branch:{str(owner):owner_counts[branch,owner] for owner in [1,2]} for branch in prior.BRANCHES})
    assert cohorts['seeds']=={1000:40,1005:16,1010:16,1014:16,1019:16}
    assert cohorts['themes']=={'river':40,'highland':32,'mining':32}
    assert cohorts['playerObservations']=={'speed':80,'security':32,'algorithm':32,'science':32,'lightweight':32}
    write(OUT/'selection-audit.json',dict(recordedAtUtc=prior.now(),passed=True,counts=cohorts,fullCanonicalMatches=True,
        duplicateIndices=0,controlIndices=registration['controlIndices'],preRegistration=ref(OUT/'pre-registration.json'),
        sourceFingerprintObserved=prior.fingerprint(),runtimeValidationNotClaimed=True,helper=ref(Path(__file__)),
        statisticalInterpretation='Primary speed80 and negative controls24 reported separately; do not pool104 to claim equal branch balance.',finalBalanceAcceptance=False))
    write(OUT/'preparation-receipt.json',dict(recordedAtUtc=prior.now(),completePreparation=True,executions=executions,
        preRegistration=ref(OUT/'pre-registration.json'),selectionAudit=ref(OUT/'selection-audit.json'),
        reusePolicy=ref(OUT/'reuse-policy.json'),harnessReuse=ref(OUT/'harness-reuse-contract.json'),
        sourceBaseline=ref(OUT/'baseline-compiled-source.json'),preparationHelper=ref(Path(__file__)),
        immutableInputs=[ref(OUT/'canonical-branch-plan.json'),ref(OUT/'pre-registration.json'),ref(origin_registration),*harness_refs],
        realNativeMatchesStarted=0,buildStarted=False,identityStarted=False,GameSourceChanged=False,
        nextAction='Wait for actual GLM864456 validation/system execution and later Root instructions. Freeze the small104 identity adapter and actual binary/manifest before authorized build/launch.',
        finalBalanceAcceptance=False))
    assert sha(origin_registration)=='8e426f289c819fda9a57f081639672acbfbaa5f674db0ecd0cc6d72680f7df69'
    assert prior.fingerprint()==BASELINE_FP
    print(json.dumps(dict(preparation=ref(OUT/'preparation-receipt.json'),registration=ref(OUT/'pre-registration.json'),counts=cohorts,
                          plannedRealCases=208,buildStarted=False,identityStarted=False,simulationStarted=False)),flush=True)


if __name__=='__main__':main()
