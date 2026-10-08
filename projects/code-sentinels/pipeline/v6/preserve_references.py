"""Snapshot every actually uploaded first frame by recorded hash; no generation requests."""
from media import *
results=[]
for path in sorted((HERE/'jobs').glob('*/attempt.json')):
 folder=path.parent;attempt=read(path);workflow=read(folder/'workflow.json');target=folder/'first-frame.png'
 if not target.exists():
  upload=workflow['20']['inputs']['image'];parts=upload.rsplit('/',1);params={'filename':parts[-1],'subfolder':parts[0] if len(parts)>1 else '', 'type':'input'}
  r=requests.get(BASE+'/view',params=params,timeout=60);r.raise_for_status()
  digest=hashlib.sha256(r.content).hexdigest()
  if digest!=attempt['firstFrameSha256']:raise RuntimeError('Recorded uploaded reference hash mismatch '+folder.name)
  target.write_bytes(r.content)
 if sha(target)!=attempt['firstFrameSha256']:raise RuntimeError('Preserved reference mismatch '+folder.name)
 save(folder/'reference-preservation.json',{'file':'first-frame.png','sha256':sha(target),'matchesSubmittedFirstFrame':True,'source':'actual localhost uploaded input retrieved using recorded workflow filename'})
 results.append(folder.name)
save(HERE/'reference-preservation.json',{'at':stamp(),'preserved':len(results),'jobs':results,'allMatchRecordedHash':True,'newGenerationRequests':0})
emit({'referencesPreserved':len(results),'matches':True,'newGenerationRequests':0})
