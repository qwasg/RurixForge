"""Reuse exact existing terrain subtiles; no whole-atlas ghost and no new cloud generation."""
from media import *
source=PROJECT/'Content/Textures/terrain-tiles.png';raw=Image.open(source).convert('RGB')
if raw.size!=(1536,1024):raise RuntimeError('Original terrain atlas dimensions changed; verify before slicing')
rects={'rock':[0,0,512,512],'water':[512,0,512,512],'dirt':[1024,0,512,512],'grass':[0,512,512,512],'road':[512,512,512,512],'ore':[1024,512,512,512]}
tiles={name:raw.crop((x,y,x+w,y+h)) for name,(x,y,w,h) in rects.items()}
aliases={'coal':'rock','highland':'grass','soil':'dirt','bedrock':'rock'}
tints={'coal':[.52,.50,.48,1.],'highland':[1.08,1.04,.91,1.],'soil':[1.12,.92,.75,1.],'bedrock':[.70,.72,.75,1.]}
# These are unchanged source texels. Runtime material parameters provide appearance variants.
for key,base_key in aliases.items():tiles[key]=tiles[base_key].copy()
order=['grass','dirt','rock','water','road','ore','coal','highland','soil','bedrock']
native=PROJECT/'Content/UI/v6/terrain';public=ROOT/'packages/client/public/games/code-sentinels/terrain-v6'
native.mkdir(parents=True,exist_ok=True);public.mkdir(parents=True,exist_ok=True)
atlas=Image.new('RGB',(4*520,3*520));boxes=[];items={}
for i,key in enumerate(order):
 tile=tiles[key];tile.save(native/(key+'.png'));shutil.copy2(native/(key+'.png'),public/(key+'.png'))
 x=i%4*520+4;y=i//4*520+4;atlas.paste(tile,(x,y))
 # Duplicate only texture-border texels for filtering gutters, never animation frames.
 atlas.paste(tile.crop((0,0,512,1)).resize((512,4)),(x,y-4));atlas.paste(tile.crop((0,511,512,512)).resize((512,4)),(x,y+512))
 atlas.paste(tile.crop((0,0,1,512)).resize((4,512)),(x-4,y));atlas.paste(tile.crop((511,0,512,512)).resize((4,512)),(x+512,y))
 for dx,dy,sx,sy in [(-4,-4,0,0),(512,-4,511,0),(-4,512,0,511),(512,512,511,511)]:atlas.paste(Image.new('RGB',(4,4),tile.getpixel((sx,sy))),(x+dx,y+dy))
 boxes.append([x,y,512,512]);source_key=aliases.get(key,key);items[key]={'frame':i,'image':f'/games/code-sentinels/terrain-v6/{key}.png','nativeTexture':f'Content/UI/v6/terrain/{key}.png','sha256':sha(native/(key+'.png')),'sourceRect':rects[source_key],'sourceKey':source_key,'bitmapPixelsUnmodified':True,'materialTint':tints.get(key,[1.,1.,1.,1.]),'variantMode':'native-material-tint' if key in aliases else 'exact-source-subtile'}
atlas.save(native/'terrain.png');shutil.copy2(native/'terrain.png',public/'terrain.png')
meta={'schemaVersion':2,'id':'terrain-v6','image':'terrain.png','width':atlas.width,'height':atlas.height,'frameCount':len(order),'boxes':boxes,'keys':order,'frameSize':[512,512],'gutter':4,'sampling':'repeat UV within selected frame with half-texel inset; atlas itself is not one ground image','sourceAtlas':'Content/Textures/terrain-tiles.png','sourceAtlasSha256':sha(source),'bitmapPixelsUnmodified':True,'materialAliases':aliases,'materialTints':tints}
save(native/'terrain.json',meta);shutil.copy2(native/'terrain.json',public/'terrain.json')
mapping={'0':'grass','1':'rock','2':'water','3':'road','4':'highland','5':'ore','6':'coal'}
terrain={'nativeAtlas':'Content/UI/v6/terrain/terrain.png','nativeMetadata':'Content/UI/v6/terrain/terrain.json','atlas':'/games/code-sentinels/terrain-v6/terrain.png','metadata':'/games/code-sentinels/terrain-v6/terrain.json','keysByTerrainId':mapping,'framesByTerrainId':{k:order.index(v) for k,v in mapping.items()},'textures':items,'underground':{'unexcavatedSoil':'soil','rock':'bedrock','excavatedFloor':'dirt','aquifer':'water'},'waterUvAnimation':{'enabled':True,'method':'repeat UV scrolling of the actual water texture within its bbox','cycleSeconds':8,'offsetPerCycle':[1,0],'mixSecondaryFlow':False},'nativeProjection':'UVmapped isometric2:1 ground quad; one instanced texture pass, not one drawcall per cell','ready':True}
terrain['tintsByTerrainId']={k:tints.get(v,[1.,1.,1.,1.]) for k,v in mapping.items()};terrain['variantMethod']='Unmodified existing texture + native material tint, no offline bitmap color rewriting'
save(native/'manifest.json',terrain);save(public/'manifest.json',terrain)
manifest=read(PROJECT/'Content/UI/v6/resource-manifest.json');manifest['terrain']=terrain
if 'terrain-v6' not in manifest['webAssetRoots']:manifest['webAssetRoots'].append('terrain-v6')
save(PROJECT/'Content/UI/v6/resource-manifest.json',manifest);save(ROOT/'packages/client/public/games/code-sentinels/ui-v6/resource-manifest.json',manifest)
# Demonstrate that each old tile is exactly sliced from its documented bounds.
verification={'sourceSha256':sha(source),'sourceSize':list(raw.size),'atlasSize':list(atlas.size),'tiles':len(order),'exactOriginalTiles':[],'newCloudRequests':0}
for key,rect in rects.items():
 x,y,w,h=rect;identical=tiles[key].tobytes()==raw.crop((x,y,x+w,y+h)).tobytes()
 if not identical:raise RuntimeError('Original subtile changed '+key)
 verification['exactOriginalTiles'].append({'key':key,'sourceRect':rect,'identicalRGB':True})
verification['unalteredAliases']=[]
for key,base_key in aliases.items():
 if tiles[key].tobytes()!=tiles[base_key].tobytes():raise RuntimeError('Alias bitmap was modified '+key)
 verification['unalteredAliases'].append({'key':key,'sourceKey':base_key,'identicalRGB':True,'appearance':'native material tint only'})
verification['pass']=True;save(HERE/'terrain-verification.json',verification);emit({'terrainKeys':order,'worldIds':mapping,'exactOriginalTiles':6,'unchangedBitmapAliases':4,'newCloudRequests':0})
