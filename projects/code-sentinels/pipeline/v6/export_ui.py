"""Export generated reference cutouts for UI, never as finished character animation."""
from media import *
out=PROJECT/'Content/UI/v6/operators'
public=ROOT/'packages/client/public/games/code-sentinels/ui-v6/operators'
out.mkdir(parents=True,exist_ok=True);public.mkdir(parents=True,exist_ok=True)
for char in CHARACTERS:
 sheet=Image.open(PROJECT/'Content/UI/v6/character-sheets'/f'{char}-directions.png').convert('RGB')
 w,h=sheet.size;portrait=key_green(sheet.crop((0,0,round(w/4),round(h/2))))
 portrait.save(out/f'{char}.png');shutil.copy2(out/f'{char}.png',public/f'{char}.png')
manifest=read(PROJECT/'Content/UI/v6/resource-manifest.json') if (PROJECT/'Content/UI/v6/resource-manifest.json').exists() else {'schemaVersion':2,'version':6,'generatedBy':'builtin-imagegen + actual local MiniMax-H3 + Blender CPU bake','characterAnimationState':'production-and-visual-review','characters':{},'ui':{}}
for char in CHARACTERS:
 existing=manifest['characters'].get(char,{})
 manifest['characters'][char]={**existing,'portrait':f'/games/code-sentinels/ui-v6/operators/{char}.png','atlas':f'/games/code-sentinels/characters-v6/{char}.png','metadata':f'/games/code-sentinels/characters-v6/{char}.json','directions':DIRS,'actions':list(SEGMENTS),'pivot':[.5,.88],'frameSize':[256,256],'ready':existing.get('ready',False)}
 manifest['ui'][char]={'file':f'operators/{char}.png','sha256':sha(out/f'{char}.png'),'purpose':'UI portrait only; not animation fallback'}
save(PROJECT/'Content/UI/v6/resource-manifest.json',manifest)
save(ROOT/'packages/client/public/games/code-sentinels/ui-v6/resource-manifest.json',manifest)
emit({'portraits':len(CHARACTERS),'manifest':str(PROJECT/'Content/UI/v6/resource-manifest.json')})
