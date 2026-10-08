"""Publish reviewed authored model bakes, including stable shared-chassis aliases."""
from media import *
BAKES=PROJECT/'Content/UI/v6/model-bakes'
PUB=ROOT/'packages/client/public/games/code-sentinels'
ALIASES={'machinegun':'autocannon','light-mortar':'mortar','scout':'scout-buggy','tank':'light-tank','aa-launcher':'aa-turret','breacher':'breach-tank','artillery':'artillery','attack-aircraft':'fighter','rail-accelerator':'rail-tank','particle-cannon':'particle-cannon','missile-truck':'missile-truck','bomber':'stealth-wing','aerospace':'aerospace-fighter','orbital-strike':'orbital-lance','anti-orbital':'aegis-array'}
FACILITIES={'data-center':'rack','research-lab':'research-console','ammo-workshop':'ammunition-workshop','ammunition-workshop':'ammunition-workshop','airfield':'airfield','aa-control':'radar-array','missile-silo':'missile-silo','fast-logistics':'logistics-belt','secure-switch':'secure-switch','fire-control':'fire-control','particle-foundry':'particle-foundry','modular-workshop':'modular-workshop','airstrip':'airfield','launchpad':'launchpad','factory':'factory','depot':'depot','repair-bay':'repair-bay','network-defense':'network-defense','energy-defense':'energy-defense','orbital-control':'orbital-control','extractor':'extractor','wind-power':'wind-power','hydro-power':'hydro-power','coal-power':'coal-power','nuclear-power':'nuclear-power','mobile-relay':'mobile-relay','drainage':'drainage-pump','drill':'drilling-rig'}
FACILITIES.update({'anti-air-control':'radar-array','rapid-logistics':'logistics-belt','secure-relay':'secure-switch','targeting-array':'fire-control','launch-pad':'launchpad'})
FACILITIES.update({'drainage':'drainage-pump','drill-workshop':'drilling-rig','data-synthesis':'research-console','wireless-relay':'secure-switch'})
only=set(sys.argv[sys.argv.index('--only')+1].split(',')) if '--only' in sys.argv else None
items=read(PROJECT/'Content/UI/v6/resource-manifest.json')['models'] if only else {}
for infofile in BAKES.glob('*/bake.json'):
 info=read(infofile);id=info['id'];kind='buildings-v6' if info['category']=='module' else 'units-v6'
 if only and id not in only:continue
 info['sourceModelSha256']=sha(PROJECT/info['sourceModel']);info['glbSha256']=sha(PROJECT/info['glb'])
 dest=PUB/kind;dest.mkdir(parents=True,exist_ok=True)
 originals=[Image.open(infofile.parent/f'{d}.png').convert('RGBA').resize((256,256),Image.Resampling.LANCZOS) for d in DIRS]
 work_folder=PROJECT/'Content/UI/v6/model-work'/id
 if (PROJECT/'Content/UI/v6/model-work-v2'/id/'work.json').exists():work_folder=PROJECT/'Content/UI/v6/model-work-v2'/id
 if (PROJECT/'Content/UI/v6/model-work-v3'/id/'work.json').exists():work_folder=PROJECT/'Content/UI/v6/model-work-v3'/id
 work=read(work_folder/'work.json') if (work_folder/'work.json').exists() else None
 if work:
  originals.extend(Image.open(work_folder/f'{d}-{i:02}.png').convert('RGBA').resize((256,256),Image.Resampling.LANCZOS) for d in DIRS for i in range(work['framesPerDirection']))
 states=[]
 for statefile in sorted((PROJECT/'Content/UI/v6/model-states'/id).glob('*/state.json')):
  state=read(statefile);state['startFrame']=len(originals);state['sourceModelSha256']=sha(PROJECT/state['sourceModel']);states.append(state)
  originals.extend(Image.open(statefile.parent/f'{d}.png').convert('RGBA').resize((256,256),Image.Resampling.LANCZOS) for d in DIRS)
 attack_folder=PROJECT/'Content/UI/v6/model-attacks'/id;attack=read(attack_folder/'attack.json') if (attack_folder/'attack.json').exists() else None
 if attack:
  attack['startFrame']=len(originals);attack['sourceModelSha256']=sha(PROJECT/attack['sourceModel'])
  originals.extend(Image.open(attack_folder/f'{d}-{i:02}.png').convert('RGBA').resize((256,256),Image.Resampling.LANCZOS) for d in DIRS for i in range(attack['framesPerDirection']))
 cols=8 if work else 4;atlas=Image.new('RGBA',(cols*256,math.ceil(len(originals)/cols)*256));boxes=[]
 bounds=[im.getbbox() for im in originals]
 if any(b is None for b in bounds):raise RuntimeError('Refusing to publish an empty direction/work frame: '+id)
 left=min(b[0] for b in bounds);top=min(b[1] for b in bounds);right=max(b[2] for b in bounds);bottom=max(b[3] for b in bounds)
 # One uniform asset-wide scale preserves all directional geometry and keeps wide ground footprints below the common anchor inside the tile.
 source_pivot_y=256*(.5+.65*math.cos(math.radians(30))/3.6)
 fit=min(1.,124/max(1,128-left,right-128),222/max(1,source_pivot_y-top),28/max(1,bottom-source_pivot_y))
 for i,original in enumerate(originals):
  im=originals[i].resize((round(256*fit),round(256*fit)),Image.Resampling.LANCZOS)
  normalized=Image.new('RGBA',(256,256));normalized.paste(im,(round(128-128*fit),round(225-source_pivot_y*fit)));atlas.paste(normalized,((i%cols)*256,(i//cols)*256));boxes.append([(i%cols)*256,(i//cols)*256,256,256])
 atlas.save(dest/f'{id}.png')
 meta={'schemaVersion':2,'id':id,'image':id+'.png','width':atlas.width,'height':atlas.height,'frameCount':len(originals),'frameSize':[256,256],'pivot':[.5,.88],'boxes':boxes,'directions':DIRS,'clips':{'idle':{d:{'start':i,'endExclusive':i+1,'fps':1,'loop':True} for i,d in enumerate(DIRS)}},'provenance':info}
 if work:
  n=work['framesPerDirection'];meta['clips']['work']={d:{'start':8+i*n,'endExclusive':8+(i+1)*n,'fps':work['fps'],'loop':True} for i,d in enumerate(DIRS)};work['sourceModelSha256']=sha(PROJECT/work['sourceModel']);meta['workProvenance']=work
 if states:
  for state in states:meta['clips'][state['state']]={d:{'start':state['startFrame']+i,'endExclusive':state['startFrame']+i+1,'fps':1,'loop':True} for i,d in enumerate(DIRS)}
  meta['stateProvenance']=states
  if id=='door':meta['clips']['closed']=meta['clips']['idle']
 if attack:
  n=attack['framesPerDirection'];start=attack['startFrame'];meta['clips']['attack']={d:{'start':start+i*n,'endExclusive':start+(i+1)*n,'fps':attack['fps'],'loop':False} for i,d in enumerate(DIRS)};meta['attackProvenance']=attack
 meta['orthographicSpan']=3.6;meta['projection']='isometric-2:1';meta['nativePlaneSpan']=3.6/math.sqrt(2)/fit;meta['bakeFitScale']=fit
 meta['sourcePivot']=[.5,source_pivot_y/256];meta['anchorMeaning']='exact projected model world origin at ground z=0'
 meta['directionConvention']=DIRECTION_CONVENTION
 meta['defaultDirection']='ne' if info['category']=='module' else 's';meta['frontModelAxis']='-Y' if info['category']=='module' else '+Y'
 if info['category']=='module':meta['defaultPlacement']='unrotated model geometry aligned to map x/y; direction index5 has root yaw-360degrees'
 save(dest/f'{id}.json',meta)
 native=PROJECT/'Content/Animations/v6'/('buildings' if info['category']=='module' else 'units');native.mkdir(parents=True,exist_ok=True)
 shutil.copy2(dest/f'{id}.png',native/f'{id}.png');shutil.copy2(dest/f'{id}.json',native/f'{id}.json')
 ui=PUB/'ui-v6/units';ui.mkdir(parents=True,exist_ok=True)
 preview_direction='ne' if info['category']=='module' else 'se'
 Image.open(infofile.parent/(preview_direction+'.png')).save(ui/f'{id}.png')
 if id.startswith('plugin-'):
  plug=PUB/'ui-v6/plugins';plug.mkdir(parents=True,exist_ok=True);shutil.copy2(ui/f'{id}.png',plug/(id.removeprefix('plugin-')+'.png'))
 items[id]={'atlas':f'/games/code-sentinels/{kind}/{id}.png','metadata':f'/games/code-sentinels/{kind}/{id}.json','nativeAtlas':str((native/f'{id}.png').relative_to(PROJECT)).replace('\\','/'),'nativeMetadata':str((native/f'{id}.json').relative_to(PROJECT)).replace('\\','/'),'image':f'/games/code-sentinels/ui-v6/units/{id}.png','sourceModel':info['sourceModel'],'glb':info['glb'],'sha256':sha(dest/f'{id}.png')}
TRANSPORT={'transport-plane':'cargo-aircraft'}
for alias,source in {**ALIASES,**FACILITIES,**TRANSPORT}.items():
 if source not in items:continue
 items[alias]={**items[source],'aliasOf':source}
 # UI catalog paths can directly use each shared chassis ID.
 shutil.copy2(PUB/'ui-v6/units'/f'{source}.png',PUB/'ui-v6/units'/f'{alias}.png') if alias!=source else None
manifest=read(PROJECT/'Content/UI/v6/resource-manifest.json')
manifest['models']=items;manifest['chassisAliases']=ALIASES
manifest['transportAliases']=TRANSPORT
manifest['directionConvention']=DIRECTION_CONVENTION
manifest['facilityAliases']=FACILITIES;manifest['roleModelMap']={k:{'model':v,'representation':'functional room equipment' if k not in ['airstrip','launchpad','wind-power','hydro-power','coal-power','nuclear-power','extractor','mobile-relay'] else 'outdoor installation','ready':v in items} for k,v in FACILITIES.items()}
manifest['assetRoots']=['Content/UI/v6','Content/Animations/v6','Content/Models/v6']
manifest['webAssetRoots']=['ui-v6','characters-v6','units-v6','buildings-v6','effects-v6','terrain-v6']
manifest['resourceModelMap']={k:{'model':k,'representation':'neutral capturable uplink equipment' if k=='strategic-node' else 'exposed mineral mesh cluster over terrain texture','ready':k in items} for k in ['strategic-node','ore-node','coal-node']}
manifest['ui']['hub']={'file':'hub/command-center.png','sha256':sha(PROJECT/'Content/UI/v6/hub/command-center.png')}
save(PROJECT/'Content/UI/v6/resource-manifest.json',manifest);save(PUB/'ui-v6/resource-manifest.json',manifest)
save(BAKES/'index.json',[read(p) for p in sorted(BAKES.glob('*/bake.json'))])
emit({'modelBakes':len(items),'sharedChassis':len([a for a in ALIASES if a in items])})
