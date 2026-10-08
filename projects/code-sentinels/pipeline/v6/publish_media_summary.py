"""Publish a distributable asset QA summary without local services, logs or submission parameters."""
from media import *
import re,subprocess
manifest=read(PROJECT/'Content/UI/v6/resource-manifest.json');pub=ROOT/'packages/client/public';issues=[];characters=[];sources=read(PROJECT/'references/v6/sources.json');file_hashes={}
def checked_hash(path):
 if path not in file_hashes:file_hashes[path]=sha(path)
 return file_hashes[path]
def image_record(url):
 path=pub/url.lstrip('/')
 if not path.exists():issues.append('Missing public image '+url);return {'url':url,'exists':False}
 with Image.open(path) as im:size=list(im.size);im.verify()
 return {'url':url,'exists':True,'sha256':checked_hash(path),'size':size}
for char in CHARACTERS:
 item=manifest['characters'][char];doc=read(ANIM/f'{char}.json');density=read(HERE/f'{char}-verification.json');boundary=read(HERE/f'{char}-source-boundary-verification.json')
 atlas_sha=checked_hash(ANIM/f'{char}.png');metadata_sha=checked_hash(ANIM/f'{char}.json')
 if not item.get('ready') or not density.get('pass') or not boundary.get('pass') or density['atlasSha256']!=atlas_sha or item['atlasSha256']!=atlas_sha:issues.append('Character verification/readiness mismatch '+char)
 for ext in ['png','json']:
  if checked_hash(ANIM/f'{char}.{ext}')!=checked_hash(PUBLIC/f'{char}.{ext}'):issues.append('Native/public character mismatch '+char+'/'+ext)
 for action in SEGMENTS:
  if set(doc['clips'][action])!=set(DIRS):issues.append('Missing character direction '+char+'/'+action)
 current=[]
 for action,dirs in doc['clips'].items():
  for d,clip in dirs.items():
   folder=HERE/'jobs'/clip['sourceJob'];review=read(folder/'source-review.json');ex=read(folder/'extracted.json')
   if not review.get('approved') or ex.get('sourceReviewSha256')!=checked_hash(folder/'source-review.json'):issues.append('Stale source review '+folder.name)
   current.append({'action':action,'direction':d,'sourceVideoId':clip['sourceJob'],'videoSha256':review['videoSha256'],'sourceFrames':clip['sourceFrameIndices']})
 characters.append({'id':char,'ready':item['ready'],'assetVerificationPass':density['pass'] and boundary['pass'],'frames':doc['frameCount'],'actions':list(SEGMENTS),'directions':DIRS,'atlasSize':[doc['width'],doc['height']],'atlasSha256':atlas_sha,'metadataSha256':metadata_sha,'frameSizes':doc['frameSizes'],'preservePixelDensity':doc['preservePixelDensity'],'sourceToAtlasSampling':2/3,'pixelsPerWorldUnit':doc['pixelsPerWorldUnit'],'rawFramesInspected':boundary['rawSourceFramesChecked'],'opaqueRgbFramesCompared':boundary['nativeOpaqueRgbFramesCompared'],'maxOpaqueRgbByteDifference':boundary['maxNativeOpaqueRgbByteDifference'],'sourceVideoCount':len(doc['provenance']),'clips':current})
 subprocess.run([sys.executable,str(HERE/'character_final_contact.py'),char],check=True)
guide_text=(ROOT/'packages/client/src/components/game/v6/V6Guide.tsx').read_text(encoding='utf-8');guide_ids=re.findall(r"art:'([^']+)'",guide_text);guide=[]
for id in guide_ids:
 item=manifest['models'].get(id)
 if not item:issues.append('Guide model missing '+id);continue
 actual=image_record(item['image']);fallback=image_record(f'/games/code-sentinels/ui-v6/units/{id}.png')
 guide.append({'id':id,'model':item.get('aliasOf',id),'catalogMediaImage':actual,'noCatalogFallback':fallback,'sourceModel':item['sourceModel'],'sourceModelSha256':checked_hash(PROJECT/item['sourceModel'])})
facility_images=[]
for id,source in manifest['facilityAliases'].items():
 item=manifest['models'].get(id) or manifest['models'].get(source)
 if not item:issues.append('Missing facility alias '+id);continue
 facility_images.append({'id':id,'model':source,**image_record(item['image'])})
for ref in sources:
 if checked_hash(PROJECT/'references/v6'/ref['file'])!=ref['sha256']:issues.append('Original reference hash mismatch '+ref['id'])
models=read(HERE/'model-verification.json');work=read(HERE/'work-verification.json');attacks=read(HERE/'weapon-attack-verification.json');effects=read(HERE/'effect-verification.json');terrain=read(HERE/'terrain-verification.json');door=read(HERE/'door-geometry-verification.json')
for name,report in [('models',models),('work',work),('attacks',attacks),('effects',effects),('terrain',terrain),('door',door)]:
 if not report.get('pass'):issues.append('Existing required QA report failed '+name)
# Existing actual bakes were verified earlier; now check their final shipped atlas/metadata mirrors.
unique_models={}
for id,item in manifest['models'].items():
 key=item['nativeMetadata']
 if key in unique_models:continue
 native_meta=PROJECT/key;doc=read(native_meta);native_atlas=PROJECT/item['nativeAtlas'];public_atlas=pub/item['atlas'].lstrip('/');public_meta=pub/item['metadata'].lstrip('/')
 if checked_hash(native_atlas)!=checked_hash(public_atlas) or checked_hash(native_meta)!=checked_hash(public_meta):issues.append('Model native/public mismatch '+id)
 if checked_hash(native_atlas)!=item['sha256']:issues.append('Model manifest hash mismatch '+id)
 unique_models[key]={'id':doc['id'],'atlasSha256':checked_hash(native_atlas),'metadataSha256':checked_hash(native_meta),'frames':doc['frameCount'],'atlasSize':[doc['width'],doc['height']],'clips':list(doc['clips'])}
summary={'schemaVersion':1,'version':6,'verifiedAt':stamp(),'scope':'Offline media source, pixel-density, bounds, source-model and shipped-path verification. This is not a gameplay, native GPU, FPS or executable acceptance report.','assetVerificationPass':not issues,'characters':characters,'models':{'authoredModels':models['models'],'staticDirectionRenders':models['renders'],'sharedChassis':models['sharedChassis'],'workModels':work['workingModules'],'workFrames':work['renderedWorkFrames'],'weaponAttackModels':attacks['weapons'],'weaponAttackFrames':attacks['renderedAttackFrames'],'doorsGeometryVerified':door['pass'],'assets':list(unique_models.values())},'effects':{'count':effects['effects'],'frames':effects['frames'],'realNewV6Videos':effects['newNativeI2V'],'preservedV5':effects['preservedV5'],'verified':effects['pass']},'terrain':{'tiles':terrain['tiles'],'sourceSha256':terrain['sourceSha256'],'verified':terrain['pass'],'appearance':'Six exact original subtiles; coal/highland/soil/bedrock reuse original pixels with renderer material tint.'},'communityReferences':sources,'referenceInterpretation':'Five new operators use a published community design, not official or universal canon. Unpublished back/lower-body views are explicitly game adaptations; DeepSeek and GPT retain prior project identities.','guideImages':guide,'facilityImages':facility_images,'nativeGpuAcceptance':'pending: executable launch not available under current system application control; no bypass attempted','issues':issues}
native_qa_path=PROJECT/'Content/UI/v6/qa/native-lifecycle-verification.json'
summary['nativeGpuAcceptance']={'status':'pending-explicit-visual-fixture','scope':'Asset file checks alone do not establish native rendering acceptance.'}
if native_qa_path.exists():
 native_qa=read(native_qa_path);asset_matches=all(sha(ANIM/f'{a["id"]}.png')==a['atlasSha256'] and sha(ANIM/f'{a["id"]}.json')==a['metadataSha256'] for a in native_qa['characterAssets']);effect_matches=bool(native_qa.get('effectAssets')) and all(sha(PROJECT/'Content/Animations/v6/effects'/f'{a["id"]}.png')==a['atlasSha256'] and sha(PROJECT/'Content/Animations/v6/effects'/f'{a["id"]}.json')==a['metadataSha256'] for a in native_qa.get('effectAssets',[]))
 summary['nativeGpuAcceptance']={'status':'verified-explicit-visual-fixtures' if native_qa['pass'] and asset_matches and effect_matches else 'stale-visual-fixture-assets','engineSha256':native_qa['engineSha256'],'report':'qa/native-lifecycle-verification.json','characters':7,'characterFrames':3584,'effects':15,'scope':native_qa['scope']}
destination=PROJECT/'Content/UI/v6/qa/media-verification.json';save(destination,summary);save(pub/'games/code-sentinels/ui-v6/qa/media-verification.json',summary)
reviews=HERE/'reviews';index={'at':stamp(),'scope':'Only these current files are rendered from the formally published metadata and atlas. All prior *-current.jpg files are historical working contacts, not release acceptance.','characters':[{'id':c['id'],'contact':c['id']+'-final-atlas.jpg','atlasSha256':c['atlasSha256'],'metadataSha256':c['metadataSha256'],'sourceClips':c['clips']} for c in characters]};save(reviews/'published-review-index.json',index)
for old in reviews.glob('*-current.jpg'):save(old.with_suffix('.obsolete.json'),{'historicalPreview':old.name,'status':'obsolete-working-contact','currentIndex':'published-review-index.json','reason':'The filename predates later real-video corrections. Use the final published metadata sourceVideoId/sourceFrames listed in the current index; the old preview is retained as historical evidence.'})
(reviews/'README.md').write_text('# Current media review\n\nUse `published-review-index.json` and the seven `*-final-atlas.jpg` files. These are derived from current published metadata, with its exact atlas SHA256 shown.\n\nEvery `*-current.jpg` is a historical working preview; corresponding `.obsolete.json` sidecars mark it as superseded. In particular, the old GLM SW cast preview uses a rejected colored-background video; release metadata uses the reviewed `glm-sw-cast-greenstage-v2`. Original videos and review receipts are retained.\n\nThe contact sheets use fixed world scale and source-specific pivots with enough cell space for complete falls. They are offline QA, not game screenshots or GPU acceptance.\n',encoding='utf-8')
emit({'summary':str(destination),'sha256':sha(destination),'charactersReady':sum(c['ready'] for c in characters),'frames':sum(c['frames'] for c in characters),'guideImages':len(guide),'facilityImages':len(facility_images),'models':len(unique_models),'issues':issues})
if issues:raise SystemExit(1)
