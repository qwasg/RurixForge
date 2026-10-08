"""Apply only the explicitly visually reviewed larger-canvas falls."""
from media import *
for job,verdict in read(HERE/'reviewed-centered-deaths.json').items():
 folder=HERE/'jobs'/job;receipt=read(folder/'video.json')
 if (folder/'source-review.json').exists() and read(folder/'source-review.json').get('approved'):continue
 save(folder/'source-review.json',{'id':job,'approved':True,'reviewer':'Codex direct inspection of original complete video contact','reviewedAt':stamp(),'segments':verdict['segments'],'notes':verdict['notes'],'videoSha256':receipt['sha256']})
 extract(folder,True)
