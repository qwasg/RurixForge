"""Apply approval only to the explicitly named source after actual raw and clean-window inspection."""
from media import *
id=sys.argv[1];folder=HERE/'jobs'/id;proposal=read(HERE/'physical-state-windows.json')[id];review=read(folder/'source-review.json')
if review['segments']!=proposal['segments'] or review['videoSha256']!=read(folder/'video.json')['sha256']:raise RuntimeError('Staged source changed after inspection')
review.update({'approved':True,'reviewedAt':stamp(),'reviewer':'Codex direct inspection of full actual RGB contact and staged alpha-cleaned selected windows','status':'approved-actual-source-windows'})
save(folder/'source-review.json',review);extract(folder,True);emit({'physicalStateSourceApproved':id})
