"""Release completed production model caches only; retain service and immutable history."""
from media import *
q=get('/queue')
if q.get('queue_running') or q.get('queue_pending'):raise RuntimeError('Queue is not empty; no cache release performed')
manifest=read(PROJECT/'Content/UI/v6/resource-manifest.json')
if not all(c.get('ready') for c in manifest['characters'].values()):raise RuntimeError('Character production is not complete')
r=requests.post(BASE+'/free',json={'unload_models':True,'free_memory':True},timeout=45);r.raise_for_status()
save(HERE/'h3-cache-release.json',{'at':stamp(),'queueRunning':0,'queuePending':0,'charactersReady':7,'request':'unload_models and free_memory only','httpStatus':r.status_code,'serviceStopped':False,'historyDeleted':False,'sourceMediaDeleted':False,'authorization':'root confirmed no further H3 work; release production cache and retain service'})
emit({'cacheReleaseStatus':r.status_code,'serviceStopped':False,'historyPreserved':True})
