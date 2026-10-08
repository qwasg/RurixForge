"""Withdraw legacy animation readiness and replace confirmed raw body-edge losses with full captures."""
from media import *
import subprocess
repairs={'claude':['sw','w','nw'],'deepseek':['sw']};manifest=read(PROJECT/'Content/UI/v6/resource-manifest.json')
for char,directions in repairs.items():
 manifest['characters'][char].update({'ready':False,'qualityStatus':'correcting-confirmed-original-body-edge-cuts','pendingSourceRepairs':[f'{char}/{d}/death' for d in directions]+(['deepseek/n/cast'] if char=='deepseek' else [])})
manifest['characterAnimationState']='production-and-visual-review';save(PROJECT/'Content/UI/v6/resource-manifest.json',manifest);save(ROOT/'packages/client/public/games/code-sentinels/ui-v6/resource-manifest.json',manifest)
for char,directions in repairs.items():
 for d in directions:
  old=HERE/'jobs'/f'{char}-{d}-{ACTION_VERSION}';review=read(old/'source-review.json')
  if 'death' in review['segments']:
   backup=old/'source-review-before-body-edge-repair.json'
   if not backup.exists():shutil.copy2(old/'source-review.json',backup)
   review['segments'].pop('death');review['reviewedAt']=stamp();review['notes']+=' Re-review of original RGB confirms the final death body/hair crosses the raw bottom edge. Death is explicitly removed and replaced with a larger real capture; other approved actual windows stay in use.';save(old/'source-review.json',review);extract(old,True)
  replacement=HERE/'jobs'/f'{char}-{d}-death-centered-v1'
  if not (replacement/'attempt.json').exists():subprocess.run([sys.executable,str(HERE/'prepare_centered_death.py'),char,d],check=True)
id='deepseek-n-cast-padded-v3';folder=HERE/'jobs'/id
if not (folder/'attempt.json').exists():
 char='deepseek';d='n';original=Image.open(HERE/'first-frames'/f'{char}-{d}.png').convert('RGB');foot=read(HERE/'reviews/reference-anchors.json')[char][d]['foot'];offset=[round(256-foot[0]),round(288-foot[1])];canvas=Image.new('RGB',(512,512),(0,255,0));canvas.paste(original,offset);ref=HERE/'first-frames/deepseek-n-cast512.png';canvas.save(ref)
 prompt=('The exact blue-haired whale-tailed maid is seen strictly from behind, showing only the back of her head and clothing. On the unchanged bright green RGB0,255,0 canvas, she performs one broad physical arm gesture. Raise both forearms forward away from the viewer, move both hands apart in a smooth outward arc, pause briefly with arms open, then lower the hands back to the exact starting posture. Her existing whale tail gently counterbalances while keeping its original shape and size. The full tail tip, head, hair, hands, dress and feet remain safely inside the added empty margins throughout. '
  'Her feet stay on their original marks and the torso remains upright. The camera is fixed elevated orthographic at the original character size. Preserve the exact hairstyle, maid headdress, dress and whale tail from the reference. Only these physical body, hair and clothing movements occur against the uniform constant green background. Complete the gesture within3 seconds, then hold the original neutral stance through the end.')
 first=str(ref.relative_to(PROJECT)).replace('\\','/');save(folder/'request.json',{'id':id,'category':'character','character':char,'direction':d,'action':'cast','firstFrame':first,'lastFrame':first,'prompt':prompt,'frames':124,'width':512,'height':512,'fps':24,'sourceFootAnchor':[foot[i]+offset[i] for i in range(2)],'outputPivot':[.5,.5625],'outputPlaneSpan':2.5456*512/384,'segments':{'cast':[0,116,32,False]},'priorityFront':True,'revisionReason':'Original cast source has a genuinely truncated whale-tail tip atf100. Wider native-pixel capture preserves complete motion and fixed sampling density; gameplay VFX stays separate.'});submit(folder)
old=HERE/'jobs/deepseek-n-cast-v2';old_review=read(old/'source-review.json');backup=old/'source-review-before-body-edge-repair.json'
if not backup.exists():shutil.copy2(old/'source-review.json',backup)
old_review.update({'approved':False,'reviewedAt':stamp(),'reason':'Independent original RGB inspection confirms whale-tail tip crosses raw bottom edge atf100; a larger genuine capture replaces the clip.','replacement':id});save(old/'source-review.json',old_review)
overrides=read(HERE/'action-overrides.json');overrides['deepseek/n/cast']=id;save(HERE/'action-overrides.json',overrides);status()
