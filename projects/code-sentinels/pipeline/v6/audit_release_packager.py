"""Bounded synthetic contract checks; never builds a release or runs a match/native EXE."""
from copy import deepcopy
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import sys
import tempfile
import unittest
import zipfile

PROJECT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(PROJECT / 'game/v6'))
import release_gate as g
import portable_release as p

OUT = Path(tempfile.mkdtemp(prefix='packager-audit-', dir=PROJECT / 'pipeline/v6'))
TARGET = {'engineSha256': 'a' * 64, 'rulesVersion': 'synthetic-v6', 'rulesFingerprint': 'b' * 64,
          'payloadSha256': 'c' * 64, 'webManifestSha256': 'd' * 64, 'mediaManifestSha256': 'e' * 64}


def write(relative, value):
    file = OUT / relative
    file.parent.mkdir(parents=True, exist_ok=True)
    file.write_text(json.dumps(value), encoding='utf-8')
    return file


def row(index=0):
    return {'index': index, 'seconds': 2000, 'winner': 1, 'rulesVersion': TARGET['rulesVersion'],
            'simulationSourceSha256': TARGET['rulesFingerprint'], 'branches': ['speed', 'speed'],
            'strategies': ['mixed-ai', 'mixed-ai'], 'plannerOrder': [1, 2],
            **{k: [None, None] for k in ('firstT5Seconds', 'secondT5Seconds', 'firstCombatSeconds', 'firstDamageSeconds')},
            **{k: [1, 2] for k in ('oreDelivered', 'actualAmmoSpent', 'actualComputeSpent', 'actualEnergySpent', 'shotsFired', 'lostValue', 'nodeControlSeconds', 'recoverableCargoWreckAmount')},
            'playabilityWarnings': {k: False for k in ('noCombat', 'noDamage', 'noOreDelivery', 'noMobileArmy', 'unfinished')}}


def matrix():
    rows = []
    for i in range(1000):
        pair = [g.BRANCHES[i // 200], g.BRANCHES[(i // 40) % 5]]; offset = (i // 2) % 20; swap = bool(i % 2)
        rows.append({**row(i), 'originalPair': pair, 'seed': 1000 + offset, 'theme': ('river', 'mining', 'highland')[offset % 3],
                     'swapped': swap, 'branches': list(reversed(pair)) if swap else pair, 'plannerOrder': [2, 1] if swap else [1, 2]})
    return rows


def lan():
    return {**TARGET, 'finalEligible': True, 'short': False, 'requestedSeconds': 2700,
            'observedCombatWallSeconds': 2700, 'clients': [{**TARGET, 'enginePid': n, 'rgbaLocal': True, 'frames': 5000,
            'accepted': 20, 'movedUnits': 2, 'device': 'synthetic-test-device'} for n in (101, 102)],
            'allCombat': [5, 6], 'allOre': [10, 20], 'coverage': list(g.LAN_CHECKS), 'errors': [], 'replayExact': True,
            'consistency': {'tick': 10000, 'authorityOwnerViewSha256': 'f' * 64, 'replicaSha256': 'f' * 64}}


def cold():
    isolated = OUT / '隔离 空格'
    return {**TARGET, 'kind': 'isolated-cold-start', 'actualNativeExecution': True, 'isolatedDirectory': str(isolated),
            'initialRuntimeFiles': {'saves': 0, 'tokens': 0, 'logs': 0}, 'usedBundledRuntime': True,
            'requiresCodex': False, 'externalCodeServicesUsed': [], 'completedFlows': ['solo', 'create', 'join', 'save', 'load', 'replay'],
            'nativeRgbaFrames': 20, 'errors': [], 'finalEligible': True,
            'processes': [{'pid': i, 'executable': str(isolated / 'bin' / name)} for i, name in ((201, 'node.exe'), (202, 'engine-host.exe'))]}


class Gates(unittest.TestCase):
    def reject(self, call, pattern=None):
        with self.assertRaises(g.ReleaseError) as context: call()
        if pattern: self.assertIn(pattern, str(context.exception))

    def test_missing_sections_reject_old_passed_boolean(self):
        file = write('invalid-envelope-old-passed.json', {'schemaVersion': 2, 'version': 6, 'status': 'approved', 'passed': True, 'target': TARGET})
        self.reject(lambda: g.validate_release(file, TARGET, OUT, PROJECT.parents[1], OUT / 'candidate'), 'sections missing')

    def test_synthetic_envelope_rejected(self):
        file = write('invalid-envelope-synthetic.json', {'schemaVersion': 2, 'version': 6, 'status': 'approved', 'target': TARGET, 'syntheticEvidence': True})
        self.reject(lambda: g.validate_release(file, TARGET, OUT, PROJECT.parents[1], OUT / 'candidate'), 'synthetic')

    def test_wrong_target_hash_rejected_before_any_packaging(self):
        value = {'schemaVersion': 2, 'version': 6, 'status': 'approved', 'target': {**TARGET, 'engineSha256': '0' * 64}}
        file = write('invalid-envelope-wrong-engine.json', value)
        self.reject(lambda: g.validate_release(file, TARGET, OUT, PROJECT.parents[1], OUT / 'candidate'), 'target mismatch')

    def test_missing_evidence_file(self):
        self.reject(lambda: g.EvidenceStore(OUT).file({'path': 'missing.json', 'sha256': '0' * 64}, 'test'), 'missing evidence')

    def test_wrong_evidence_hash(self):
        file = write('evidence.json', {'scope': 'synthetic reference hashing test'})
        self.reject(lambda: g.EvidenceStore(OUT).file({'path': str(file), 'sha256': '0' * 64}, 'test'), 'SHA256 mismatch')

    def test_evidence_escape(self):
        file = PROJECT / 'game/v6/strategy-plan.json'
        self.reject(lambda: g.EvidenceStore(OUT).file({'path': str(file), 'sha256': g.sha(file)}, 'test'), 'escapes')

    def test_canonical_matrix_count_and_order(self):
        rows = matrix(); g.matrix_check(rows, TARGET)
        self.assertEqual(len({g.scenario_key(r) for r in rows}), 1000)
        self.assertEqual(rows[1]['plannerOrder'], [2, 1])
        self.assertEqual(rows[999]['seed'], 1019)

    def test_pilot_cannot_substitute_1000(self):
        self.reject(lambda: g.matrix_check(matrix()[:100], TARGET), 'exactly1000')

    def test_matrix_duplicate(self):
        rows = matrix(); rows[-1] = deepcopy(rows[0])
        self.reject(lambda: g.matrix_check(rows, TARGET), 'duplicate')

    def test_matrix_wrong_seed_or_planner(self):
        for key, value in [('seed', 9999), ('plannerOrder', [1, 2])]:
            rows = matrix(); rows[1][key] = value
            self.reject(lambda: g.matrix_check(rows, TARGET), 'canonical case 1')

    def test_matrix_boolean_order_or_index(self):
        for key, value in [('index', True), ('plannerOrder', [True, 2])]:
            item = row(); item[key] = value
            self.reject(lambda: g.validate_match_rows([item], TARGET, 'synthetic'))

    def test_matrix_fingerprint_and_null_t5(self):
        item = row(); before = deepcopy(item); g.validate_match_rows([item], TARGET, 'synthetic')
        self.assertEqual(item, before); self.assertEqual(item['firstT5Seconds'], [None, None])
        item['simulationSourceSha256'] = '0' * 64
        self.reject(lambda: g.validate_match_rows([item], TARGET, 'synthetic'), 'runner rules')

    def test_synthetic_rows_and_playability_rejected(self):
        item = row(); item['syntheticFixture'] = True
        self.reject(lambda: g.validate_match_rows([item], TARGET, 'synthetic'), 'synthetic')
        item = row(); item['playabilityWarnings']['noCombat'] = True
        self.reject(lambda: g.validate_match_rows([item], TARGET, 'synthetic'), 'noncombat')

    def test_strategy_exact_126_and_plan_identity(self):
        file = PROJECT / 'game/v6/strategy-plan.json'; plan = g.read_json(file); rows = [{**row(c['index']), **c, 'planIndex': c['index'], 'planId': plan['planId'], 'planSha256': g.sha(file)} for c in g.canonical_strategy_cases()]
        g.strategy_check(rows, TARGET, plan, g.sha(file)); self.assertEqual(len({g.scenario_key(r) for r in rows}), 126)
        rows[-1]['planSha256'] = '0' * 64
        self.reject(lambda: g.strategy_check(rows, TARGET, plan, g.sha(file)), 'plan identity')

    def test_runner_does_not_impersonate_host(self):
        file = OUT / 'synthetic-runner.dat'; file.write_bytes(b'not an executable, synthetic hashing test')
        value = {'kind': 'native-balance-acceptance', 'actualNativeMatches': True, 'injectedResources': False,
                 'runner': {'artifact': {'path': str(file), 'sha256': g.sha(file)}, 'rulesVersion': TARGET['rulesVersion'], 'rulesFingerprint': TARGET['rulesFingerprint'], 'engineSha256': TARGET['engineSha256']}}
        self.reject(lambda: g.validate_balance(value, TARGET, g.EvidenceStore(OUT)), 'impersonate')

    def test_statistics_recomputed_and_tampering_rejected(self):
        import summarize_balance
        item = {**row(), 'seed': 1000, 'theme': 'river'}; rows = [item]
        analysis = summarize_balance.aggregate(rows, expected_games=1)
        analysis.update(aggregatorSha256=g.sha(summarize_balance.__file__), inputFiles=[{'sha256': 'f' * 64}])
        g.verify_analysis(analysis, rows, ['f' * 64], TARGET, 'synthetic analysis')
        analysis['branches']['speed']['wins'] += 1
        self.reject(lambda: g.verify_analysis(analysis, rows, ['f' * 64], TARGET, 'synthetic analysis'), 'recomputation')

    def test_build_inventory_cannot_omit_critical_sources(self):
        repo = OUT / 'synthetic-source-repository'; repo.mkdir()
        rows = []
        for relative in ('Cargo.toml', 'Cargo.lock', 'projects/code-sentinels/native-v6/Cargo.toml',
                         'projects/code-sentinels/native-v6/src/lib.rs', 'crates/engine-host/Cargo.toml', 'crates/engine-host/src/main.rs'):
            file = repo / relative; file.parent.mkdir(parents=True, exist_ok=True); file.write_text('synthetic text, not compiled')
            rows.append({'path': relative, 'sha256': g.sha(file), 'bytes': file.stat().st_size})
        inventory = write('synthetic-source-inventory.json', rows)
        doc = {'buildExit': 0, 'artifact': {'sha256': TARGET['engineSha256']}, 'rulesVersion': TARGET['rulesVersion'], 'rulesFingerprint': TARGET['rulesFingerprint'],
               'source': {'unchangedDuringBuild': True, 'beforeManifest': str(inventory), 'afterManifest': str(inventory), 'beforeSha256': g.sha(inventory), 'afterSha256': g.sha(inventory), 'fileCount': len(rows)}}
        g.validate_build(doc, TARGET, g.EvidenceStore(OUT), repo)
        inventory.write_text(json.dumps(rows[:-1])); doc['source'].update(beforeSha256=g.sha(inventory), afterSha256=g.sha(inventory), fileCount=len(rows)-1)
        self.reject(lambda: g.validate_build(doc, TARGET, g.EvidenceStore(OUT), repo), 'critical')

    def test_real_historical_performance_scope_and_wrong_hash(self):
        value = g.read_json(PROJECT / 'game/v6/pressure-acceptance-f5dc46fc-20260911.json')
        target = {**TARGET, 'engineSha256': value['native']['sha256'].lower(), 'rulesVersion': value['rulesVersion'], 'rulesFingerprint': value['rulesFingerprint']}
        facts = g.validate_performance(value, target); self.assertAlmostEqual(facts['tickP99Ms'], 14.9759)
        self.reject(lambda: g.validate_performance(value, TARGET), 'hash mismatch')
        value['simulation']['counts']['units'] = 199
        self.reject(lambda: g.validate_performance(value, target), 'full128')

    def test_lan_boundary_and_short_false_positive(self):
        g.validate_lan(lan(), TARGET)
        for patch in ({'observedCombatWallSeconds': 2699.9}, {'finalEligible': False}, {'short': True}, {'passed': True, 'observedCombatWallSeconds': 150}):
            self.reject(lambda: g.validate_lan({**lan(), **patch}, TARGET))

    def test_lan_two_independent_processes_payload_and_view(self):
        value = lan(); value['clients'][1]['enginePid'] = 101
        self.reject(lambda: g.validate_lan(value, TARGET), 'independent')
        value = lan(); value['clients'][1]['payloadSha256'] = '0' * 64
        self.reject(lambda: g.validate_lan(value, TARGET), 'payload mismatch')
        value = lan(); value['consistency']['replicaSha256'] = '0' * 64
        self.reject(lambda: g.validate_lan(value, TARGET), 'state hashes')

    def test_coldstart_complete_synthetic_component(self):
        g.validate_cold_start(cold(), TARGET, OUT / 'project', OUT / 'candidate')

    def test_coldstart_foreign_process_or_state_or_duplicate(self):
        value = cold(); value['processes'][0]['executable'] = str(OUT / 'developer-node.exe')
        self.reject(lambda: g.validate_cold_start(value, TARGET, OUT / 'project', OUT / 'candidate'), 'isolated package')
        value = cold(); value['initialRuntimeFiles']['saves'] = 1
        self.reject(lambda: g.validate_cold_start(value, TARGET, OUT / 'project', OUT / 'candidate'), 'preexisting')
        value = cold(); value['processes'][1]['pid'] = 201
        self.reject(lambda: g.validate_cold_start(value, TARGET, OUT / 'project', OUT / 'candidate'), 'duplicate')

    def test_private_paths_and_secrets(self):
        for name in ('../escape', 'C:/absolute', 'Web/../bin/x', 'QA/Logs/test.json', '.forge/save/v6.json', 'tokens.json', '.env', 'QA/raw.log'):
            self.reject(lambda: p.relative_name(name))
        file = write('synthetic-secret.json', {'nested': {'access_token': 'deliberately-fake-token'}})
        self.reject(lambda: p.CopyPlan().add(file, 'QA/safe-name.json', OUT), 'Sensitive key')

    def test_case_collision_and_source_mutation(self):
        file = write('copy-source.json', {'safe': True}); plan = p.CopyPlan(); plan.add(file, 'Web/test.json', OUT)
        self.reject(lambda: plan.add(file, 'Web/TEST.json', OUT), 'Case-insensitive')
        before = g.sha(file); file.write_text('{}', encoding='utf-8')
        self.reject(lambda: p.CopyPlan().add(file, 'Web/test.json', OUT, expected_sha=before), 'changed before copy')

    def test_late_qa_and_runtime_payload(self):
        plan = p.CopyPlan(); plan.data(b'body', 'Web/index.html'); before = p.canonical_digest(plan.records(p.runtime_path)); plan.data(b'QA', 'QA/final.json')
        self.assertEqual(before, p.canonical_digest(plan.records(p.runtime_path)))
        self.assertIn('QA/final.json', [r['path'] for r in plan.records()])

    def test_refresh_archives_stale_web_and_preserves_saves(self):
        root = OUT / 'refresh'; root.mkdir(); old = root / 'CodeSentinels-V6-Windows'; old.mkdir()
        (old / p.MARKER).write_text(json.dumps({'version': 6, 'candidate': True})); (old / 'Web').mkdir(); (old / 'Web/stale.js').write_text('old')
        for relative in ('.forge/save/v6/keep.json', 'Logs/keep.log', 'personal-note.txt'):
            file = old / relative; file.parent.mkdir(parents=True, exist_ok=True); file.write_text('retained')
        stage = root / '.stage'; stage.mkdir(); (stage / 'Web').mkdir(); (stage / 'Web/new.js').write_text('new'); (stage / p.MARKER).write_text(json.dumps({'version': 6, 'candidate': True}))
        backup = Path(p.install_stage(stage, old, True))
        self.assertTrue((backup / 'Web/stale.js').is_file()); self.assertFalse((old / 'Web/stale.js').exists())
        self.assertTrue((old / 'Web/new.js').is_file())
        for relative in ('.forge/save/v6/keep.json', 'Logs/keep.log', 'personal-note.txt'): self.assertEqual((old / relative).read_text(), 'retained')

    def test_refuse_unknown_and_final_output_refresh(self):
        for label, candidate in [('final', False), ('unknown', None)]:
            root = OUT / label; root.mkdir(); stage = root / 'stage'; stage.mkdir(); out = root / 'CodeSentinels-V6-Windows'; out.mkdir()
            if candidate is not None: (out / p.MARKER).write_text(json.dumps({'version': 6, 'candidate': candidate}))
            self.reject(lambda: p.install_stage(stage, out, True))

    def toy_marker(self):
        data = b'synthetic toy payload'; record = {'path': 'Web/index.html', 'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()}
        return data, {'candidate': False, 'version': 6, 'files': [record], 'included': ['Web/index.html', p.MARKER], 'manifestSha256': p.canonical_digest([record])}

    def test_zip_exact_allowlist_and_hash_rejection(self):
        data, marker = self.toy_marker()
        for label in ('extra', 'missing', 'bad-hash', 'stale-marker'):
            file = OUT / (label + '.synthetic-invalid.zip')
            with zipfile.ZipFile(file, 'x') as z:
                z.writestr('toy/' + p.MARKER, json.dumps({**marker, 'candidate': True} if label == 'stale-marker' else marker))
                if label != 'missing': z.writestr('toy/Web/index.html', b'wrong' if label == 'bad-hash' else data)
                if label == 'extra': z.writestr('toy/secret.txt', b'extra')
            self.reject(lambda: p.verify_archive(file, 'toy', marker))

    def test_managed_extra_files_rejected_but_root_personal_allowed(self):
        data, marker = self.toy_marker(); marker['candidate'] = True
        root = OUT / 'managed-check'; (root / 'Web').mkdir(parents=True); (root / 'Web/index.html').write_bytes(data); (root / p.MARKER).write_text(json.dumps(marker))
        (root / 'personal.txt').write_text('retained'); p.verify_tree(root, marker)
        (root / 'Web/stale.js').write_text('stale')
        self.reject(lambda: p.verify_tree(root, marker), 'stale/unlisted')

    def test_zip_corrupt_crc_rejected(self):
        data, marker = self.toy_marker(); file = OUT / 'bad-crc.synthetic-invalid.zip'
        with zipfile.ZipFile(file, 'x', zipfile.ZIP_STORED) as z:
            z.writestr('toy/' + p.MARKER, json.dumps(marker)); z.writestr('toy/Web/index.html', data)
        raw = file.read_bytes(); at = raw.index(data); raw = raw[:at] + bytes([raw[at] ^ 1]) + raw[at + 1:]; file.write_bytes(raw)
        self.reject(lambda: p.verify_archive(file, 'toy', marker), 'CRC')


if __name__ == '__main__':
    with (OUT / 'synthetic-contracts.txt').open('w', encoding='utf-8') as stream:
        result = unittest.TextTestRunner(stream=stream, verbosity=2).run(unittest.defaultTestLoader.loadTestsFromTestCase(Gates))
    summary = {'scope': 'Explicit synthetic validation-only fixtures plus read-only historical pressure report parsing. No native launch, real match, final acceptance envelope or real game ZIP was created.',
               'syntheticEvidence': True, 'testsRun': result.testsRun, 'failures': len(result.failures), 'errors': len(result.errors), 'passed': result.wasSuccessful(),
               'directory': str(OUT), 'sourceHashes': {name: g.sha(PROJECT / 'game/v6' / name) for name in ('release_gate.py', 'portable_release.py')}}
    (OUT / 'audit-receipt.json').write_text(json.dumps(summary, indent=2), encoding='utf-8')
    print(json.dumps(summary)); print((OUT / 'synthetic-contracts.txt').read_text(encoding='utf-8'))
    raise SystemExit(not result.wasSuccessful())
