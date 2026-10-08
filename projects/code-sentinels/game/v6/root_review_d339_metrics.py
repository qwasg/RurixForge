"""Read-only independent recount of the completed d339 release matrices.

This produces review inputs, never a balance approval or modified game result.
"""
import collections
import hashlib
import json
import pathlib
import statistics

ROOT = pathlib.Path(__file__).resolve().parents[2]
RUN = ROOT / 'game/v6/final-balance-d33999c4-20260914'
RULES = 'd33999c4bf39fea8de5f89df32e2a59c3fc2d242ba2a433078f347c6779f0d35'
BRANCHES = ['speed', 'security', 'algorithm', 'science', 'lightweight']


def distribution(values):
    vals = sorted(v for v in values if v is not None)
    def quantile(q):
        if not vals:
            return None
        i = (len(vals) - 1) * q
        lo = int(i)
        return vals[lo] + (vals[min(lo + 1, len(vals) - 1)] - vals[lo]) * (i - lo)
    return dict(observations=len(values), reached=len(vals), missing=len(values)-len(vals),
                min=min(vals) if vals else None, p10=quantile(.1),
                median=statistics.median(vals) if vals else None,
                p90=quantile(.9), max=max(vals) if vals else None)


def observe(record, row, slot):
    record['observations'] += 1
    record['wins' if row['winner'] == slot + 1 else 'losses' if row['winner'] in (1, 2) else 'unresolved'] += 1


def counter():
    return {'observations': 0, 'wins': 0, 'losses': 0, 'unresolved': 0}


def review(matrix, expected):
    files = sorted((RUN / matrix).glob('results-*.jsonl'))
    rows, refs = [], []
    for p in files:
        raw = p.read_bytes()
        refs.append(dict(path=str(p.relative_to(ROOT)).replace('\\', '/'), sha256=hashlib.sha256(raw).hexdigest()))
        rows.extend(json.loads(line) for line in raw.splitlines() if line.strip())
    assert len(rows) == expected
    assert sorted(r['index'] for r in rows) == list(range(expected))
    assert all(r['simulationSourceSha256'] == RULES for r in rows)
    assert all(r['winner'] in (None, 1, 2) for r in rows)
    assert all(bool(r['unresolvedAtBudget']) == (r['winner'] is None) for r in rows)
    if matrix == 'branch':
        for r in rows:
            i = r['index']
            pair = [BRANCHES[i // 200], BRANCHES[(i // 40) % 5]]
            swap = bool(i % 2)
            assert r['originalPair'] == pair and r['swapped'] == swap
            assert r['branches'] == (pair[::-1] if swap else pair)
            assert r['plannerOrder'] == ([2, 1] if swap else [1, 2])
            assert r['seed'] == 1000 + (i // 2) % 20
    field = 'branches' if matrix == 'branch' else 'strategies'
    cross = [r for r in rows if r[field][0] != r[field][1]]
    labels = sorted({x for r in rows for x in r[field]})
    overall = {b: counter() for b in labels}
    matchups = {b: {c: counter() for c in labels if c != b} for b in labels}
    sides = {b: {str(s): counter() for s in [1, 2]} for b in labels}
    first = {b: {'first': counter(), 'second': counter()} for b in labels}
    tech = {b: [] for b in labels}
    second_tech = {b: [] for b in labels}
    for r in cross:
        for slot, b in enumerate(r[field]):
            observe(overall[b], r, slot)
            observe(matchups[b][r[field][1-slot]], r, slot)
            observe(sides[b][str(slot+1)], r, slot)
            observe(first[b]['first' if r['plannerOrder'][0] == slot + 1 else 'second'], r, slot)
            tech[b].append(r['firstT5Seconds'][slot])
            second_tech[b].append(r['secondT5Seconds'][slot])
    bins = collections.Counter()
    for r in rows:
        t = r['seconds']
        bins['under18' if t < 1080 else '18to24' if t < 1440 else '24to30' if t < 1800 else '30to45' if t <= 2700 else 'above45'] += 1
    return dict(games=len(rows), uniqueIndices=len(set(r['index'] for r in rows)),
                completed=sum(r['winner'] in (1, 2) for r in rows),
                unresolved=[{k:r[k] for k in ['index','branches','strategies','seed','theme','plannerOrder','seconds','winner','playabilityWarnings']} for r in rows if r['winner'] is None],
                inputFiles=refs, durations=distribution([r['seconds'] for r in rows]), durationBins=dict(bins),
                reasons=dict(collections.Counter(r['winReason'] for r in rows)),
                warnings={k:sum(r['playabilityWarnings'][k] for r in rows) for k in rows[0]['playabilityWarnings']},
                crossStrategyOrBranchGames=len(cross), overall=overall, matchups=matchups,
                playerSides=sides, plannerPositions=first,
                firstT5={k:distribution(v) for k,v in tech.items()},
                secondT5={k:distribution(v) for k,v in second_tech.items()})


report = dict(kind='independent-root-matrix-recount', rulesFingerprint=RULES,
              finalBalanceAcceptance=False,
              scope='Arithmetic recount of immutable result rows; full native/Save/control integrity is independently audited elsewhere. Correlated seeds and side swaps are not independent random trials. Tech medians condition on actual reach and retain null counts.',
              branch=review('branch', 1000), strategy=review('strategy', 126))
out = RUN / 'root-independent-metrics.json'
assert not out.exists(), 'Preserve prior report; choose an explicit new filename for a justified rerun.'
out.write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
print(json.dumps({'output':str(out), 'sha256':hashlib.sha256(out.read_bytes()).hexdigest(),
                  **{m:{k:report[m][k] for k in ['games','completed','unresolved','durations','durationBins','overall','matchups','firstT5','secondT5']} for m in ['branch','strategy']}}, ensure_ascii=False, indent=2))
