"""Full-source contact sheet for selecting complete genuine action intervals."""
import argparse
import json
import math
import subprocess
import sys
from pathlib import Path
HERE=Path(__file__).resolve().parent
PROJECT=HERE.parents[1]
sys.path.insert(0,str(HERE.parent/'python-libs'))
import imageio_ffmpeg
from PIL import Image,ImageDraw

def review(clip):
    folder=HERE/'jobs'/clip
    receipt=json.loads((folder/'video.json').read_text(encoding='utf-8-sig'))
    source=PROJECT/receipt['fileRef']
    output=folder/'source-thumbs';output.mkdir(exist_ok=True)
    subprocess.run([imageio_ffmpeg.get_ffmpeg_exe(),'-v','error','-i',str(source),'-vf','fps=2,scale=320:-2',
        '-y',str(output/'frame-%03d.png')],check=True)
    thumbs=sorted(output.glob('frame-*.png'))
    frame=Image.open(thumbs[0]);width,height=frame.size
    sheet=Image.new('RGB',(width*5,(height+24)*math.ceil(len(thumbs)/5)),'#15202a')
    draw=ImageDraw.Draw(sheet)
    for i,path in enumerate(thumbs):
        x=(i%5)*width;y=(i//5)*(height+24)
        sheet.paste(Image.open(path).convert('RGB'),(x,y))
        draw.text((x+4,y+height+3),f'{clip}  t={i/2:.1f}s',fill='white')
    sheet.save(folder/'full-source-contact.jpg',quality=90)
    print(json.dumps({'id':clip,'source':str(source),'contact':str(folder/'full-source-contact.jpg'),'panels':len(thumbs)}))

if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('clip');review(parser.parse_args().clip)
