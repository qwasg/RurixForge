"""The compiled labeling and reference flood-fill agree for the current chromaticity key."""
from media import *
results=[]
for char in CHARACTERS:
 im=Image.open(HERE/'first-frames'/f'{char}-n.png').convert('RGB');a=np.asarray(im,dtype=np.float32)/255;excess=a[:,:,1]-np.maximum(a[:,:,0],a[:,:,2]);candidate=excess/np.maximum(a[:,:,1],1/255)>.15
 flood=Image.new('L',(386,386),255);flood.paste(Image.fromarray(np.uint8(candidate)*255,'L'),(1,1));ImageDraw.floodfill(flood,(0,0),128);old=np.asarray(flood.crop((1,1,385,385)))==128
 labels,count=ndimage.label(candidate);ids=np.unique(np.concatenate([labels[0],labels[-1],labels[:,0],labels[:,-1]]));lookup=np.zeros(count+1,dtype=bool);lookup[ids]=True;lookup[0]=False;new=lookup[labels]
 if not np.array_equal(old,new):raise RuntimeError('Extraction connectivity changed '+char)
 results.append({'character':char,'masksIdentical':True})
save(HERE/'matte-equivalence.json',{'pass':True,'results':results,'dependency':'already installed scipy.ndimage, no new package installation','matteVersion':3,'connectivityImplementationsEquivalent':True});emit({'equivalent':len(results),'pass':True})
