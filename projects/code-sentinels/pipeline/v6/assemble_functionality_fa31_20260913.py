"""Audit actual FA31 receipts and assemble functional-scope evidence only.

This script never starts native code, changes the candidate, creates global
approval, or makes a ZIP. Existing audit/aggregate outputs are preserved.
"""
import hashlib
import json
from pathlib import Path
import re
import struct
import subprocess
import sys
from datetime import datetime, timezone

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'pipeline/python-libs'))
sys.path.insert(0, str(ROOT / 'game/v6'))
from PIL import Image
from release_gate import (EvidenceStore, FUNCTION_TAGS, UI_ACTIONS, identity,
                          read_json, require, sha, validate_cold_start,
                          validate_functionality)
from portable_release import check_data_privacy

CANDIDATE = ROOT / 'dist/final-candidate-0acffa83-20260913/CodeSentinels-V6-Windows'
UI_PATH = ROOT / 'qa/v6/client/fa31-20260913/native-ui-acceptance.json'
TEST_PATH = ROOT / 'game/v6/final-contracts-0acffa83-20260912-a/contract-execution-resolved-forward-20260913.json'
COLD_DIR = ROOT / 'game/v6/cold-start-runs/final-fa31b360-20260913'
COLD_PATH = COLD_DIR / 'cold-start-acceptance.json'
OUT = ROOT / 'game/v6/functionality-final-fa31b360-20260913.json'
AUDIT_PATH = ROOT / 'game/v6/functionality-evidence-audit-fa31b360-20260913.json'
SHUTDOWN_PATH = COLD_DIR / 'independent-shutdown-process-verification.json'


def ref(path):
    path = Path(path).resolve()
    require(path.is_relative_to(ROOT) and path.is_file(), 'Evidence must exist within project')
    return {'path': path.relative_to(ROOT).as_posix(), 'sha256': sha(path)}


def exclusive_json(path, document):
    with path.open('x', encoding='utf-8') as stream:
        json.dump(document, stream, ensure_ascii=False, indent=2)


def checked_ref(value):
    file = store.file(value, 'audit-' + str(len(store.references)))
    check_data_privacy(file)
    return ref(file)


for path in (OUT, AUDIT_PATH, SHUTDOWN_PATH):
    require(not path.exists(), 'Prior evidence preserved: ' + str(path))
target = read_json(CANDIDATE / 'v6-candidate.json')['target']
require(target['engineSha256'] == 'fa31b3608a0f418f6bed58d0cac70c2c09885ba2d9912733c1c8aff07c0f1a1e', 'Unexpected engine')
require(target['rulesFingerprint'] == '0acffa83ef75bfeb39efeaf9a49b706c0b02446d4399dab14048b1e8398ab08a', 'Unexpected rules')
require(target['payloadSha256'] == 'e8fdd353bc49838ccc8b199978a9f2d3bda592463a42b3c6e747f495e53a646a', 'Unexpected payload')
for path, expected in ((UI_PATH, '0b8614ae2084de71f5c9fe52ee99ff4587ece43e05a90bc7eac08da5767ccfde'),
                       (TEST_PATH, 'da45e9591edfebc1dd8cbdd38f226eefdf677ee18869f0e484720b6eacdee38b'),
                       (COLD_PATH, 'd8d6471fa00f774f2898cf308fd00991cefe791041bdc0835173c27e68fe160a')):
    require(sha(path) == expected, 'Reviewed report changed: ' + path.name)
ui, tests, cold = map(read_json, (UI_PATH, TEST_PATH, COLD_PATH))
store = EvidenceStore(ROOT)
identity(ui, target, 'UI', payload=True)
validate_cold_start(cold, target, ROOT, CANDIDATE)
require(tests['rulesFingerprint'] == target['rulesFingerprint'], 'Tests have different rules')
require(tests['actualNativeExecution'] and tests['completeRequiredTestCoverage'], 'Required native tests incomplete')
require(tests['unexecutedTargets'] == tests['missingTargets'] == [], 'Unexecuted native target remains')
test_refs = [ref(TEST_PATH)] + [checked_ref(r) for r in tests['evidence']]
ui_refs = [ref(UI_PATH)] + [checked_ref(r) for r in ui['evidence']]

full_log = (TEST_PATH.parent / 'cargo-tests.log').read_text(encoding='utf-8-sig').splitlines()
rerun_log = (ROOT / 'game/v6/forward-charge-real-engagement-20260913.log').read_text(encoding='utf-8-sig').splitlines()
for item in tests['targets']:
    lines = rerun_log if item['target'] == 'tests/v6_forward_energy.rs' else full_log
    line = lines[item['resultLogLine'] - 1]
    result = re.search(r'(\d+) passed; (\d+) failed; (\d+) ignored;', line)
    require(result and tuple(map(int, result.groups())) == (item['passed'], item['failed'], item['ignored']), 'Native result line mismatch')
require(sum(t['passed'] for t in tests['targets']) == tests['passedTests'] == 254, 'Native passing count mismatch')
require(sum(t['failed'] for t in tests['targets']) == tests['failedTests'] == 0, 'Unresolved native failures')
require(sum(t['ignored'] for t in tests['targets']) == tests['ignoredTests'] == 2, 'Optional benchmark count mismatch')
coverage = {
    'construction-and-layers': ['paid_t2_construction_connects_basement_and_upper_floor'],
    'weapons-and-four-defenses': ['own_wall_stops_a_direct_round_without_friendly_fire', 'guidance_jamming_spends_finite_defense_charge_and_freezes_the_aim', 'split_merge_cannot_duplicate_fixed_defense_capacity_or_recharge', 'a_fully_absorbed_scan_does_not_apply_a_hit_mark', 'claude_interception_plugin_strengthens_its_real_barrier'],
    'power-and-compute-isolation': ['destruction_of_a_vertical_shaft_breaks_both_power_and_compute', 'radio_does_not_leak_through_floors_or_across_player_ownership'],
    'ground-and-air-logistics': ['logistics::logistics_contracts::air_cargo_is_delivered_after_real_takeoff_cruise_and_landing_and_fuel_is_conserved', 'logistics::logistics_contracts::manual_ground_route_preserves_partial_edge_and_is_idempotent', 'logistics::logistics_contracts::unloading_duration_scales_with_cargo_and_real_boost_without_creating_ammo'],
    'research-and-plugins': ['research_branches_are_parallel_without_unlocking_other_factions', 'plugin_slot_requires_the_units_own_tier_even_with_advanced_research'],
    'ownership-and-idempotency': ['duplicate_command_is_idempotent_and_wrong_owner_cannot_recycle'],
    'split-merge-and-collapse': ['split_merge_preserves_gpu_hp_and_refund_value', 'paid_independent_wings_collapse_only_above_removed_support_and_replay'],
    'save-load-replay-identity': ['save_roundtrip_and_public_command_replay_match', 'save_rule_identity_is_required_by_load_and_both_replay_entries'],
}
require(set(coverage) == FUNCTION_TAGS, 'Missing coverage category')
for names in coverage.values():
    for name in names:
        require('test ' + name + ' ... ok' in full_log, 'Coverage example did not execute: ' + name)

png_audit = []
for screen in ui['screenshots']:
    original = store.file(screen['original'], 'original-' + str(len(png_audit)), publish=False)
    png = store.file(screen['png'], 'png-' + str(len(png_audit)))
    with Image.open(original) as a, Image.open(png) as b:
        rgba = a.convert('RGBA').tobytes()
        require(a.format == 'JPEG' and b.format == 'PNG' and b.size == tuple(screen['size']), 'Wrong screenshot format/dimensions')
        require(rgba == b.convert('RGBA').tobytes(), 'PNG differs from decoded original screenshot')
        require(hashlib.sha256(rgba).hexdigest() == screen['decodedRgbaSha256'], 'Screenshot decoded digest differs')
    require(screen['png'] in ui['evidence'], 'PNG absent from direct UI evidence')
    png_audit.append({'file': ref(png), 'width': screen['size'][0], 'height': screen['size'][1], 'decodedPixelsEqualOriginal': True})
require(len(png_audit) == 7, 'Expected seven actual CUA screenshots')

setup_path = UI_PATH.parent / 'paid-layer-setup.json'
setup = read_json(setup_path)
checkpoint = next(row for row in setup if row.get('event') == 'paid opening verified')
opening_orders = [row for row in setup if row.get('event') == 'order' and row['receipt']['sequence'] <= 13]
require(len(opening_orders) == 13 and all(row['receipt']['accepted'] for row in opening_orders), 'Paid opening orders incomplete')
require(checkpoint['player']['credits'] == ui['ordinaryPaidOpening']['creditsRemaining'] and checkpoint['tick'] == 8188, 'Paid opening checkpoint differs')
commands = [row['command'] for row in opening_orders]
built = {
    'extractor': sum(c.get('op') == 'build' and c.get('kind') == 'extractor' for c in commands),
    'power': sum(c.get('op') == 'build' and c.get('kind') == 'wind-power' for c in commands),
    'dataCenter': sum(c.get('op') == 'room' and c.get('kind') == 'data-center' for c in commands),
    'gpu': sum(c.get('op') == 'install-gpu' for c in commands),
    'lab': sum(c.get('op') == 'room' and c.get('kind') == 'research-lab' for c in commands),
    'starterTurrets': sum(c.get('op') == 'deploy' and c.get('kind') in ('vscode', 'pycharm') for c in commands),
    'powerLines': sum(c.get('op') == 'wire' and c.get('kind') == 'power' for c in commands),
    'computeLines': sum(c.get('op') == 'wire' and c.get('kind') == 'compute' for c in commands),
}
saved_path = CANDIDATE / '.forge/save/v6/8a25a2eb-2332-466b-b605-0c1ee42bdc58.json'
require(sha(saved_path) == ui['replayComparison']['savedFileSha256'], 'Original saved UI state changed')
saved = read_json(saved_path)['save']
owned = [row for row in saved['orders'] if row['order']['owner'] == 1]
require(len(owned) == 24 and owned[15:] == ui['browserOrdersFromSavedGame'], 'Nine browser orders differ from actual own replay orders')
before = read_json(UI_PATH.parent / 'replay-before-skill.json')['snapshot']
ended = read_json(UI_PATH.parent / 'replay-ended-state.json')['snapshot']
require(before['tick'] == 34431 and ended['tick'] == saved['snapshot']['tick'] == 35031, 'Replay endpoints differ')
comparisons = []
for name in ('players', 'buildings', 'rooms', 'units', 'links'):
    a = [row for row in saved['snapshot'][name] if row.get('owner') == 1]
    b = [row for row in ended[name] if row.get('owner') == 1]
    require(a == b, 'Own saved/replay collection differs: ' + name)
    comparisons.append({'collection': name, 'owner': 1, 'count': len(a), 'equal': True})
unit = next(row for row in ended['units'] if row['id'] == 693)
require(unit['lastCastTick'] == 35019 and unit['attackCount'] == 53 and unit['covered'] is True and unit['wired'] is False, 'GLM state differs')
stream = read_json(UI_PATH.parent / 'native-stream.json')
require(stream['rgbaFrames'] == ui['rgbaFrames'] == 30 and stream['errors'] == ui['errors'] == [], 'Native UI observation differs')
require(UI_ACTIONS <= set(ui['completedActions']), 'Missing actual CUA action')

cold_refs = [ref(COLD_PATH)]
cold_pixels = []
for frame in cold['nativeFrameEvidence']:
    png = store.file(frame, 'cold-frame-' + str(len(cold_pixels)))
    raw = png.with_suffix('.rgba').read_bytes()
    require(len(raw) == 20 + 1280 * 720 * 4 and raw[:4] == b'FGF1' and struct.unpack_from('<HH', raw, 8) == (1280, 720), 'Native packet format differs')
    require(struct.unpack_from('<I', raw, 12)[0] & 2 == 0, 'Native packet was truncated')
    with Image.open(png) as image:
        require(image.format == 'PNG' and image.size == (1280, 720) and image.convert('RGBA').tobytes() == raw[20:], 'Cold PNG differs from actual raw native packet')
    cold_pixels.append({'file': ref(png), 'rawFile': png.with_suffix('.rgba').relative_to(ROOT).as_posix(), 'rawSha256': hashlib.sha256(raw).hexdigest(), 'width': 1280, 'height': 720, 'decodedPixelsEqualRawPacket': True})
    cold_refs.append(ref(png))
require(len(cold_pixels) == 2 and sum(c['frames'] for c in cold['clients']) == cold['nativeRgbaFrames'] == 311, 'Cold native frame evidence/count differs')
require(sha(ROOT / 'game/v6/cold-start.mjs') == cold['driverSha256'], 'Cold driver changed')
for name in ('actual-process-paths.json', 'ending-owner-views.json', 'events.jsonl', 'blue-process.log', 'red-process.log'):
    cold_refs.append(checked_ref(ref(COLD_DIR / name)))
for process in cold['processes']:
    executable = Path(process['executable'])
    require(sha(executable) == sha(CANDIDATE / 'bin' / executable.name), 'Isolated executable differs from bundled candidate')
ids = [int(p['pid']) for p in cold['processes']]
query = ' OR '.join('ProcessId = ' + str(pid) for pid in ids)
command = "[Console]::OutputEncoding=[System.Text.UTF8Encoding]::new(); $ErrorActionPreference='Stop'; $observed=@(Get-CimInstance Win32_Process -Filter '" + query + "'); ConvertTo-Json -InputObject @($observed | Select-Object ProcessId,Name,ExecutablePath) -Compress"
result = subprocess.run(['powershell', '-NoProfile', '-Command', command], check=True, capture_output=True, encoding='utf-8')
remaining = json.loads(result.stdout.strip())
require(remaining == [], 'A recorded owned cold-start process still exists')
shutdown = {'recordedAtUtc': datetime.now(timezone.utc).isoformat(), 'scope': 'Independent read-only Win32_Process query for the four exact recorded cold-start process IDs after the driver recorded normal shutdown.', 'ownedPids': ids, 'remainingOwnedProcesses': remaining, 'remainingOwnedCount': len(remaining), 'normalShutdownSource': ref(COLD_PATH)}
exclusive_json(SHUTDOWN_PATH, shutdown)
cold_refs.append(ref(SHUTDOWN_PATH))

audit = {'schemaVersion': 1, 'kind': 'functional-evidence-audit', 'recordedAtUtc': datetime.now(timezone.utc).isoformat(), 'target': target, 'scope': 'Read-only independent reference hashes, actual native result-log lines, preserved saved-order/own-state comparisons, seven decoded CUA image pairs, two raw native PNG pairs and owned-process shutdown query. No native game execution is started by this audit; no global release approval.', 'passed': True, 'issues': [], 'nativeTestTargets': len(tests['targets']), 'resolvedPassedTests': 254, 'optionalIgnoredBenchmarks': 2, 'nativeResultLogLinesVerified': True, 'uiEvidenceReferencesVerified': len(ui['evidence']), 'testEvidenceReferencesVerified': len(tests['evidence']), 'paidOpeningBuilt': built, 'savedGamePath': saved_path.relative_to(ROOT).as_posix(), 'savedGameSha256': sha(saved_path), 'actualOwnerOrders': len(owned), 'actualBrowserOrdersAfterSetup': len(owned[15:]), 'savedReplayOwnCollections': comparisons, 'cuaPngs': png_audit, 'coldNativePngs': cold_pixels, 'coldStartGatePassed': True, 'shutdownVerification': ref(SHUTDOWN_PATH), 'visuallyReviewedImages': 9, 'visualScope': 'Actual lobby at634x701 and1280x720, equipment archive, guide, upper room343 with installedGPU/network, supplied GLM693 and replay endpoint; cold PNGs show each local starting viewport. Visual inspection does not replace action receipts or assert measured FPS.', 'telemetryLimits': 'UI and cold stream meshFallbacks are null/unobserved, never zero. Saved cold raw packets are the first accepted frames; clients.frameSha256 records the latest accepted sample and is not relabelled as their stored raw packet hash.', 'finalReleaseAccepted': False}
exclusive_json(AUDIT_PATH, audit)

document = {'schemaVersion': 2, 'kind': 'native-functional-acceptance', **target, 'recordedAtUtc': datetime.now(timezone.utc).isoformat(), 'finalEligible': True, 'status': 'functional-scope-accepted', 'scope': 'Reviewed completed native functional evidence only. Native tests use separately executed test binaries with the same compiled Game rules; the target host is independently catalogued. Original failed invocation and its one actual resolving rerun are both retained. This is not global release, full balance, pressure or45-minute LAN approval.',
    'nativeTests': {'actualNativeExecution': True, 'passedTests': 254, 'failedTests': 0, 'unexecutedTests': 0, 'ignoredComponentBenchmarks': 2, 'coverage': sorted(coverage), 'coverageExamplesFromActualPassingLog': coverage, 'coverageMeaning': 'Current logical coverage across253 retained passing cases plus one affected-test rerun. The original full invocation had253 passed and1 failed; it is not rewritten as one all-green invocation. Two explicit opt-in component benchmarks remain ignored and are outside required functional cases.', 'resolvedExecution': tests['resolution'], 'evidence': test_refs},
    'ordinaryOpening': {'actualNativeExecution': True, 'ordinaryPaidCommands': True, 'injectedResources': False, 'startingCredits': 2000, 'startingCreditsBasis': 'Standard compiled Game::new starting credits and ordinary CUA solo creation; setup receipts begin at tick6163 after real elapsed passive income, rather than an initial-tick snapshot.', 'creditsRemaining': checkpoint['player']['credits'], 'acceptedOrders': len(opening_orders), 'verifiedTick': checkpoint['tick'], 'built': built, 'observedPower': checkpoint['player']['power'], 'observedLoad': checkpoint['player']['demand'], 'observedComputeProduction': checkpoint['player']['production'], 'observedOreDelivered': checkpoint['player']['totals']['ore-delivered'], 'scope': 'First13 accepted paid API setup orders within an ordinary2000-credit solo session. Later research/upper shell and nine accepted CUA orders are separate. The966.7633333337553 reserve includes actual elapsed passive income.', 'evidence': [ref(UI_PATH), ref(setup_path), ref(AUDIT_PATH)]},
    'nativeUi': {'actualNativeExecution': True, 'rgbaFrames': stream['rgbaFrames'], 'errors': [], 'completedActions': ui['completedActions'], 'acceptedBrowserOrders': len(owned[15:]), 'apiSetupOrdersBeforeBrowser': 15, 'skill': ui['skill'], 'saveLoadReplay': {'savedTick': 35031, 'rewindTick': 34431, 'savedGameSha256': sha(saved_path), 'ownerStateComparisons': comparisons, 'wholeAuthorityStateCompared': False}, 'historicalUiAttempt': ui['historicalUiAttempt'], 'coldStartFrameCountKeptSeparate': cold['nativeRgbaFrames'], 'scope': 'Ten actual CUA action categories and30 independently observed local RGBA frames. Cold311 frames and two cold PNGs support standalone native display, not additional CUA actions. GLM fixed-line covered=true/wired=false; this is not a locked tether or isolated offline skill-cost measurement. UI process cleanup was scoped termination after session leave, not graceful shutdown proof. Cold normal shutdown is separately verified. Mesh fallback telemetry remains unobserved in both streams.', 'evidence': ui_refs + cold_refs + [ref(AUDIT_PATH)]},
    'coldStartScope': {'actualNativeExecution': True, 'finalEligible': True, 'nativeRgbaFrames': cold['nativeRgbaFrames'], 'completedFlows': cold['completedFlows'], 'normalShutdown': cold['normalShutdown'], 'evidence': ref(COLD_PATH)},
    'review': {'audit': ref(AUDIT_PATH), 'explicitPngReferences': [r['file'] for r in png_audit + cold_pixels], 'allNinePngsDirectlyRegisteredInNativeUi': True, 'finalReleaseAccepted': False}}
gate_store = EvidenceStore(ROOT)
validate_functionality(document, target, gate_store)
for record in gate_store.references.values():
    check_data_privacy(record['path'])
require(len([r for r in gate_store.references.values() if r['path'].suffix.lower() == '.png']) == 9, 'Nine real PNG references must be directly registered')
exclusive_json(OUT, document)
check_data_privacy(OUT)
print(json.dumps({'functionalGatePassed': True, 'coldStartGatePassed': True, 'functionalReport': ref(OUT), 'auditReport': ref(AUDIT_PATH), 'shutdownReport': ref(SHUTDOWN_PATH), 'registeredEvidenceReferences': len(gate_store.references), 'directPngReferences': 9, 'payloadUnchanged': target['payloadSha256'], 'globalReleaseApproved': False, 'zipCreated': False}, ensure_ascii=False))
