"""Summarize actual native match receipts without manufacturing a balance pass."""
import argparse
from collections import Counter, defaultdict
from datetime import datetime, timezone
import hashlib
import json
import math
from pathlib import Path
import statistics

BRANCHES = ('speed', 'security', 'algorithm', 'science', 'lightweight')


def finite_number(value):
    return type(value) in (int, float) and math.isfinite(value)


def finished(row):
    return type(row.get('winner')) is int and row['winner'] in (1, 2)


def stats(values):
    supplied = list(values)
    data = sorted(float(v) for v in supplied if finite_number(v))
    def percentile(q):
        position = (len(data) - 1) * q
        low = math.floor(position)
        return data[low] + (data[min(low + 1, len(data) - 1)] - data[low]) * (position - low)
    return {'samples': len(data), 'missing': len(supplied) - len(data), **({
        'min': data[0], 'p10': percentile(.1), 'median': statistics.median(data),
        'p90': percentile(.9), 'max': data[-1], 'mean': statistics.mean(data),
    } if data else {})}


def per_owner(row, field, owner):
    values = row.get(field)
    return values[owner] if isinstance(values, list) and len(values) == 2 else None


def describe(entries):
    """One entry is one player's perspective; a self-match contributes both sides."""
    n = len(entries)
    wins = sum(finished(row) and row['winner'] == owner + 1 for row, owner in entries)
    unresolved = sum(row.get('winner') is None for row, _ in entries)
    completed = sum(finished(row) for row, _ in entries)
    metrics = {}
    for name in ('seconds', 'firstT5Seconds', 'secondT5Seconds', 'oreDelivered',
                 'actualAmmoSpent', 'actualComputeSpent', 'shotsFired', 'lostValue',
                 'nodeControlSeconds', 'recoverableCargoWreckAmount'):
        metrics[name] = stats(row.get(name) if name == 'seconds' else per_owner(row, name, owner)
                              for row, owner in entries)
    for name in ('peakAliveAi', 'activeAiCount', 'successfulActiveSkills', 'acceptedUpgrades', 'acceptedPlugins'):
        metrics[name] = stats(per_owner(row.get('orderMetrics', {}), name, owner) for row, owner in entries)
    ai_shots, ai_skills, ai_plugins = [], [], []
    for row, owner in entries:
        actors = per_owner(row.get('orderMetrics', {}), 'aiActors', owner)
        for output, key in ((ai_shots, 'shots'), (ai_skills, 'activeSkills'), (ai_plugins, 'plugins')):
            output.append(sum(actor.get(key, 0) for actor in actors) if isinstance(actors, list) else None)
    metrics.update(aiActualShots=stats(ai_shots), aiActiveSkills=stats(ai_skills), aiPlugins=stats(ai_plugins))
    loss_pairs = []
    ratios = []
    for row, owner in entries:
        own, enemy = per_owner(row, 'lostValue', owner), per_owner(row, 'lostValue', 1 - owner)
        valid = all(finite_number(v) and v >= 0 for v in (own, enemy))
        if valid:
            loss_pairs.append((own, enemy))
        ratios.append(enemy / own if valid and own > 0 else None)
    total_own = sum(own for own, _ in loss_pairs)
    total_enemy = sum(enemy for _, enemy in loss_pairs)
    first = [per_owner(row, 'firstT5Seconds', owner) for row, owner in entries]
    second = [per_owner(row, 'secondT5Seconds', owner) for row, owner in entries]
    return {
        'playerObservations': n, 'wins': wins, 'losses': completed - wins, 'unresolved': unresolved,
        'invalidOutcomes': n - completed - unresolved,
        'winFractionAllObservations': wins / n if n else None,
        'winFractionCompleted': wins / completed if completed else None,
        'firstT5Reached': sum(finite_number(v) and v >= 0 for v in first),
        'firstT5In18To22Minutes': sum(finite_number(v) and 1080 <= v <= 1320 for v in first),
        'secondT5Reached': sum(finite_number(v) and v >= 0 for v in second),
        'secondT5Before32Minutes': sum(finite_number(v) and 0 <= v < 1920 for v in second),
        'destroyedAssetExchange': {
            'scope': 'Destroyed invested asset value only. Use ratio of sums for aggregate exchange; averaging per-player ratios biases mirrored games. Ammo and compute expenditure are separate.',
            'recordedPairs': len(loss_pairs), 'missingPairs': n - len(loss_pairs),
            'ownLossTotal': total_own, 'enemyLossTotal': total_enemy,
            'aggregateEnemyOverOwnRatio': total_enemy / total_own if total_own > 0 else None,
            'bothZero': sum(own == 0 and enemy == 0 for own, enemy in loss_pairs),
            'ownZeroEnemyPositive': sum(own == 0 and enemy > 0 for own, enemy in loss_pairs),
            'perPlayerRatioDistribution': stats(ratios),
        },
        'metrics': metrics,
    }


def aggregate(rows, expected_games=None, require_matrix=False):
    issues = []
    indices = [row.get('index') for row in rows]
    if any(type(index) is not int for index in indices):
        issues.append('Match indices must be integers; booleans and missing indices are invalid.')
    if len(indices) != len(set(indices)):
        issues.append('Duplicate match indices; do not combine a pilot with a rerun of the same cases.')
    fingerprints = Counter(row.get('simulationSourceSha256') if isinstance(row.get('simulationSourceSha256'), str) else '' for row in rows)
    if len(fingerprints) != 1 or any(len(k) != 64 or any(c not in '0123456789abcdef' for c in k.lower()) for k in fingerprints):
        issues.append('All receipts must carry the same nonempty native simulation fingerprint.')
    versions = Counter(row.get('rulesVersion') if isinstance(row.get('rulesVersion'), str) else '' for row in rows)
    if len(versions) != 1 or '' in versions:
        issues.append('Rules versions are missing or inconsistent.')
    if expected_games is not None and len(rows) != expected_games:
        issues.append(f'Expected {expected_games} matches, received {len(rows)}.')
    by_branch, by_side, by_theme, by_strategy, pair = [defaultdict(list) for _ in range(5)]
    durations, warnings = [], Counter()
    scenarios = Counter()
    missing_planner_order = 0
    for row in rows:
        if row.get('winner') is not None and not finished(row):
            issues.append(f'Invalid winner in match {row.get("index")}.')
        duration = row.get('seconds')
        if not finite_number(duration) or duration <= 0:
            issues.append(f'Invalid native elapsed time in match {row.get("index")}.')
        durations.append(duration)
        warnings.update(key for key, value in row.get('playabilityWarnings', {}).items() if value)
        branches, strategies = row.get('branches', []), row.get('strategies', [])
        if len(branches) != 2 or any(b not in BRANCHES for b in branches) or len(strategies) != 2:
            issues.append(f'Invalid player definitions in match {row.get("index")}.')
            continue
        planner_order = row.get('plannerOrder', [1, 2])
        if 'plannerOrder' not in row:
            missing_planner_order += 1
        if (not isinstance(planner_order, list) or len(planner_order) != 2
                or any(type(value) is not int for value in planner_order)
                or sorted(planner_order) != [1, 2]):
            issues.append(f'Invalid planner execution order in match {row.get("index")}.')
        scenario = json.dumps([row.get('seed'), row.get('theme'), branches, strategies, planner_order], separators=(',', ':'))
        scenarios[scenario] += 1
        for owner in (0, 1):
            entry = (row, owner)
            by_branch[branches[owner]].append(entry)
            by_side[str(owner + 1)].append(entry)
            by_theme[str(row.get('theme', 'missing'))].append(entry)
            by_strategy[strategies[owner]].append(entry)
            pair[branches[owner] + '/' + branches[1 - owner]].append(entry)
        if require_matrix:
            index = row.get('index')
            if type(index) is not int or not 0 <= index < 1000:
                issues.append('Branch matrix indices must cover 0 through999.')
                continue
            expected_pair = [BRANCHES[index // 200], BRANCHES[(index // 40) % 5]]
            seed_index, swapped = (index // 2) % 20, bool(index % 2)
            expected_branches = list(reversed(expected_pair)) if swapped else expected_pair
            if (row.get('seed') != 1000 + seed_index or row.get('swapped') is not swapped
                    or row.get('originalPair') != expected_pair or branches != expected_branches
                    or strategies != ['mixed-ai', 'mixed-ai']
                    or row.get('theme') != ['river', 'mining', 'highland'][seed_index % 3]):
                issues.append(f'Match {index} does not match the approved branch/seed/sides/theme enumeration.')
    if require_matrix and set(indices) != set(range(1000)):
        issues.append('Required1000-case branch matrix is incomplete.')
    if require_matrix and missing_planner_order:
        issues.append('Final branch matrix must explicitly record plannerOrder; historical inference is only for diagnostics.')
    completed = sum(finished(row) for row in rows)
    return {
        'scope': 'Recorded native CPU matches only. These aggregates do not prove GPU, LAN, human usability or final balance acceptance.',
        'games': len(rows), 'completed': completed, 'unresolved': len(rows) - completed,
        'distinctScenarioCount': len(scenarios),
        'repeatedScenarioRuns': sum(count - 1 for count in scenarios.values()),
        'scenarioMultiplicity': dict(Counter(str(count) for count in scenarios.values())),
        'plannerOrderUnspecifiedRuns': missing_planner_order,
        'scenarioCountingNote': 'Scenario key is seed, theme, actual branch assignments, strategies and planner order. Historical runners without plannerOrder always called player1 then player2; they are counted as [1,2]. Run counts are not independent sample counts.',
        'integrityPassed': not issues, 'integrityIssues': sorted(set(issues)),
        'rulesVersions': dict(versions), 'simulationFingerprints': dict(fingerprints),
        'durationSeconds': stats(durations),
        'completedIn30To45Minutes': sum(finished(row) and finite_number(row.get('seconds')) and 1800 <= row['seconds'] <= 2700 for row in rows),
        'winsBySide': dict(Counter(str(row.get('winner')) for row in rows)),
        'victoryReasons': dict(Counter(row.get('winReason', '') for row in rows)),
        'playabilityWarningCounts': dict(warnings),
        'branches': {key: describe(value) for key, value in sorted(by_branch.items())},
        'branchMatchups': {key: describe(value) for key, value in sorted(pair.items())},
        'spawnSides': {key: describe(value) for key, value in sorted(by_side.items())},
        'themes': {key: describe(value) for key, value in sorted(by_theme.items())},
        'strategies': {key: describe(value) for key, value in sorted(by_strategy.items())},
        'finalBalanceAcceptance': False,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('inputs', nargs='+', type=Path)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--expected-games', type=int)
    parser.add_argument('--require-branch-matrix', action='store_true')
    args = parser.parse_args()
    if args.output.exists():
        raise SystemExit('Existing evidence preserved; choose a new output path.')
    rows, files = [], []
    for file in args.inputs:
        data = file.read_bytes()
        parsed = [json.loads(line) for line in data.decode('utf-8-sig').splitlines() if line.strip()]
        rows.extend(parsed)
        files.append({'path': str(file.resolve()), 'rows': len(parsed), 'sha256': hashlib.sha256(data).hexdigest()})
    if not rows:
        raise SystemExit('No actual match receipts supplied.')
    report = aggregate(rows, args.expected_games, args.require_branch_matrix)
    report.update(recordedAtUtc=datetime.now(timezone.utc).isoformat(), inputFiles=files,
                  analysisSchemaVersion=1, aggregatorSha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest())
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding='utf-8')
    print(json.dumps({'games': report['games'], 'integrityPassed': report['integrityPassed'],
                      'issues': report['integrityIssues'], 'duration': report['durationSeconds'],
                      'winsBySide': report['winsBySide'], 'warnings': report['playabilityWarningCounts'],
                      'report': str(args.output.resolve())}, ensure_ascii=False))
    if not report['integrityPassed']:
        raise SystemExit(1)


if __name__ == '__main__':
    main()
