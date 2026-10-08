"""Update approved unshipped actor extractions; preserve ready Claude/DeepSeek and every source video."""
from media import *
manifest=read(PROJECT/'Content/UI/v6/resource-manifest.json');overrides=read(HERE/'action-overrides.json');seen=set();updated=[]
for char in CHARACTERS:
 if manifest['characters'][char].get('ready'):continue
 for d in DIRS:
  for action in SEGMENTS:
   folder=resolve_source(char,d,action,overrides)
   if folder.name in seen:continue
   seen.add(folder.name)
   if not (folder/'extracted.json').exists() or not (folder/'source-review.json').exists():continue
   if not read(folder/'source-review.json').get('approved') or read(folder/'extracted.json').get('matteProcessing',{}).get('version',0)>=4:continue
   extract(folder,True);updated.append(folder.name)
save(HERE/'matte-v4-refresh.json',{'at':stamp(),'updated':updated,'sourceVideosChanged':False,'readyActorsRepacked':False,'reason':'Remove compressed green-screen holes enclosed by moving hair/limbs while preserving non-key costume greens'});emit({'updatedApprovedUnshippedSources':len(updated)})
