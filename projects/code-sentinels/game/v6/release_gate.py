"""Fail-closed V6 release evidence checks. No simulation, packaging or approval is fabricated."""
from collections import Counter
from dataclasses import dataclass
import hashlib
import json
import math
from pathlib import Path
import statistics

BRANCHES = ('speed', 'security', 'algorithm', 'science', 'lightweight')
STYLES = ('expansion', 'maintech', 'multitech', 'mech', 'mixed-ai', 'turtle')
LEVELS = {str(i) for i in range(-2, 6)}
FUNCTION_TAGS = {'construction-and-layers', 'weapons-and-four-defenses', 'power-and-compute-isolation',
                 'ground-and-air-logistics', 'research-and-plugins', 'ownership-and-idempotency',
                 'split-merge-and-collapse', 'save-load-replay-identity'}
UI_ACTIONS = {'new-solo', 'build-room', 'power-wire', 'compute-wire', 'install-gpu',
              'deploy-unit', 'skill-target', 'change-layer', 'save-load', 'replay'}
LAN_CHECKS = {'ownership', 'deduplication', 'delayed-transport', 'disconnect-recovery',
              'timeout-forfeit', 'independent-camera-layers', 'save-load', 'exact-replay', 'consistent-outcome'}
BALANCE_CRITERIA = {'branch-matchups', 'strategy-comparisons', 'match-duration',
                    'technology-timing', 'mixed-ai-and-hard-kill', 'resource-control-and-logistics'}


class ReleaseError(RuntimeError):
    pass


def require(value, message):
    if not value:
        raise ReleaseError(message)


def sha(file):
    with Path(file).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def digest(value, label):
    require(isinstance(value, str) and len(value) == 64 and all(c in '0123456789abcdef' for c in value.lower()), f'{label}: SHA256 required')
    return value.lower()


def number(value, minimum=0):
    return type(value) in (int, float) and math.isfinite(value) and value >= minimum


def integer(value, minimum=0):
    return type(value) is int and value >= minimum


def read_json(path):
    return json.loads(Path(path).read_text(encoding='utf-8-sig'))


def identity(document, target, label, payload=False):
    require(isinstance(document, dict), f'{label}: object required')
    actual = document.get('engineSha256', document.get('native', {}).get('sha256'))
    require(digest(actual, label + '/engine') == target['engineSha256'], f'{label}: engine hash mismatch')
    require(document.get('rulesVersion') == target['rulesVersion'], f'{label}: rulesVersion mismatch')
    require(digest(document.get('rulesFingerprint'), label + '/rules') == target['rulesFingerprint'], f'{label}: rules fingerprint mismatch')
    if payload:
        require(digest(document.get('payloadSha256'), label + '/payload') == target['payloadSha256'], f'{label}: tested package payload mismatch')
    require(document.get('syntheticEvidence') is not True, f'{label}: fabricated observation fixtures cannot approve release')


class EvidenceStore:
    """References are local files beneath an explicit evidence root, bound by bytes."""
    def __init__(self, root):
        self.root = Path(root).resolve()
        self.references = {}

    def file(self, reference, label, publish=True):
        require(isinstance(reference, dict), f'{label}: evidence reference required')
        name = reference.get('path')
        require(isinstance(name, str) and name.strip() and '\x00' not in name, f'{label}: evidence path required')
        path = Path(name)
        path = (path if path.is_absolute() else self.root / path).resolve()
        require(path.is_relative_to(self.root) and path.is_file(), f'{label}: missing evidence or path escapes evidence root')
        expected = digest(reference.get('sha256'), label)
        require(sha(path) == expected, f'{label}: evidence SHA256 mismatch')
        self.references[label] = {'path': path, 'sha256': expected, 'publish': publish}
        return path

    def json(self, reference, label, publish=True):
        path = self.file(reference, label, publish)
        require(path.suffix.lower() == '.json', f'{label}: JSON evidence required')
        value = read_json(path)
        require(isinstance(value, dict), f'{label}: JSON object required')
        require(value.get('syntheticEvidence') is not True, f'{label}: synthetic observations are not release evidence')
        return value

    def supporting(self, refs, label):
        require(isinstance(refs, list) and refs, f'{label}: underlying evidence references required')
        for i, ref in enumerate(refs):
            # Execution stdout can substantiate a summary without shipping logs.
            self.file(ref, f'{label}-{i}', publish=Path(ref.get('path', '')).suffix.lower() != '.log')


def validate_host(document, target, store):
    identity(document, target, 'host identity')
    require(document.get('kind') == 'native-host-identity' and document.get('actualNativeExecution') is True,
            'host identity: actual native catalogue query required')
    require(integer(document.get('nativePid'), 1), 'host identity: real native process ID required')
    require('game.session.catalog' in document.get('observedMethods', []), 'host identity: catalogue query not observed')
    catalog = store.json(document.get('catalog'), 'host-catalog')
    require(catalog.get('version') == 6 and catalog.get('rulesVersion') == target['rulesVersion'], 'host catalogue: version mismatch')
    require(digest(catalog.get('rulesFingerprint'), 'host catalogue') == target['rulesFingerprint'], 'host catalogue: rules fingerprint mismatch')


def validate_build(document, target, store, repository):
    require(document.get('buildExit') == 0 and document.get('source', {}).get('unchangedDuringBuild') is True,
            'native build: completed stable-source build required')
    require(digest(document.get('artifact', {}).get('sha256'), 'build artifact') == target['engineSha256'], 'native build: engine mismatch')
    require(document.get('rulesVersion') == target['rulesVersion'] and digest(document.get('rulesFingerprint'), 'build rules') == target['rulesFingerprint'], 'native build: rule identity mismatch')
    source = document['source']
    before = store.file({'path': source.get('beforeManifest'), 'sha256': source.get('beforeSha256')}, 'native-source-before')
    after = store.file({'path': source.get('afterManifest'), 'sha256': source.get('afterSha256')}, 'native-source-after')
    require(sha(before) == sha(after), 'native build: before/after source manifests differ')
    rows = read_json(after)
    require(isinstance(rows, list) and rows and len(rows) == source.get('fileCount'), 'native build: source inventory incomplete')
    repository = Path(repository).resolve()
    names = set()
    for row in rows:
        path = (repository / row.get('path', '')).resolve()
        require(path.is_relative_to(repository) and path.is_file(), 'native build: source path missing/outside repository')
        name = str(path).casefold()
        require(name not in names, 'native build: duplicate source inventory path')
        names.add(name)
        require(sha(path) == digest(row.get('sha256'), 'native source'), 'native build: current source differs from compiled inventory')
    critical = {repository / 'Cargo.toml', repository / 'Cargo.lock'}
    for relative in ('projects/code-sentinels/native-v6', 'crates/engine-host'):
        base = repository / relative
        critical.add(base / 'Cargo.toml')
        if (base / 'build.rs').is_file():
            critical.add(base / 'build.rs')
        critical.update((base / 'src').rglob('*.rs'))
    require(all(str(p.resolve()).casefold() in names for p in critical), 'native build: critical native/host source inventory is incomplete')


def validate_functionality(document, target, store):
    identity(document, target, 'functionality')
    require(document.get('kind') == 'native-functional-acceptance' and document.get('finalEligible') is True,
            'functionality: final actual functional evidence required')
    tests = document.get('nativeTests', {})
    require(tests.get('actualNativeExecution') is True and integer(tests.get('passedTests'), 1)
            and tests.get('failedTests') == 0 and tests.get('unexecutedTests') == 0,
            'functionality: native tests must have executed with no unresolved failures/blocked targets')
    require(FUNCTION_TAGS <= set(tests.get('coverage', [])), 'functionality: mandatory rule coverage missing')
    store.supporting(tests.get('evidence'), 'functional-native-tests')
    opening = document.get('ordinaryOpening', {})
    require(opening.get('actualNativeExecution') is True and opening.get('ordinaryPaidCommands') is True
            and opening.get('injectedResources') is False and opening.get('startingCredits') == 2000
            and number(opening.get('creditsRemaining'), 500) and integer(opening.get('acceptedOrders'), 10),
            'functionality: real paid2000-credit opening and500-credit reserve required')
    counts = opening.get('built', {})
    require(all(integer(counts.get(k), n) for k, n in {'extractor': 1, 'power': 1, 'dataCenter': 1, 'gpu': 1, 'lab': 1, 'starterTurrets': 2, 'powerLines': 1, 'computeLines': 1}.items()),
            'functionality: paid opening infrastructure incomplete')
    store.supporting(opening.get('evidence'), 'functional-opening')
    ui = document.get('nativeUi', {})
    require(ui.get('actualNativeExecution') is True and integer(ui.get('rgbaFrames'), 1)
            and UI_ACTIONS <= set(ui.get('completedActions', [])) and ui.get('errors') == [],
            'functionality: final native UI/input/targeting flows not all observed')
    store.supporting(ui.get('evidence'), 'functional-ui')


def rows_from_refs(refs, store, label):
    require(isinstance(refs, list) and refs, f'{label}: actual JSONL receipt references required')
    rows = []
    input_hashes = []
    for i, ref in enumerate(refs):
        file = store.file(ref, f'{label}-receipts-{i:02}')
        require(file.suffix.lower() == '.jsonl', f'{label}: JSONL match receipts required')
        parsed = [json.loads(line) for line in file.read_text(encoding='utf-8-sig').splitlines() if line.strip()]
        require(parsed and all(isinstance(row, dict) for row in parsed), f'{label}: empty or malformed receipt file')
        rows.extend(parsed)
        input_hashes.append(sha(file))
    return rows, input_hashes


def validate_match_rows(rows, target, label):
    seen = set()
    for row in rows:
        index = row.get('index')
        require(integer(index) and index not in seen, f'{label}: missing/duplicate/noninteger case index')
        seen.add(index)
        require(row.get('syntheticFixture') is not True and row.get('syntheticEvidence') is not True,
                f'{label}: synthetic matches cannot approve a release')
        require(row.get('rulesVersion') == target['rulesVersion'] and digest(row.get('simulationSourceSha256'), label + '/simulation') == target['rulesFingerprint'], f'{label}: runner rules do not match observed host rules')
        require(number(row.get('seconds')) and row['seconds'] > 0, f'{label}: actual elapsed native game time required')
        require(row.get('winner') is None or type(row.get('winner')) is int and row['winner'] in (1, 2), f'{label}: invalid winner')
        require(isinstance(row.get('branches'), list) and len(row['branches']) == 2 and all(v in BRANCHES for v in row['branches']), f'{label}: invalid branches')
        require(isinstance(row.get('strategies'), list) and len(row['strategies']) == 2 and all(v in STYLES for v in row['strategies']), f'{label}: invalid strategies')
        planner = row.get('plannerOrder')
        require(isinstance(planner, list) and len(planner) == 2 and all(type(v) is int for v in planner) and sorted(planner) == [1, 2], f'{label}: explicit valid plannerOrder required')
        for key in ('firstT5Seconds', 'secondT5Seconds', 'firstCombatSeconds', 'firstDamageSeconds'):
            values = row.get(key)
            require(isinstance(values, list) and len(values) == 2 and all(v is None or number(v) and v <= row['seconds'] for v in values), f'{label}: {key} must preserve unavailable values as null')
        for key in ('oreDelivered', 'actualAmmoSpent', 'actualComputeSpent', 'actualEnergySpent', 'shotsFired', 'lostValue', 'nodeControlSeconds', 'recoverableCargoWreckAmount'):
            values = row.get(key)
            require(isinstance(values, list) and len(values) == 2 and all(number(v) for v in values), f'{label}: real per-owner metric {key} missing')
        warnings = row.get('playabilityWarnings')
        require(isinstance(warnings, dict) and all(type(warnings.get(k)) is bool for k in ('noCombat', 'noDamage', 'noOreDelivery', 'noMobileArmy', 'unfinished')), f'{label}: playability warning observations required')
        require(not any(warnings[k] for k in ('noCombat', 'noDamage', 'noOreDelivery')), f'{label}: observed noncombat/nonproductive policy remains unresolved')
    return seen


def scenario_key(row):
    return json.dumps([row['seed'], row['theme'], row['branches'], row['strategies'], row['plannerOrder']], separators=(',', ':'))


def matrix_check(rows, target):
    require(len(rows) == 1000 and validate_match_rows(rows, target, 'branch matrix') == set(range(1000)), 'branch matrix: exactly1000 canonical cases required')
    for row in rows:
        i = row['index']; pair = [BRANCHES[i // 200], BRANCHES[(i // 40) % 5]]; seed = (i // 2) % 20; swapped = bool(i % 2)
        require(row.get('originalPair') == pair and row.get('swapped') is swapped
                and row['branches'] == (list(reversed(pair)) if swapped else pair)
                and row.get('seed') == 1000 + seed and row.get('theme') == ('river', 'mining', 'highland')[seed % 3]
                and row['strategies'] == ['mixed-ai', 'mixed-ai'] and row['plannerOrder'] == ([2, 1] if swapped else [1, 2]),
                f'branch matrix: canonical case {i} mismatch')
    require(len({scenario_key(row) for row in rows}) == 1000, 'branch matrix: repeated configurations are not1000 independent planned scenarios')


def canonical_strategy_cases():
    cases = []; pair_index = 0
    for i, a in enumerate(STYLES):
        for b in STYLES[i:]:
            for theme_index, theme in enumerate(('river', 'mining', 'highland')):
                branch = BRANCHES[(pair_index + theme_index) % 5]
                for swapped in (False, True):
                    cases.append({'index': len(cases), 'seed': 1000 + theme_index, 'theme': theme,
                                  'branches': [branch, branch], 'strategies': [b, a] if swapped else [a, b],
                                  'swapped': swapped, 'plannerOrder': [2, 1] if a == b and swapped else [1, 2]})
            pair_index += 1
    return cases


def strategy_check(rows, target, plan, plan_hash):
    require(plan.get('schemaVersion') == 1 and plan.get('planId') == 'v6-strategy-126-v1', 'strategy plan: unsupported experiment version')
    expected = canonical_strategy_cases(); cases = plan.get('cases')
    require(isinstance(cases, list) and len(cases) == 126, 'strategy plan:126 approved cases required')
    for actual, wanted in zip(cases, expected):
        require(all(type(actual.get(k)) is type(v) and actual[k] == v for k, v in wanted.items()), f'strategy plan: approved case{wanted["index"]} changed')
    require(len(rows) == 126 and validate_match_rows(rows, target, 'strategies') == set(range(126)), 'strategies: all126 real planned receipts required')
    for row in rows:
        wanted = expected[row['index']]
        require(all(row.get(k) == v and type(row.get(k)) is type(v) for k, v in wanted.items()), 'strategy receipt does not match its planned scenario')
        require(row.get('planId') == plan['planId'] and row.get('planIndex') == row['index'] and digest(row.get('planSha256'), 'strategy plan reference') == plan_hash, 'strategy receipt: exact plan identity missing/mismatched')
    require(len({scenario_key(row) for row in rows}) == 126, 'strategy receipt: repeated configurations in126-case comparison')


def verify_analysis(analysis, rows, hashes, target, label):
    require(analysis.get('integrityPassed') is True and analysis.get('integrityIssues') == [] and analysis.get('games') == len(rows), f'{label}: analysis integrity/count mismatch')
    require(analysis.get('distinctScenarioCount') == len({scenario_key(r) for r in rows}) and analysis.get('plannerOrderUnspecifiedRuns') == 0, f'{label}: distinct scenarios/explicit planner order missing')
    require(Counter(digest(f.get('sha256'), label + '/input') for f in analysis.get('inputFiles', [])) == Counter(hashes), f'{label}: analysis does not reference the supplied raw inputs')
    require(analysis.get('rulesVersions') == {target['rulesVersion']: len(rows)} and {digest(k, label + '/fingerprint'): v for k, v in analysis.get('simulationFingerprints', {}).items()} == {target['rulesFingerprint']: len(rows)}, f'{label}: analysis rule identity mismatch')
    require(analysis.get('durationSeconds', {}).get('samples') == len(rows) and analysis['durationSeconds'].get('median') == statistics.median(r['seconds'] for r in rows), f'{label}: recorded duration summary mismatch')
    require(analysis.get('finalBalanceAcceptance') is False, f'{label}: raw analysis must not manufacture final balance approval')
    # Recompute the reviewed statistics, including faction exchange/timing, instead
    # of trusting an integrity boolean attached to unrelated or altered totals.
    import summarize_balance
    require(digest(analysis.get('aggregatorSha256'), label + '/aggregator') == sha(summarize_balance.__file__), f'{label}: analysis script version mismatch')
    require([digest(f.get('sha256'), label + '/ordered-input') for f in analysis['inputFiles']] == hashes, f'{label}: receipt input ordering differs from analysis')
    recomputed = summarize_balance.aggregate(rows, expected_games=len(rows), require_matrix=len(rows) == 1000)
    require(all(analysis.get(key) == value for key, value in recomputed.items()), f'{label}: statistics differ from actual receipt recomputation')


def validate_balance(document, target, store):
    require(document.get('kind') == 'native-balance-acceptance' and document.get('actualNativeMatches') is True and document.get('injectedResources') is False, 'balance: real ordinary-command native matches required')
    runner = document.get('runner', {})
    require(runner.get('rulesVersion') == target['rulesVersion'] and digest(runner.get('rulesFingerprint'), 'balance runner') == target['rulesFingerprint'], 'balance runner: compiled rule identity differs from final host')
    store.file(runner.get('artifact'), 'balance-runner-artifact', publish=False)
    require('engineSha256' not in runner, 'balance runner must not impersonate the host executable')
    branch = document.get('branchMatrix', {}); rows, inputs = rows_from_refs(branch.get('inputs'), store, 'branch-matrix'); matrix_check(rows, target)
    analysis = store.json(branch.get('analysis'), 'branch-matrix-analysis'); verify_analysis(analysis, rows, inputs, target, 'branch analysis')
    strategy = document.get('strategies', {}); plan = store.json(strategy.get('plan'), 'strategy-plan'); planned_rows, strategy_inputs = rows_from_refs(strategy.get('inputs'), store, 'strategy')
    strategy_check(planned_rows, target, plan, digest(strategy['plan'].get('sha256'), 'strategy plan'))
    strategy_analysis = store.json(strategy.get('analysis'), 'strategy-analysis'); verify_analysis(strategy_analysis, planned_rows, strategy_inputs, target, 'strategy analysis')
    decision = document.get('decision', {})
    require(decision.get('status') == 'accepted' and decision.get('blockingIssues') == [], 'balance: unresolved balance review cannot release')
    criteria = decision.get('criteria', {})
    require(BALANCE_CRITERIA <= set(criteria), 'balance: separate timing/technology/roles/strategy review is incomplete')
    for key in BALANCE_CRITERIA:
        item = criteria[key]
        require(isinstance(item, dict) and item.get('outcome') == 'accepted' and isinstance(item.get('rationale'), str) and len(item['rationale'].strip()) >= 12, f'balance: criterion {key} lacks a concrete accepted review')
    return {'branchGames': len(rows), 'branchScenarios': len({scenario_key(r) for r in rows}), 'strategyGames': len(planned_rows), 'strategyScenarios': len({scenario_key(r) for r in planned_rows}), 'runnerSha256': store.references['balance-runner-artifact']['sha256']}


def validate_performance(document, target):
    identity(document, target, 'performance')
    simulation = document.get('simulation', {}); counts = simulation.get('counts', {}); gpu = document.get('gpu', {})
    require(all(integer(counts.get(k), n) for k, n in {'shells': 128, 'rooms': 512, 'units': 200, 'minimumProjectilesBeforeStep': 600}.items()), 'performance: full128/512/200/600 fixture required')
    require(integer(simulation.get('minimumMovingUnits'), 200) and integer(simulation.get('samples'), 3600) and integer(simulation.get('ticks'), 3600) and number(simulation.get('wallSeconds'), 60), 'performance: sustained native tick sampling/movement incomplete')
    require(integer(simulation.get('actualWeaponAttacks'), 1) and set(simulation.get('activeLayerSamples', {})) == LEVELS and all(integer(v, 1) for v in simulation['activeLayerSamples'].values()), 'performance: actual attacks/all8 simulated layers required')
    p99 = simulation.get('stepMs', {}).get('p99'); require(number(p99) and p99 <= 16.7, 'performance: native tick P99 exceeds16.7ms')
    require(number(gpu.get('wallSeconds'), 60) and integer(gpu.get('frames', gpu.get('frameCount')), 1800) and number(gpu.get('observedFps'), 30), 'performance: actual60second GPU readback sampling required')
    layers = gpu.get('perLayer', {})
    require(set(layers) == LEVELS and all(number(v.get('seconds'), 7) and integer(v.get('frames'), 1) and number(v.get('fps'), 30) for v in layers.values()), 'performance: all8 GPU layers must reach30FPS')
    require(gpu.get('errors') == [] and gpu.get('truncatedFrames') == 0, 'performance: renderer errors/truncation remain')
    diagnostics = gpu.get('frameDiagnostics', [])
    require(len(diagnostics) >= 2 and all(v.get('width') == 1280 and v.get('height') == 720 and v.get('meshFallbacks') == 0 and v.get('truncated') is False and isinstance(v.get('deviceName'), str) and v['deviceName'].strip() for v in diagnostics), 'performance: real1280x720 GPU outputs without fallback required')
    return {'tickP99Ms': p99, 'minimumLayerFps': min(v['fps'] for v in layers.values()), 'samples': simulation['samples'], 'maximumTickMs': simulation.get('stepMs', {}).get('max'), 'maximumFrameGapMs': gpu.get('maxInterframeGapMs')}


def validate_lan(document, target):
    identity(document, target, 'LAN', payload=True)
    require(document.get('finalEligible') is True and document.get('short') is False, 'LAN: a short/nonfinal report cannot release')
    require(number(document.get('requestedSeconds'), 2700) and number(document.get('observedCombatWallSeconds'), 2700), 'LAN: actual45minute combat wall time required')
    clients = document.get('clients')
    require(isinstance(clients, list) and len(clients) == 2, 'LAN: exactly two independently observed clients required')
    require(all(integer(v.get('enginePid'), 1) for v in clients) and len({v['enginePid'] for v in clients}) == 2, 'LAN: two real independent native processes required')
    for client in clients:
        identity(client, target, 'LAN client', payload=True)
        require(client.get('rgbaLocal') is True and integer(client.get('frames'), 1) and integer(client.get('accepted'), 1) and integer(client.get('movedUnits'), 1) and isinstance(client.get('device'), str) and client['device'].strip(), 'LAN: each client must render locally and actually issue/move paid units')
    for field in ('allCombat', 'allOre'):
        require(isinstance(document.get(field), list) and len(document[field]) == 2 and all(number(v) and v > 0 for v in document[field]), f'LAN: both players need actual {field}')
    require(LAN_CHECKS <= set(document.get('coverage', [])), 'LAN: delay/dedup/recovery/timeout/save/replay/camera coverage incomplete')
    consistency = document.get('consistency', {})
    require(integer(consistency.get('tick'), 1) and digest(consistency.get('authorityOwnerViewSha256'), 'LAN authority state') == digest(consistency.get('replicaSha256'), 'LAN replica state'), 'LAN: aligned owner-visible state hashes must agree')
    require(document.get('errors') == [] and document.get('replayExact') is True, 'LAN: unresolved errors or inexact replay')
    return {'observedCombatWallSeconds': document['observedCombatWallSeconds'], 'clients': len(clients), 'localRgbaFrames': [c['frames'] for c in clients]}


def validate_cold_start(document, target, project, candidate):
    identity(document, target, 'cold start', payload=True)
    require(document.get('kind') == 'isolated-cold-start' and document.get('actualNativeExecution') is True, 'cold start: actual isolated package execution required')
    directory = document.get('isolatedDirectory')
    require(isinstance(directory, str) and Path(directory).is_absolute(), 'cold start: isolated absolute directory required')
    directory = Path(directory).resolve()
    require(not directory.is_relative_to(Path(project).resolve()) and not directory.is_relative_to(Path(candidate).resolve()), 'cold start: developer/candidate working directory is not isolated')
    require(document.get('initialRuntimeFiles') == {'saves': 0, 'tokens': 0, 'logs': 0}, 'cold start: preexisting runtime state invalidates clean start')
    require(document.get('usedBundledRuntime') is True and document.get('requiresCodex') is False and document.get('externalCodeServicesUsed') == [], 'cold start: standalone bundled runtime proof required')
    require({'solo', 'create', 'join', 'save', 'load', 'replay'} <= set(document.get('completedFlows', [])), 'cold start: required standalone flows incomplete')
    require(integer(document.get('nativeRgbaFrames'), 1) and document.get('errors') == [] and document.get('finalEligible') is True, 'cold start: final real renderer/flow acceptance required')
    processes = document.get('processes', [])
    require(isinstance(processes, list) and len(processes) >= 2 and all(integer(p.get('pid'), 1) and isinstance(p.get('executable'), str) and Path(p['executable']).is_absolute() and Path(p['executable']).resolve().is_relative_to(directory) for p in processes), 'cold start: executable dependencies must be from the isolated package')
    require(len({p['pid'] for p in processes}) == len(processes), 'cold start: duplicate process observations')
    require({'node.exe', 'engine-host.exe'} <= {Path(p['executable']).name.lower() for p in processes}, 'cold start: both bundled Node and native engine processes must be observed')


@dataclass
class GateResult:
    facts: dict
    store: EvidenceStore


def validate_release(acceptance_path, expected, evidence_root, repository, candidate):
    acceptance = read_json(acceptance_path)
    require(acceptance.get('schemaVersion') == 2 and acceptance.get('version') == 6 and acceptance.get('status') == 'approved', 'release: schema2 explicit final approval required')
    require(acceptance.get('syntheticEvidence') is not True, 'release: synthetic acceptance is forbidden')
    target = acceptance.get('target', {})
    for key in ('engineSha256', 'rulesFingerprint', 'payloadSha256', 'webManifestSha256', 'mediaManifestSha256'):
        require(digest(target.get(key), 'release/' + key) == expected[key], 'release: target mismatch for ' + key)
    require(target.get('rulesVersion') == expected['rulesVersion'] and isinstance(target.get('rulesVersion'), str) and target['rulesVersion'].strip(), 'release: rulesVersion mismatch')
    store = EvidenceStore(evidence_root); refs = acceptance.get('evidence', {})
    required = ('hostIdentity', 'nativeBuild', 'functionality', 'balance', 'performance', 'lan', 'coldStart')
    require(isinstance(refs, dict) and set(required) <= set(refs), 'release: independent evidence sections missing')
    documents = {name: store.json(refs[name], name) for name in required}
    validate_host(documents['hostIdentity'], target, store)
    validate_build(documents['nativeBuild'], target, store, repository)
    validate_functionality(documents['functionality'], target, store)
    facts = {'balance': validate_balance(documents['balance'], target, store),
             'performance': validate_performance(documents['performance'], target),
             'lan': validate_lan(documents['lan'], target)}
    validate_cold_start(documents['coldStart'], target, evidence_root, candidate)
    facts['target'] = target
    return GateResult(facts, store)
