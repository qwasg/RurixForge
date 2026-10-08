"""Recover full-resolution alpha frames from immutable real MP4 sources; no generation."""
from media import *
chars=sys.argv[1].split(',') if len(sys.argv)>1 else CHARACTERS;seen=set();updated=[];overrides=read(HERE/'action-overrides.json')
for char in chars:
 for d in DIRS:
  for action in SEGMENTS:
   folder=resolve_source(char,d,action,overrides)
   if folder.name in seen:continue
   seen.add(folder.name)
   if not (folder/'extracted.json').exists() or not (folder/'source-review.json').exists() or not read(folder/'source-review.json').get('approved'):continue
   if read(folder/'extracted.json').get('nativeFrames'):continue
   extract(folder,True);updated.append(folder.name)
save(HERE/('native-frame-refresh-'+','.join(chars)+'.json'),{'at':stamp(),'updated':updated,'sourceVideosChanged':False,'downsampledPreviewsUsedForPacking':False});emit({'nativeSourceExtractions':len(updated)})
