from media import *
for path in sorted((HERE/'jobs').glob('*/request.json')):
 if read(path).get('action') in ['attack','cast']:submit(path.parent)
status()
