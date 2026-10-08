"""Upgrade earlier boundary receipts to the all-selected-frame RGB comparison contract."""
from media import *
import concurrent.futures,subprocess
pending=[c for c in CHARACTERS if not read(HERE/f'{c}-source-boundary-verification.json').get('nativeOpaqueRgbFramesCompared')]
def run(c):
 p=subprocess.run([sys.executable,str(HERE/'verify_source_boundaries.py'),c],capture_output=True,text=True);print(p.stdout,flush=True)
 if p.returncode:raise RuntimeError(p.stderr)
 return c
with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:list(pool.map(run,pending))
