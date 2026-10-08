"""Seal already reviewed extractions only when their actual source indices match the approved windows."""
from media import *
sealed=[];issues=[]
for path in sorted((HERE/'jobs').glob('*/source-review.json')):
 folder=path.parent;review=read(path)
 if not review.get('approved') or not (folder/'extracted.json').exists():continue
 meta=read(folder/'extracted.json')
 if meta.get('review')!='approved':continue
 valid=meta['videoSha256']==review['videoSha256']
 for action,(start,end,count,loop) in review['segments'].items():
  expected=np.linspace(start,min(end-1,meta['actualSourceFrames']-1),count).round().astype(int).tolist()
  if meta.get('sourceFrameIndices',{}).get(action)!=expected:valid=False
 if not valid:issues.append(folder.name);continue
 meta['sourceReviewSha256']=sha(path);save(folder/'extracted.json',meta);sealed.append(folder.name)
save(HERE/'review-sealing.json',{'at':stamp(),'sealed':len(sealed),'issues':issues,'sourcePixelDataChanged':False});emit({'sealedReviewedExtractions':len(sealed),'issues':issues})
