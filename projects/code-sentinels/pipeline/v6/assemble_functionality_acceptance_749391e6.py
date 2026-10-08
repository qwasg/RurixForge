"""Verify completed mixed-configuration native correctness evidence and assemble its scope report.

No executable is launched, rebuilt, moved, signed or reclassified here. This is
not a global release approval and cannot create a ZIP.
"""
from collections import Counter
from copy import deepcopy
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import re
import sys

PROJECT = Path(__file__).resolve().parents[2]
REPO = PROJECT.parents[1]
sys.path.insert(0, str(PROJECT / 'game/v6'))
import release_gate as gate
import portable_release as package


def ref(file):
    return {'path': file.relative_to(PROJECT).as_posix(), 'sha256': gate.sha(file)}


def unique(refs):
    result, seen = [], set()
    for item in refs:
        path = Path(item['path'])
        path = (path if path.is_absolute() else PROJECT / path).resolve()
        value = ref(path)
        assert value['sha256'] == item['sha256'].lower()
        if value['path'] not in seen:
            seen.add(value['path'])
            result.append(value)
    return result


def main():
    run = PROJECT / 'game/v6/final-contracts-749391e6-20260911-a'
    candidate_path = PROJECT / 'game/v6/functionality-candidate-749391e6-20260911.json'
    contract_path = run / 'contract-execution-with-existing-debug.json'
    candidate = gate.read_json(candidate_path)
    candidate_hash = gate.sha(candidate_path)
    contract = gate.read_json(contract_path)
    target = gate.read_json(PROJECT / 'dist/final-candidate-749391e6-20260911/CodeSentinels-V6-Windows/v6-candidate.json')['target']
    gate.identity(candidate, target, 'prior candidate', payload=True)
    gate.identity(contract, target, 'same-rule native contracts')
    assert candidate['finalEligible'] is False and candidate['nativeTests']['unexecutedTests'] == 84
    assert (contract['passedTests'], contract['failedTests'], contract['unexecutedTests']) == (222, 0, 0)
    assert contract['completeRequiredTestCoverage'] is True and contract['releaseConfigurationUnexecutedTests'] == 84
    store = gate.EvidenceStore(PROJECT)
    for i, item in enumerate(contract['evidence']):
        store.file(item, 'source-evidence-' + str(i), publish=not item['path'].endswith('.log'))
    library = next(r for r in contract['records'] if r['target'] == 'unittests src/lib.rs')
    assert library['status'] == 'executed-existing-original-debug'
    debug_path = PROJECT / library['artifact']['path']
    debug_bytes = debug_path.read_bytes()
    debug_hash = hashlib.sha256(debug_bytes).hexdigest()
    assert debug_hash == library['artifact']['sha256'] == '7f6fd53be4875f8c16c14566b373b2d5bf9c5a35832304077fb58506f779742e'
    result = gate.read_json(run / 'existing-debug-lib-result.json')
    source_identity = gate.read_json(run / 'existing-debug-source-identity.json')
    old_verification = gate.read_json(run / 'existing-debug-lib-verification.json')
    assert result['exitCode'] == 0 and result['unchanged'] is True
    assert result['beforeSha256'].lower() == result['afterSha256'].lower() == debug_hash
    assert Path(source_identity['originalArtifact']).resolve() == debug_path.resolve()
    assert source_identity['currentSha256'] == old_verification['sha256'].lower() == debug_hash
    assert source_identity['historicStandaloneShaCaptured'] is False and old_verification['historicalStandaloneShaWasRecorded'] is False
    assert source_identity['embeddedFingerprint'] == target['rulesFingerprint']
    assert debug_bytes.count(target['rulesFingerprint'].encode()) == source_identity['embeddedCurrentOccurrences'] == 3
    assert len(source_identity['sourceRecords']) == source_identity['sourceCount'] == 33
    for record in source_identity['sourceRecords']:
        assert gate.sha(PROJECT / record['path']) == record['actual'] == record['expected'] and record['matched'] is True
    rules_root = PROJECT / 'native-v6'
    files = list((rules_root / 'src').rglob('*.rs')) + [rules_root / name for name in ('Cargo.toml', 'Cargo.lock', 'build.rs')]
    calculated = hashlib.sha256(b'code-sentinels-native-rules-v1\0')
    for file in sorted(files, key=lambda p: p.relative_to(rules_root).as_posix()):
        name = file.relative_to(rules_root).as_posix().encode(); data = file.read_bytes()
        calculated.update(len(name).to_bytes(8, 'little')); calculated.update(name)
        calculated.update(len(data).to_bytes(8, 'little')); calculated.update(data)
    assert calculated.hexdigest() == target['rulesFingerprint']
    build_path = PROJECT / 'game/v6/claude-freeze-host-receipt-20260911.json'
    build = gate.read_json(build_path)
    gate.validate_build(build, target, store, REPO)
    assert build['source']['fileCount'] == 167
    source_manifest_hash = gate.sha(PROJECT / build['source']['afterManifest'])
    assert source_manifest_hash == 'e9e876aed0dcdc427b1e7f4412941e612dbe356696ce37a3013bc0ffd478a347'
    # The three included library helper modules predate this unchanged debug program.
    helper_sources = []
    for name in ('bot_ai_contracts.rs', 'logistics_contracts.rs', 'traits_contracts.rs'):
        file = rules_root / 'tests/support' / name
        assert file.stat().st_mtime <= debug_path.stat().st_mtime
        helper_sources.append(ref(file))
    log = (run / 'existing-debug-lib-executed.log').read_text(encoding='utf-8-sig')
    passed = re.findall(r'^test ([^\s]+) \.\.\. ok\s*$', log, re.MULTILINE)
    ignored = re.findall(r'^test ([^\s]+) \.\.\. ignored', log, re.MULTILINE)
    assert len(passed) == len(set(passed)) == 84 and set(passed) == set(library['tests'])
    assert len(ignored) == 2 and set(ignored) == set(library['ignoredTests'])
    assert 'test result: ok. 84 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 772.66s' in log
    prior_log = (PROJECT / old_verification['priorExecutionLog']).read_text(encoding='utf-8-sig')
    prior = re.findall(r'^test ([^\s]+) \.\.\. ok\s*$', prior_log, re.MULTILINE)
    assert len(prior) == len(set(prior)) == 17 and set(prior) <= set(passed)
    assert library['priorTargetedAddedToTotal'] == 0 and library['priorTargetedOverlap'] == 17
    groups = dict(Counter(name.rsplit('::', 1)[0] for name in passed))
    assert groups == library['groups']
    assert groups['logistics::logistics_contracts'] == 23 and groups['logistics::tests'] == 5
    original_log = (run / 'cargo-tests.log').read_text(encoding='utf-8-sig').splitlines()
    integration = [r for r in contract['records'] if r is not library]
    for record in integration:
        if record['status'] == 'executed':
            text = original_log[record['resultLogLine'] - 1]
        else:
            assert record['status'] == 'executed-on-one-original-path-retry'
            text = (PROJECT / record['retry']['evidence']['path']).read_text(encoding='utf-8-sig')
        assert re.search(r'test result: ok\. ' + str(record['passed']) + r' passed; 0 failed;', text)
    assert sum(r['passed'] for r in integration) == 138
    release_refusal = library['releaseConfiguration']
    assert release_refusal['status'] == 'os-blocked-unexecuted' and release_refusal['error'] == 4551
    frozen_paths = [PROJECT / 'game/build_portable_v6.py'] + [PROJECT / 'game/v6' / name for name in (
        'portable_release.py', 'release_gate.py', 'strategy-plan.json', 'README.md', 'RELEASE-ACCEPTANCE-SCHEMA.md')]
    frozen = {file.relative_to(PROJECT).as_posix(): gate.sha(file) for file in frozen_paths}
    report = deepcopy(candidate)
    report.update(recordedAtUtc=datetime.now(timezone.utc).isoformat(), finalEligible=True,
                  status='functional-scope-accepted-with-explicit-test-configurations',
                  statusDetail='功能专项必需测试去重后222项实际通过：138项release集成测试与84项同源码既有debug库测试。此前release库的4551拒绝仍单列为该构建配置未执行，未改写为通过；不构成全局发布批准。')
    report['scope'] = 'Final functionality scope only: actual same-rule native required-test coverage, ordinary paid opening, CUA/native UI and explicit image evidence. Mixed test configurations are identified. This does not approve global release, balance, performance or45-minute LAN.'
    report['previousCandidate'] = ref(candidate_path)
    tests = report['nativeTests']
    tests.update(passedTests=222, failedTests=0, unexecutedTests=0, requiredUniqueTests=222,
                 releaseIntegrationPassedTests=138, existingDebugLibraryPassedTests=84,
                 priorTargetedLibraryTests=17, priorTargetedOverlap=17, priorTargetedAddedToTotal=0,
                 releaseConfigurationUnexecutedTests=84, coverage=sorted(gate.FUNCTION_TAGS),
                 pendingCoverage=[], partialCoverage={},
                 coverageMeaning='All required logical cases executed once across138 release integration cases and84 same-source existing debug library cases. The historical optimized release-library artifact was not executed; this is correctness coverage, not debug performance evidence.')
    tests.pop('blockedTargets', None)
    tests['historicalUnexecutedInvocations'] = [release_refusal]
    tests['executedTargets'] = [{'target': r['target'], 'passed': r['passed'], 'status': r['status'],
                                'configuration': r.get('configuration', 'release integration test program')} for r in contract['records']]
    tests['libraryExecution'] = {k: library[k] for k in ('status', 'configuration', 'artifact', 'passed', 'failed', 'ignored', 'filtered', 'executionSeconds', 'tests', 'ignoredTests', 'groups')}
    tests['sourceBinding'] = {'embeddedRulesFingerprint': target['rulesFingerprint'], 'recomputedCurrentRulesFingerprint': calculated.hexdigest(),
                             'sourceManifestFiles': 167, 'sourceManifestSha256': source_manifest_hash,
                             'debugArtifactBeforeAfterSha256': debug_hash, 'historicalStandaloneDebugShaCaptured': False,
                             'scope': 'Current artifact SHA was verified before/after execution; no standalone historical SHA is invented. Prior original-path17 execution, embedded fingerprint and current source inventory provide the recorded identity chain.'}
    tests['evidence'] = unique([ref(contract_path), *contract['evidence'], ref(candidate_path), ref(build_path),
                               ref(PROJECT / build['source']['beforeManifest']), ref(PROJECT / build['source']['afterManifest']),
                               ref(PROJECT / source_identity['sourceManifest'])])
    report['review']['nativeLibraryRecheck'] = {'actualUniquePassed': 84, 'prior17AreSubset': True, 'filteredOut': 0,
                                               'executionSeconds': 772.66, 'originalProgramUnchanged': True,
                                               'sourceFingerprintRecomputed': True, 'groundAndAirLibraryCases': 28,
                                               'rawLog': ref(run / 'existing-debug-lib-executed.log')}
    report['review']['limits'] = 'This read-only audit did not launch another executable or repeat the live UI comparison. Debug correctness results are not a release-library execution or performance measurement. Recorded owner1 UI equality remains narrower than hidden-authority equality. Global balance/LAN acceptance is separate.'
    # The frozen validator now accepts this functionality scope without changes.
    validation_store = gate.EvidenceStore(PROJECT)
    gate.validate_functionality(report, target, validation_store)
    image_refs = [r for r in report['nativeUi']['evidence'] if r['path'].endswith('.png')]
    assert len(image_refs) == 8 and all((PROJECT / r['path']).read_bytes()[:8] == b'\x89PNG\r\n\x1a\n' for r in image_refs)
    output = PROJECT / 'game/v6/functionality-acceptance-749391e6-20260911.json'
    assert not output.exists(), 'Preserve prior final-scope evidence'
    output.write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding='utf-8')
    package.check_data_privacy(output)
    assert gate.sha(candidate_path) == candidate_hash
    assert gate.sha(debug_path) == debug_hash
    assert all(gate.sha(PROJECT / name) == value for name, value in frozen.items())
    audit = {'scope': 'Read-only verification and new functionality-scope wrapper; no source edits, test process execution, global approval or ZIP.',
             'report': ref(output), 'functionalGateAccepted': True, 'requiredUniquePassed': 222, 'requiredUncovered': 0,
             'releaseIntegrationPassed': 138, 'existingDebugLibraryPassed': 84, 'releaseLibraryConfigurationStillUnexecuted': 84,
             'priorTargeted17Added': 0, 'ignoredOptInBenchmarks': 2, 'explicitTruePngReferences': 8,
             'debugBeforeAfterAndCurrentSha256': debug_hash, 'current167SourceManifestSha256': source_manifest_hash,
             'originalCandidateUnchanged': ref(candidate_path), 'frozenPackagingSourcesUnchanged': frozen,
             'libraryHelperSourceRefsForAuditOnly': helper_sources}
    audit_path = PROJECT / 'pipeline/v6/functionality-acceptance-review-749391e6-20260911.json'
    assert not audit_path.exists()
    audit_path.write_text(json.dumps(audit, ensure_ascii=False, indent=2), encoding='utf-8')
    print(json.dumps({'report': str(output), 'sha256': gate.sha(output), 'gateAccepted': True, 'passedUnique': 222,
                      'uncovered': 0, 'releaseLibStillNotExecuted': 84, 'review': str(audit_path)}))


if __name__ == '__main__':
    main()
