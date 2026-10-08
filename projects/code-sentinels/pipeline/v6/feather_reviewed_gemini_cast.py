"""Soften only reviewed outer rune-ray endpoints; the complete character is well inside the band."""
from media import *
folder=HERE/'jobs/gemini-n-cast-v2';review=read(folder/'source-review.json')
review['peripheralEdgeFadePx']=6;review['bodyClearOfEdgeFade']=True;review['reviewedAt']=stamp();review['notes']+=' Raw boundary candidates are exclusively outer white rune rays; every physical body part is clear of the6px edge band. Apply a compositor alpha feather to those peripheral endpoints, never to hide a body cut.'
save(folder/'source-review.json',review);extract(folder,True)
