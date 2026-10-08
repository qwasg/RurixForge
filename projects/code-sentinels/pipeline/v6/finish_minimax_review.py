"""Apply only visually inspected source-specific matte revisions; never regenerate video."""
from media import *
for id in ['minimax-w-cast-body-v1','minimax-se-cast-body-v1']:
 folder=HERE/'jobs'/id;review=read(folder/'source-review.json')
 if review.get('darkForegroundMatte')=='reviewed-connected-low-luminance-props':continue
 review['darkForegroundMatte']='reviewed-connected-low-luminance-props';review['reviewedAt']=stamp();review['notes']+=' Exact raw RGB and native/atlas frames163/166/484 were compared: dark microphone shaft inherits green screen spill and was made translucent by chromaticity key. Preserve only original low-luminance pixels connected to the opaque figure, excluding exterior screen components; no RGB pose pixels are created or painted. Fixed2/3 source sampling is unchanged.';save(folder/'source-review.json',review);extract(folder,True)
folder=HERE/'jobs/minimax-n-attack-v2';review=read(folder/'source-review.json')
if not review.get('peripheralEdgeFadePx'):
 review['peripheralEdgeFadePx']=6;review['bodyClearOfEdgeFade']=True;review['reviewedAt']=stamp();review['notes']+=' Exact original RGB f23/f38/f54/f61 inspected: top/side border is exclusively outward gold/black energy streaks. Entire rear-facing figure, hair, clothing, microphone and legs remain clear of the6px band. Only safe peripheral alpha receives6px feather.';save(folder/'source-review.json',review);extract(folder,True)
allowances=read(HERE/'reviewed-source-edge-allowances.json');allowances['minimax/n/attack']={'sourceJob':folder.name,'classification':'peripheral-effect-only','evidence':'Exact original RGB f23/f38/f54/f61 visually inspected in reviews/minimax-n-attack-raw-boundary.jpg. Gold/black radial streaks alone reach top/side borders; hair, hands, microphone, clothes and boots remain complete with clear margins. Reviewed6px peripheral feather affects only ray endpoints.'};save(HERE/'reviewed-source-edge-allowances.json',allowances)
