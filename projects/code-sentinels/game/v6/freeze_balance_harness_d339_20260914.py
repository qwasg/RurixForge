"""Freeze the reviewed control harness and every immutable measurement input."""
from pathlib import Path
import hashlib
import json
import shutil
import build_direct_range_probe_runner_20260913 as utility
import balance_dispatch_control_d339_20260914 as control

ROOT = Path(__file__).resolve().parents[2]
BASE = ROOT / 'game/v6/final-balance-d33999c4-20260914'
FP = 'd33999c4bf39fea8de5f89df32e2a59c3fc2d242ba2a433078f347c6779f0d35'
read, sha, ref, write, now = utility.read, utility.sha, utility.ref, utility.write, utility.now


def main():
    assert not (BASE / 'measurement-manifest.json').exists()
    assert utility.fingerprint() == FP
    build = read(BASE / 'runner-build-receipt.json')
    assert build['exitCode'] == 0 and build['sourceUnchanged'] and build['rulesFingerprintAfter'] == FP
    executable = ROOT / build['artifact']['path']
    assert sha(executable) == build['artifact']['sha256']
    tests_path = ROOT / 'game/v6/dispatch-control-d339-tests-20260914-a/receipt.json'
    assert sha(tests_path) == '44497bb8051b40b784602119227a2599efecbb7ff6dae15a461eb49b8e40e263'
    assert sha(ROOT / 'game/v6/balance_dispatch_control_d339_20260914.py') == '529e3bb4a3b63f539cdfbb64e46bad539bf2d88e5134bde6a7f4373c28d24503'
    assert sha(ROOT / 'game/v6/balance_dispatch_queue_d339_20260914.py') == '071bdab0a7dbfd14b2fa39fc8d6b94ecdf2e5bcb6b2351ad4ddc83e694743fcd'
    harness = []
    for name in ['shared_balance_harness_d339_20260914.py', 'balance_dispatch_control_d339_20260914.py', 'balance_dispatch_queue_d339_20260914.py']:
        source, destination = ROOT / 'game/v6' / name, BASE / 'harness' / name
        assert not destination.exists()
        shutil.copy2(source, destination)
        assert sha(source) == sha(destination)
        harness.append(ref(destination))
    initial = control.set_control(BASE / 'control.json', 'resume', 'Initial authorized fresh d339 verification.')
    shutil.copy2(BASE / 'control.json', BASE / 'control-initial.json')
    matrices, lists = {}, {}
    for name, required in [('strategy', 126), ('branch', 1000)]:
        plan_path = BASE / name / f'{name}-plan.json'
        plan = read(plan_path)
        assert len(plan['cases']) == required and {case['index'] for case in plan['cases']} == set(range(required))
        jobs = []
        for case in plan['cases']:
            index = case['index']
            command = [str(executable), '--plan', str(plan_path), '--first', str(index), '--limit', '1', '--minutes', '55',
                       '--source-hash', FP, '--stop-on-failure', 'true', '--out', str(BASE / name / f'results-{index:03}.jsonl'),
                       '--diagnostics', str(BASE / name / 'diagnostics')]
            jobs.append(dict(matrix=name, index=index, case=case, command=command))
        path = BASE / name / 'execution-plan.json'
        write(path, dict(matrix=name, requiredTotalCases=required, freshCases=required, reusedOriginalCases=0,
                         plan=ref(plan_path), jobs=jobs, maxConcurrentProcesses=16))
        matrices[name] = dict(executionPlan=ref(path), inputPlan=ref(plan_path), requiredCases=required,
                              freshCases=required, reusedCases=0)
        lists[name] = jobs
    ordered = []
    for ordinal in range(1000):
        for name in ['strategy', 'branch']:
            if ordinal < len(lists[name]):
                ordered.append(lists[name][ordinal])
    dispatch_path = BASE / 'dispatch-plan.json'
    write(dispatch_path, dict(scope='Fixed interleaving, one shared16-process cap. All1126 cases fresh.', jobs=ordered))
    identity_command = [str(executable), '--source-hash', FP, '--maps', 'true', '--seeds', '1', '--out', str(BASE / 'identity/map-audit.json')]
    frozen_inputs = [ref(BASE / 'preparation.json'), ref(BASE / 'runner-build-receipt.json'), ref(BASE / 'compiled-source-before.json'),
                     ref(BASE / 'control-initial.json'), ref(dispatch_path), ref(tests_path), build['artifact'], *harness]
    frozen_inputs.extend([ref(ROOT / 'game/v6/summarize_balance.py'), ref(ROOT / 'game/v6/release_gate.py'),
                          ref(ROOT / 'V6-APPROVED-PLAN.md'), ref(ROOT / 'game/v6/RELEASE-ACCEPTANCE-SCHEMA.md')])
    for matrix in matrices.values():
        frozen_inputs.extend([matrix['executionPlan'], matrix['inputPlan']])
    manifest = dict(schemaVersion=1, frozenAtUtc=now(), workspaceRoot=str(ROOT), rulesVersion='v6.2', rulesFingerprint=FP,
                    runner=build['artifact'], runnerBuild=ref(BASE / 'runner-build-receipt.json'),
                    compiledSource=ref(BASE / 'compiled-source-before.json'), matrices=matrices,
                    dispatchPlan=ref(dispatch_path), identityCommand=identity_command, harness=harness,
                    controllerTests=ref(tests_path), initialControl=ref(BASE / 'control-initial.json'),
                    mutableControlPath=str(BASE / 'control.json'), immutableInputs=frozen_inputs,
                    maxGameWorkers=16, allCasesFresh=True, reusedRows=0, mutatesGameSource=False,
                    aggregator=ref(ROOT / 'game/v6/summarize_balance.py'), gate=ref(ROOT / 'game/v6/release_gate.py'),
                    scope='Frozen ordinary-Game d339 verification and audited dispatch controls, not release/balance/performance approval.')
    write(BASE / 'measurement-manifest.json', manifest)
    protocol = {
        'controlFile': str(BASE / 'control.json'), 'statusFile': str(BASE / 'status.json'),
        'setter': str(BASE / 'harness/balance_dispatch_control_d339_20260914.py'),
        'schema': {'schemaVersion': 1, 'revision': 'monotonically increasing integer',
                   'command': 'resume | pauseDispatch | stopAfterActive', 'requestedAtUtc': 'UTC ISO string', 'reason': 'plain text'},
        'commands': {
            'pauseDispatch': 'Stop new dispatch after observation, let every active native match finish its original budget/outcome, then remain alive waiting.',
            'resume': 'Continue the same queue in the same live harness; never rerun completed cases or restart an exited attempt.',
            'stopAfterActive': 'Latched: finish active cases naturally, cancel unstarted queue, preserve all existing data, then exit. Resume cannot undo an observed stop.',
        },
        'exclusiveWindowCondition': 'Wait for status details.pausedAndDrained true, activeGameWorkers0, identityPid null, and verify the status control revision/hash matches your pause request. CPU release alone does not authorize GPU/LAN work.',
        'audit': 'Every observed raw control byte change is retained and hashed with observation UTC. Each case binds its exact control snapshot at dispatch.',
        'invalidControl': 'Missing, malformed, unknown fields or nonmonotonic revision fail closed to paused dispatch. Existing games are never killed.',
        'simulation': 'Control does not alter seed, planner order, costs, rules,55minute simulation budget, ticks or winner. Paused waiting affects harness wall time only.',
        'firstIdentity': 'Exactly the frozen existing-runner map-audit invocation. If OS4551 or another launch error occurs, stop and record; no alternate names, paths or arguments are attempted.',
        'notYetObserved': '23 Python control/queue tests passed; no real matrix pause/resume cycle is claimed before execution.',
    }
    write(BASE / 'control-protocol.json', protocol)
    print(json.dumps(dict(frozen=True, manifest=ref(BASE / 'measurement-manifest.json'), harness=harness,
                          controlProtocol=ref(BASE / 'control-protocol.json'), identityStarted=False, matrixStarted=False)), flush=True)


if __name__ == '__main__':
    main()
