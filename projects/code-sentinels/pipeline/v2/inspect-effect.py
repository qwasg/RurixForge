"""Decode every provider VFX frame to verify motion, fade and unclipped borders."""
import argparse
import json
import sys
from pathlib import Path

HERE=Path(__file__).resolve().parent
PROJECT=HERE.parents[1]
sys.path.insert(0,str(HERE.parent/'python-libs'))
import imageio_ffmpeg
from PIL import Image,ImageChops,ImageStat

parser=argparse.ArgumentParser()
parser.add_argument('effect',choices=['deepseek-tide','gpt-nova','pycharm-matrix'])
effect=parser.parse_args().effect
source=PROJECT/f'SourceMedia/effects/{effect}.mp4'
reader=imageio_ffmpeg.read_frames(str(source),pix_fmt='rgb24')
info=next(reader)
w,h=info['size']
details=[]
for index,raw in enumerate(reader):
    image=Image.frombytes('RGB',(w,h),raw)
    r,g,b=image.split()
    peak=ImageChops.lighter(ImageChops.lighter(r,g),b)
    mask=peak.point(lambda value:255 if value>3 else 0)
    box=mask.getbbox()
    margins=[box[0],box[1],w-box[2],h-box[3]] if box else [w,h,w,h]
    details.append({'frame':index,'foregroundBbox':list(box) if box else None,'margins':margins,'energy':round(ImageStat.Stat(peak).mean[0],5)})
minima=[min(item['margins'][side] for item in details) for side in range(4)]
energy=[item['energy'] for item in details]
result={'effect':effect,'sourceVideo':str(source.relative_to(PROJECT)).replace('\\','/'),'size':[w,h],'sourceFps':info['fps'],'duration':info['duration'],
        'allDecodedFrames':len(details),'minimumMarginsLTRB':minima,'clippedFrameCount':sum(min(item['margins'])<1 for item in details),
        'minimumTenPixelMarginPassed':min(minima)>=10,'initialEnergy':energy[0],'peakEnergy':max(energy),'lastEnergy':energy[-1],
        'lastVsPeakEnergyRatio':round(energy[-1]/max(energy),5) if max(energy) else 0,'frames':details}
(HERE/f'{effect}.source-validation.json').write_text(json.dumps(result,ensure_ascii=False,indent=2),encoding='utf-8')
print(json.dumps({key:value for key,value in result.items() if key!='frames'},ensure_ascii=False))
