"""Reviewed strict-rear breathing reference, preserving the original microphone and full source cycle."""
from media import *
folder=HERE/'jobs/minimax-n-idle-physical-v1';receipt=read(folder/'video.json')
save(folder/'source-review.json',{'id':folder.name,'approved':True,'reviewedAt':stamp(),'reviewer':'Codex full original24-sample video contact inspection','notes':'Strict back of head and back of clothing remain visible throughout. One subtle physical hair/breathing cycle, original small microphone, fixed body size/location and clean constant green. No face or side profile appears. The complete source returns to its original supplied neutral pose for looping.','segments':{'idle':[0,124,18,True]},'videoSha256':receipt['sha256']});extract(folder,True)
