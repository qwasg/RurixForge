"""Bounded three-at-a-time production; each clip owns its exclusive receipt guard."""
import argparse
import concurrent.futures
import json
from produce_clip import produce
from pathlib import Path

parser=argparse.ArgumentParser()
parser.add_argument('clips',nargs='+')
args=parser.parse_args()
if len(set(args.clips))!=len(args.clips): raise ValueError('Duplicate clip IDs are prohibited')
config=json.loads((Path(__file__).resolve().parents[4]/'data/gen-backends.json').read_text(encoding='utf-8-sig'))
workers=1 if any(b.get('id')=='comfyui-minimax-h3' and b.get('enabled') for b in config['backends']) else 3
with concurrent.futures.ThreadPoolExecutor(max_workers=workers) as executor:
    futures={executor.submit(produce,clip):clip for clip in args.clips}
    failed=[]
    for future in concurrent.futures.as_completed(futures):
        clip=futures[future]
        try: future.result()
        except Exception as exc:
            failed.append(clip)
            print(json.dumps({'id':clip,'status':'stopped','error':str(exc)[:500]},ensure_ascii=True),flush=True)
    if failed: raise SystemExit('Unfinished clips: '+', '.join(failed))
