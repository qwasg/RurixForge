"""Record actual visual review and explicit source windows; never automatic approval."""
from media import *
p=argparse.ArgumentParser();p.add_argument('job');p.add_argument('--notes',required=True);p.add_argument('--segments',required=True);args=p.parse_args()
folder=HERE/'jobs'/args.job;spec=read(folder/'request.json');video=read(folder/'video.json')
segments=json.loads(args.segments)
save(folder/'source-review.json',{'id':args.job,'approved':True,'reviewer':'Codex agent visual inspection of real source contact sheet','reviewedAt':stamp(),'notes':args.notes,'segments':segments,'videoSha256':video['sha256'],'identitySource':spec.get('sourceSheet',spec.get('firstFrame'))})
if (folder/'extracted.json').exists() and not (folder/'extracted-before-review.json').exists():shutil.copy2(folder/'extracted.json',folder/'extracted-before-review.json')
extract(folder,True)
emit({'approved':args.job,'sourceWindows':segments})
