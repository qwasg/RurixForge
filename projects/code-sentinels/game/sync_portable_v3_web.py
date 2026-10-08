"""Apply a root-confirmed UI rebuild only to the existing V3 Web directory.
Old hashed assets remain available for pages already open during the update.
"""
import argparse,json,pathlib,re,shutil,datetime
from engine_client import ROOT,REPO
from v3_baseline import verify,digest,EVIDENCE

def main():
    parser=argparse.ArgumentParser();parser.add_argument('--confirmed-client',required=True);args=parser.parse_args()
    if not re.fullmatch(r'index-[A-Za-z0-9_-]+\.js',args.confirmed_client):raise ValueError('An exact confirmed Vite bundle name is required')
    verify()
    out=ROOT/'dist/CodeSentinels-V3-Windows';web=out/'Web';source=REPO/'packages/client/dist'
    expected=(ROOT/'dist').resolve()/'CodeSentinels-V3-Windows'
    if not out.is_dir()or out.resolve()!=expected or web.resolve()!=expected/'Web':raise RuntimeError('Unexpected V3 destination; refusing UI update')
    index=(source/'index.html').read_text(encoding='utf8')
    if f'/assets/{args.confirmed_client}'not in index:raise RuntimeError('Confirmed bundle does not match current dist')
    build=json.loads((EVIDENCE/'portable-build.json').read_text(encoding='utf8'));previous=build['clientBundle']
    copied=[]
    for p in source.rglob('*'):
        if not p.is_file()or p==source/'index.html':continue
        relative=p.relative_to(source);destination=web/relative
        if not destination.resolve().is_relative_to(web.resolve()):raise RuntimeError('UI asset escaped V3 Web directory')
        destination.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(p,destination);copied.append(str(relative).replace('\\','/'))
    # Publish the new entrypoint only after its complete asset set is present.
    shutil.copy2(source/'index.html',web/'index.html')
    verify()
    update={'at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'previousBundle':previous,'clientBundle':args.confirmed_client,'filesCopied':len(copied)+1,'scope':'V3 Web only; no native/runtime files changed; prior hashed assets retained'}
    build.setdefault('webUpdates',[]).append(update);build.update({'clientBundle':args.confirmed_client,'clientIndexSha256':digest(source/'index.html'),'validationStatus':'confirmed UI update applied; repeat browser/resource acceptance'})
    (EVIDENCE/'portable-build.json').write_text(json.dumps(build,ensure_ascii=False,indent=2),encoding='utf8')
    print(json.dumps(update,ensure_ascii=False))
if __name__=='__main__':main()
