"""Use the first complete visible SE arm push, avoiding a second unrequested repetition."""
from media import *
folder=HERE/'jobs/claude-se-attack-body-v1';review=read(folder/'source-review.json');review['segments']['attack']=[0,48,16,False]
review['notes']+=' Dense source review shows a second arm push after2s; this clip uses only the first complete0-2s push/retraction to match one ordinary attack.'
review['reviewedAt']=stamp();save(folder/'source-review.json',review);extract(folder,True);pack('claude')
