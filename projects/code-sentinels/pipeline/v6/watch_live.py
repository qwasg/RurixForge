"""Collect actual queued video results while applying reviewed extractor updates safely."""
import importlib,time
import media
while True:
 media=importlib.reload(media)
 errors=[]
 for path in sorted((media.HERE/'jobs').glob('*/request.json')):
  try:
   if media.collect(path.parent):media.extract(path.parent)
  except Exception as exc:
   errors.append({'job':path.parent.name,'error':str(exc)})
 for character in media.CHARACTERS:
  manifest=media.read(media.PROJECT/'Content/UI/v6/resource-manifest.json')
  if not (media.ANIM/f'{character}.json').exists() or not manifest['characters'][character].get('ready'):
   try:media.pack(character)
   except Exception as exc:errors.append({'character':character,'error':str(exc)})
 media.save(media.HERE/'collector-errors.json',{'at':media.stamp(),'errors':errors})
 media.status();time.sleep(20)
