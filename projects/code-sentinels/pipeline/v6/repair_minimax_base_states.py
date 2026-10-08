"""Visually failed front composite becomes distinct actual I2V states; SW gets a recovering hit."""
from media import *
overrides=read(HERE/'action-overrides.json')
motions={'idle':'Stand quietly and breathe slowly. The hair moves gently, with both feet resting on the original marks.',
 'walk':'March in place for four alternating steps. Lift the right knee and foot, return it, then lift the left knee and foot, return it. Keep both feet individually visible. Arms respond naturally while retaining the original microphone. Finish the fourth step by4 seconds and return to the original stance.',
 'hit':'Make one brief physical startle: close the eyes, pull the chin and shoulders inward, bring the elbows close to the chest and soften the knees slightly. Then straighten fully and return to the original upright relaxed pose by2 seconds. Keep both feet planted. Remain upright after recovering.',
 'death':'Lower gently onto both knees, lower the torso fully to lie on the side or face-down by3 seconds, and stay lying through the end. Keep the entire head, hair, hands, dress and feet inside the image.'}
for d,action in [('s','idle'),('s','walk'),('s','hit'),('s','death'),('sw','hit')]:
 id=f'minimax-{d}-{action}-physical-v4';folder=HERE/'jobs'/id
 if not (folder/'request.json').exists():
  reference=HERE/'first-frames'/f'minimax-{d}.png';extra={};size=384
  if action=='death':
   foot=read(HERE/'reviews/reference-anchors.json')['minimax'][d]['foot'];offset=[round(256-foot[0]),round(288-foot[1])];canvas=Image.new('RGB',(512,512),(0,255,0));canvas.paste(Image.open(reference).convert('RGB'),offset);reference=HERE/'first-frames/minimax-s-death-physical512.png';canvas.save(reference);size=512;extra={'sourceFootAnchor':[foot[i]+offset[i] for i in range(2)],'outputPivot':[.5,.5625],'outputPlaneSpan':2.5456*512/384}
  facing='front view toward the viewer' if d=='s' else 'front-left three-quarter view toward lower-left'
  prompt=('Keep the identical saturated GREEN RGB0,255,0 canvas of this reference on every frame, at the same constant brightness. The single illustrated pink/white singer stays in the exact '+facing+'. '+motions[action]+
   ' Only her physical body, hair and cloth move. Preserve the original face, hairstyle, costume and existing small microphone. Maintain the original body scale and position with a fixed elevated orthographic camera. All the empty space remains exactly the same flat pure green as the reference throughout.')
  first=str(reference.relative_to(PROJECT)).replace('\\','/');spec={'id':id,'category':'character','character':'minimax','direction':d,'action':action,'firstFrame':first,'prompt':prompt,'frames':124,'width':size,'height':size,'fps':24,'segments':{action:[0,124,32,action in ['idle','walk']]},'priorityFront':False,'revisionReason':'Front composite changes its entire backdrop to colored rings; separate physical sources replace its four unusable states.' if d=='s' else 'The SW composite flinch flows directly into falling without upright recovery; a dedicated reacting-and-recovering hit source is required.',**extra}
  if action!='death':spec['lastFrame']=first
  save(folder/'request.json',spec)
 submit(folder);overrides[f'minimax/{d}/{action}']=id;save(HERE/'action-overrides.json',overrides)
old=HERE/'jobs/minimax-s-physical-states-v3';receipt=read(old/'video.json');save(old/'source-review.json',{'id':old.name,'approved':False,'reviewedAt':stamp(),'reason':'Entire main sequence has opaque purple/green concentric backdrop changes, confirmed by originalRGB contact; replaced by four separate genuine I2V states.','videoSha256':receipt['sha256']});status()
