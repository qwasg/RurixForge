"""Call the unchanged reviewed summarizer without Windows argv length limits."""
from pathlib import Path
import hashlib
import json
import runpy
import sys

ROOT = Path(__file__).resolve().parents[2]
BASE = ROOT / 'game/v6/final-balance-0acffa83-20260913'
invocation_path = Path(sys.argv[1]).resolve()
assert invocation_path.is_relative_to(BASE)
invocation = json.loads(invocation_path.read_text(encoding='utf-8'))
manifest = json.loads((BASE / 'measurement-manifest.json').read_text(encoding='utf-8'))
aggregator = ROOT / manifest['aggregator']['path']
assert hashlib.sha256(aggregator.read_bytes()).hexdigest() == manifest['aggregator']['sha256']
assert Path(invocation['command'][1]).resolve() == aggregator.resolve()
sys.argv = invocation['command'][1:]
runpy.run_path(str(aggregator), run_name='__main__')
