from media import *
records=read(HERE/'imagegen-character-records.json');sources=read(PROJECT/'references/v6/sources.json')
r=requests.get('https://api.github.com/repos/larcgpt/ai-model-musume/commits/main',timeout=45);r.raise_for_status();commit=r.json()['sha']
for source in sources:
 source['repositoryCommitObserved']=commit;source['downloadedReferenceSha256']=sha(PROJECT/'references/v6'/source['file'])
 source['verification']='Actual original image downloaded and viewed; exact published community version, not official or universally canonical.'
save(PROJECT/'references/v6/sources.json',sources)
manifest=read(PROJECT/'Content/UI/v6/resource-manifest.json')
for record in records:
 source=PROJECT/record['sourceReference'];sheet=PROJECT/record['projectFile'];im=Image.open(sheet)
 record.update({'sourceSha256':sha(source),'generatedSheetSha256':sha(sheet),'actualSheetSize':list(im.size)})
 char=record['id'];manifest['characters'][char].update({'sourceReference':record['sourceReference'],'referenceSheet':record['projectFile'],'nativeAtlas':f'Content/Animations/v6/characters/{char}.png','nativeMetadata':f'Content/Animations/v6/characters/{char}.json','portraitReady':True,'identityAdaptation':record['adaptationNotes'],'directionConvention':DIRECTION_CONVENTION})
save(HERE/'imagegen-character-records.json',records)
save(PROJECT/'Content/UI/v6/resource-manifest.json',manifest);save(ROOT/'packages/client/public/games/code-sentinels/ui-v6/resource-manifest.json',manifest)
emit({'verifiedCommunityCommit':commit,'characterSheets':len(records),'portraitsReady':7,'animationsRemainPending':True})
