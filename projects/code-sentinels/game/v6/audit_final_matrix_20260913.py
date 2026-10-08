"""Run existing release-gate row/analysis checks without deciding balance acceptance."""
from pathlib import Path
import argparse
import datetime
import hashlib
import json
import release_gate

ROOT = Path(__file__).resolve().parents[2]
BASE = ROOT / 'game/v6/final-balance-0acffa83-20260913'


def read(p): return json.loads(p.read_text(encoding='utf-8-sig'))
def sha(p): return hashlib.sha256(p.read_bytes()).hexdigest()
def ref(p): return dict(path=p.relative_to(ROOT).as_posix(), sha256=sha(p))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('matrix', choices=['strategy', 'branch'])
    args = parser.parse_args()
    out = BASE / args.matrix
    manifest = read(BASE / 'measurement-manifest.json')
    target = {k: manifest[k] for k in ['rulesVersion', 'rulesFingerprint']}
    analysis_path = out / 'analysis.json'
    analysis = read(analysis_path)
    inputs, rows, hashes = [], [], []
    for item in analysis['inputFiles']:
        p = Path(item['path'])
        assert p.is_relative_to(ROOT) and sha(p) == item['sha256']
        rows += [json.loads(line) for line in p.read_text(encoding='utf-8').splitlines() if line.strip()]
        inputs.append(ref(p)); hashes.append(item['sha256'])
    if args.matrix == 'strategy':
        plan_ref = manifest['matrices']['strategy']['inputPlan']
        plan_path = ROOT / plan_ref['path']
        assert sha(plan_path) == plan_ref['sha256']
        release_gate.strategy_check(rows, target, read(plan_path), plan_ref['sha256'])
    else:
        release_gate.matrix_check(rows, target)
    release_gate.verify_analysis(analysis, rows, hashes, target, args.matrix + ' final audit')
    report = dict(recordedAtUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(), scope='Existing release-gate canonical rows and exact aggregator recomputation only. No criterion decision or final balance approval.', matrix=args.matrix, **target, games=len(rows), completed=analysis['completed'], unresolved=analysis['unresolved'], passed=True, finalBalanceAcceptance=False, runner=manifest['runner'], inputs=inputs, analysis=ref(analysis_path), gate=ref(ROOT / 'game/v6/release_gate.py'), aggregate=manifest['aggregator'])
    if args.matrix == 'strategy': report['plan'] = ref(plan_path)
    dest = out / 'strict-integrity-check.json'
    with dest.open('x', encoding='utf-8', newline='\n') as f:
        json.dump(report, f, ensure_ascii=False, indent=2); f.write('\n')
    print(json.dumps(dict(matrix=args.matrix, games=len(rows), strictIntegrityPassed=True, report=str(dest), finalBalanceAcceptance=False)))


if __name__ == '__main__': main()
