"""Known visual-QA revision: additional fixed empty space prevents expanding effects clipping."""
from media import *
for old in sorted((HERE/'jobs').glob('fx-*-v1')):
 spec=read(old/'request.json');id=spec['effect'];ref=HERE/'first-frames'/f'fx-{id}-padded.png'
 im=Image.open(PROJECT/'Content/UI/v6/effect-references'/f'{id}.png').convert('RGB').resize((244,244),Image.Resampling.LANCZOS)
 canvas=Image.new('RGB',(384,384));canvas.paste(im,(70,70));canvas.save(ref)
 spec['id']='fx-'+id+'-v2';spec['firstFrame']=str(ref.relative_to(PROJECT)).replace('\\','/')
 spec['prompt']+=' The supplied reference now includes extra BLACK SAFETY MARGINS. Preserve this smaller exact scale. The entire effect, its expanding ripples and every bright spark must stay inside the central eighty percent of the frame. Never fill the image or crop the bright silhouette. The camera is locked; never zoom in to enlarge the effect.'
 spec['revisionReason']='Viewed v1 contact sheets: expanding shields and beam shock ring approached source boundaries. v2 uses fixed padded reference; prior actual videos and receipts retained.'
 folder=HERE/'jobs'/spec['id'];save(folder/'request.json',spec);submit(folder)
status()
