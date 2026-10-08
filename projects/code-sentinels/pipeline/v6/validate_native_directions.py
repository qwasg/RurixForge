"""Actual GPU vector-direction equivalence and directional FX texel checks."""
from media import *
character_run=sys.argv[sys.argv.index('--characters')+1] if '--characters' in sys.argv else 'media-lifecycle-depthfix-20260911'
fx_run=sys.argv[sys.argv.index('--effects')+1] if '--effects' in sys.argv else 'fx-lifecycle-depthfix-20260911'
for label in [character_run,fx_run]:
 if not all(c in 'abcdefghijklmnopqrstuvwxyz0123456789-' for c in label):raise ValueError('Invalid label')
chars=PROJECT/'Logs/v6'/character_run;fx=PROJECT/'Logs/v6'/fx_run;cr=read(chars/'capture-report.json');fr=read(fx/'capture-report.json');char_by_file={e['file']:e for e in cr['captures']};base=np.asarray(Image.open(fx/'baseline.png').convert('RGBA'));results=[];deaths=[];issues=[]
for entry in fr['captures']:
 expected=entry.get('expected',[])
 if not expected:continue
 if expected[0].get('kind')=='world-vector-facing-check':
  char=expected[0]['character'];local=5 if entry['file'].endswith('-5.png') else 9;other=char_by_file.get(f'{char}-death-{local:02}.png');same=other is not None and entry['rgbaSha256']==other['rgbaSha256'];deaths.append({'character':char,'frameInDeath':local,'eightExplicitFacingsMatchNativeWorldVectorFacingsByteForByte':same,'nativeGpuFile':entry['file'],'comparisonFile':other['file'] if other else None})
  if not same:issues.append(entry['file']+': facing-vector GPU identity mismatch')
  continue
 if expected[0].get('kind')!='directional-rotation-check':continue
 v=expected[0];asset=v['asset'];doc=read(PROJECT/'Content/Animations/v6/effects'/f'{asset}.json');x,y,w,h=doc['boxes'][v['frame']];source=np.asarray(Image.open(PROJECT/'Content/Animations/v6/effects'/f'{asset}.png').convert('RGBA'))[y:y+h,x:x+w];actual=np.asarray(Image.open(fx/entry['file']).convert('RGBA'));dx,dy=DIRECTION_MAP[v['directionIndex']]['worldDelta'];rotation=math.atan2(-(dx+dy)*.25,(dx-dy)*.5);co,si=math.cos(rotation),math.sin(rotation);span=float(np.float32(float(np.float32(doc.get('nativePlaneSpan',2.5456)))*float(np.float32(1.8))));pivot=doc['pivot'];foot=v['expectedFoot'];points=[]
 for u,sv in [(0,0),(1,0),(1,1),(0,1)]:
  ax=(u-pivot[0])*span;ay=(pivot[1]-sv)*span;points.append([foot[0]+(ax*co-ay*si)*90,foot[1]-(ax*si+ay*co)*90])
 px=np.arange(max(0,math.floor(min(p[0] for p in points))),min(1920,math.ceil(max(p[0] for p in points))));py=np.arange(max(0,math.floor(min(p[1] for p in points))),min(1080,math.ceil(max(p[1] for p in points))));ax=(px[None,:]+.5-foot[0])/90;ay=-(py[:,None]+.5-foot[1])/90;u=(ax*co+ay*si)/span+pivot[0];sv=pivot[1]-(-ax*si+ay*co)/span;qx=u*w;qy=sv*h;inside=(u>=0)&(u<1)&(sv>=0)&(sv<1);uncertain=(np.abs(qx-np.round(qx))<.025)|(np.abs(qy-np.round(qy))<.025);tx=np.clip(np.floor(qx).astype(int),0,w-1);ty=np.clip(np.floor(qy).astype(int),0,h-1);sample=source[ty,tx].astype(float);background=base[py[:,None],px[None,:],:3].astype(float);gpu=actual[py[:,None],px[None,:],:3].astype(float);alpha=sample[:,:,3:4]/255;predicted=np.clip(sample[:,:,:3]*alpha+background,0,255);meaningful=inside&~uncertain&(np.abs(predicted-background).max(2)>8);count=int(meaningful.sum());error=np.abs(predicted-gpu);match=float((error.max(2)[meaningful]<=3).mean()) if count else 0;results.append({'asset':asset,'direction':DIRS[v['directionIndex']],'worldDelta':[dx,dy],'expectedRotationRadians':rotation,'nativeGpuFile':entry['file'],'meaningfulPixelsCompared':count,'texelMatchWithin3Bytes':match})
 if count<100 or match<.98:issues.append(f'{asset}/{DIRS[v["directionIndex"]]}: directional source consumption mismatch {match:.5f}')
result={'at':stamp(),'engineHash':fr['engineHash'],'scope':'GPU image identity for explicit-facing versus world-vector-derived death rendering, and GPU directional effect pixels versus the mathematical isometric target vector and real source texels. This does not claim earned movement or combat.','deathDirectionChecks':deaths,'directionalEffects':results,'issues':issues,'pass':cr.get('captured',False) and fr.get('captured',False) and len(deaths)==14 and len(results)==16 and not issues}
save(fx/'gpu-directions-and-tails.json',result);emit({'engineHash':result['engineHash'],'deathCases':len(deaths),'directionalEffects':len(results),'issues':issues,'pass':result['pass']})
