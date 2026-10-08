"""Stage, verify actual pixel density and raw source boundaries, then atomically publish a character."""
import sys,subprocess,hashlib,shutil,json,os
import media as m
import density_pack

def promote(character):
 staged=m.HERE/'density-staging';metadata=m.read(staged/f'{character}.json');m.ANIM.mkdir(parents=True,exist_ok=True);m.PUBLIC.mkdir(parents=True,exist_ok=True)
 archive=m.HERE/'obsolete-atlases/pre-density';archive.mkdir(parents=True,exist_ok=True)
 for ext in ['png','json']:
  old=m.ANIM/f'{character}.{ext}';backup=archive/old.name
  if old.exists() and not backup.exists():shutil.copy2(old,backup)
  temp=m.ANIM/f'{character}.{ext}.pending';shutil.copy2(staged/old.name,temp);temp.replace(old);shutil.copy2(old,m.PUBLIC/old.name)
 manifest=m.read(m.PROJECT/'Content/UI/v6/resource-manifest.json');manifest['characters'][character].update({'ready':True,'qualityStatus':'asset-verified-density-and-raw-source-boundaries','pendingSourceRepairs':[],'nativeAtlas':f'Content/Animations/v6/characters/{character}.png','nativeMetadata':f'Content/Animations/v6/characters/{character}.json','frameCount':metadata['frameCount'],'atlasSha256':m.sha(m.ANIM/f'{character}.png'),'preservePixelDensity':True,'pixelsPerWorldUnit':metadata['pixelsPerWorldUnit'],'variableFrameSize':metadata['variableFrameSize']})
 manifest['characterAnimationState']='ready' if all(v.get('ready') for v in manifest['characters'].values()) else 'production-and-visual-review';m.save(m.PROJECT/'Content/UI/v6/resource-manifest.json',manifest);m.save(m.ROOT/'packages/client/public/games/code-sentinels/ui-v6/resource-manifest.json',manifest)
 m.save(m.HERE/f'{character}-verification.json',m.read(m.HERE/f'{character}-staged-density-verification.json'));m.emit({'characterPublishedAfterVerification':character,'frames':metadata['frameCount'],'frameSizes':metadata['frameSizes'],'pixelsPerWorldUnit':metadata['pixelsPerWorldUnit']})

def _finalize_impl(character):
 overrides=m.read(m.HERE/'action-overrides.json');parts=[m.sha(m.HERE/'density_pack.py'),m.sha(m.HERE/'verify_character.py'),m.sha(m.HERE/'verify_source_boundaries.py'),m.sha(m.HERE/'reviewed-source-edge-allowances.json')]
 for d in m.DIRS:
  for action in m.SEGMENTS:
   folder=m.resolve_source(character,d,action,overrides)
   if not (folder/'extracted.json').exists() or not (folder/'source-review.json').exists():return
   review=m.read(folder/'source-review.json');extracted=m.read(folder/'extracted.json')
   if not review.get('approved') or not extracted.get('nativeFrames') or extracted.get('sourceReviewSha256')!=m.sha(folder/'source-review.json') or action not in extracted.get('clips',{}):return
   parts.extend([folder.name,action,m.sha(folder/'source-review.json'),m.sha(folder/'extracted.json')])
 fingerprint=hashlib.sha256('|'.join(parts).encode()).hexdigest();attempt=m.HERE/f'{character}-finalize-attempt.json'
 if attempt.exists() and m.read(attempt).get('fingerprint')==fingerprint:return
 packed=density_pack.pack_character(character,publish=False)
 if packed is None:return
 commands=[['verify_character.py',character,'--staging'],['verify_source_boundaries.py',character,'--staging']]
 for cmd in commands:subprocess.run([sys.executable,str(m.HERE/cmd[0]),*cmd[1:]],check=True)
 density=m.read(m.HERE/f'{character}-staged-density-verification.json');boundaries=m.read(m.HERE/f'{character}-source-boundary-verification.json');passed=density['pass'] and boundaries['pass']
 m.save(attempt,{'at':m.stamp(),'fingerprint':fingerprint,'pass':passed,'densityIssues':density['issues'],'sourceBoundaryIssues':boundaries['issues']})
 if passed:promote(character)
 else:m.emit({'characterHeldForReview':character,'densityIssues':density['issues'],'sourceBoundaryIssues':boundaries['issues']})

def finalize(character):
 lock=m.HERE/f'{character}-finalize.lock'
 try:
  with lock.open('x',encoding='utf-8') as f:json.dump({'pid':os.getpid(),'at':m.stamp(),'character':character},f)
 except FileExistsError:return
 try:return _finalize_impl(character)
 finally:lock.unlink(missing_ok=True)

if __name__=='__main__':finalize(sys.argv[1])
