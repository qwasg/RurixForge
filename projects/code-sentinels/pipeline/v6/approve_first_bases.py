"""Explicit acceptance after viewing the two real cleaned composite contact sheets."""
from media import *
for char in ['claude','deepseek']:
 for d in DIRS:
  folder=HERE/'jobs'/f'{char}-{d}-{ACTION_VERSION}';review=read(folder/'source-review.json');meta=read(folder/'extracted.json');receipt=read(folder/'video.json')
  if review['videoSha256']!=receipt['sha256'] or sha(PROJECT/receipt['fileRef'])!=receipt['sha256']:raise RuntimeError('Source changed since visual review')
  review.update({'approved':True,'reviewedAt':stamp(),'status':'approved-selected-actual-windows','notes':'Viewed original full-source direction contacts, proposed real timeline windows, and final alpha-clean selected frames on dark background. Idle breathing, actual directional steps, recoil and complete settling corpse are legible. Composite attack/cast are excluded. Strict north reactions/locomotion come from separately reviewed videos; Claude-SE ink-swirled hit is excluded and replaced separately.'})
  if review.get('garbageMatte'):review['garbageMatteParameters']={'primaryOpaqueThreshold':150,'safetyMarginPixels':6,'edgeFeatherPixels':2,'subjectRGBPainted':False}
  save(folder/'source-review.json',review);meta['review']='approved';save(folder/'extracted.json',meta)
emit({'approvedBaseSources':16,'characters':['claude','deepseek'],'compositeAttackOrCastUsed':False})
