"""Turn newly completed provider MP4s into runtime atlases while production runs."""
import argparse
import json
import time
import importlib
from pathlib import Path
import extract_clip
from merge_building import merge

HERE=Path(__file__).resolve().parent
PROJECT=HERE.parents[1]
parser=argparse.ArgumentParser()
parser.add_argument('--watch',action='store_true')
args=parser.parse_args()
deadline=time.monotonic()+10800
while True:
    importlib.reload(extract_clip)
    for receipt in sorted((HERE/'jobs').glob('*/video.json')):
        folder=receipt.parent
        if (folder/'extraction.json').exists() or (folder/'extraction-failure.json').exists(): continue
        try: extract_clip.extract(folder.name)
        except Exception as error:
            (folder/'extraction-failure.json').write_text(json.dumps({'id':folder.name,'error':str(error)[:600]},indent=2),encoding='utf-8')
            print(json.dumps({'id':folder.name,'mediaExtractionStopped':str(error)[:500]}),flush=True)
    for working in sorted((PROJECT/'Content/Animations/v5/buildings').glob('*-work.json')):
        slug=working.stem[:-5]
        merged=working.with_name(slug+'.json')
        if merged.exists(): continue
        if all(working.with_name(f'{slug}-{s}.json').exists() for s in ['land','work','destroy']): merge(slug)
    if not args.watch or time.monotonic()>deadline: break
    if len(list((HERE/'jobs').glob('*/extraction.json')))>=44: break
    time.sleep(15)
