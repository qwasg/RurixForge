"""Prepare chroma canvases and per-clip requests; never generate animation pixels."""
import json
import argparse
from pathlib import Path
from PIL import Image

HERE = Path(__file__).resolve().parent
PROJECT = HERE.parents[1]
OUT = HERE / 'references'
OUT.mkdir(parents=True, exist_ok=True)
BUILDINGS = {
 'command-core': 'command headquarters; two radar dishes gently scanning, antenna lamps blinking and cyan holographic core circulating',
 'data-center': 'GPU data center; cooling fan blades rotating, cabinet status LEDs sequencing and cyan coolant flowing in fixed pipes',
 'wind-power': 'wind generator; the three turbine blades make exactly one complete smooth mechanical rotation; the mast and foundation remain absolutely still',
 'hydro-power': 'hydroelectric generator; water turbine blades turning and a narrow contained water stream circulating inside the existing channels; no water appears outside the footprint',
 'coal-power': 'coal fired power plant; internal furnace flickering and a thin puff of dark smoke rising from its existing chimney, then dissipating within the padded image',
 'nuclear-power': 'nuclear power plant; cooling steam gently rising from existing vents, the sealed cyan reactor core pulsing softly; no radiation cloud or new structures',
 'mobile-relay': 'mobile compute relay; antenna gently scanning, a small blue signal ring pulsing around the existing antenna; parked wheels and chassis remain still',
 'research-lab': 'research laboratory; a small contained hologram rotating and status lights sequencing on the existing lab, research instruments moving subtly',
 'resource-extractor': 'resource extractor; its existing mechanical drill and conveyor move rhythmically, with tiny contained mineral sparks at the drill tip',
 'cudad-wall': 'cudad armored wall segment; a subtle cyan charge travels along existing circuits, central coil rotating slowly; all wall panels remain locked in place',
 'vscode-turret': 'VS Code precision compiler turret; its existing precision cannon scans a small horizontal arc and returns to center, blue cooling lights cycle while the fixed foundation stays still; preserve the supplied official VS Code symbol exactly',
 'pycharm-turret': 'PyCharm compiler turret; the existing green-tinted debugging cannon charges softly with rotating mechanical cooling parts, returns to the reference pose and does not fire; preserve the supplied official PyCharm symbol exactly',
}
BASE = ('Create a production-quality real-time-strategy sprite animation from the supplied exact building. '
        'LOCKED isometric orthographic camera, exact original perspective, exact original proportions, scale and colors. '
        'No camera movement, no zoom, no rotation of the whole building, no cuts, no text, no logos, no HUD. '
        'Solid uniform pure chroma magenta #FF00FF background must remain completely unchanged; no floor, scenery, horizon or gradient. '
        'Keep all smoke, particles, shadows and fragments well inside the frame with at least 12 percent clear margin. '
        'Only the specified building and its local animation appear. The building base is centered horizontally and ends at image y=82 percent. ')

def save(path, value):
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2), encoding='utf-8')

def main(only=None):
    jobs=[]
    for slug, work in BUILDINGS.items():
        if only and slug not in only: continue
        source=PROJECT/('Content/UI/v5/building-references' if slug.endswith('-turret') else 'Content/UI/v4/art')/f'{slug}.png'
        if not source.exists(): continue
        if any((HERE/'jobs'/f'{slug}-{s}'/'attempt.json').exists() for s in ['land','work','destroy']):
            continue
        original=Image.open(source).convert('RGBA')
        bounds=original.getchannel('A').getbbox()
        if not bounds:
            raise ValueError(f'Empty original building {slug}')
        cut=original.crop(bounds)
        ratio=min(656/cut.width,656/cut.height)
        resized=cut.resize((round(cut.width*ratio),round(cut.height*ratio)),Image.Resampling.LANCZOS)
        x=(1024-resized.width)//2
        y=840-resized.height
        for state,dy in [('ground',0),('air',-85)]:
            canvas=Image.new('RGBA',(1024,1024),(255,0,255,255))
            canvas.alpha_composite(resized,(x,y+dy))
            canvas.convert('RGB').save(OUT/f'{slug}-{state}.png')
        common={'category':'building','building':slug,'model':'MiniMax/MiniMax-H3','resolution':'768P','durationSec':4,
                'sourceImage':str(source.relative_to(PROJECT)).replace('\\','/'),'key':'magenta','frameSize':256,
                'referenceGeometry':{'canvas':[1024,1024],'originalAlphaBbox':bounds,'fittedSize':resized.size,'groundTopLeft':[x,y],'groundBaseY':840,'airOffsetY':-85}}
        actions={
          'land': ('At the start the intact building hovers just slightly above its final foundation location. '
                   'Over the first 1.4 seconds it descends smoothly vertically by only the small offset visible between the two provided reference images. '
                   'At 1.4 seconds its base makes contact: small dense grey dust puffs spread sideways from the feet, tiny amber sparks, a restrained cyan commissioning pulse. '
                   'Mechanical supports lock with one small physical settling motion. By 2.5 seconds all dust has dissipated and the intact building is firmly seated. '
                   'The final 1.5 seconds remain stable exactly matching the final reference; the building must not bounce, shrink, grow or disappear.',32,16,'air','ground'),
          'work': ('A seamless four-second working loop of this '+work+'. '
                   'The exact geometry and position of every static structural panel remain unchanged. Start and finish on the exact supplied reference pose, '
                   'with any rotating mechanical part returning to its initial position after a complete revolution. Continuous graceful motion, no explosions or damage.',48,16,'ground','ground'),
          'destroy': ('A single clear four-second destruction event. Start with the intact building still in place for 0.1 seconds. '
                      'A small amber internal impact flashes at 0.2 seconds, then the central structure fractures into recognizable original panels. '
                      'At 0.5 to 1.7 seconds it buckles and collapses downward into its own footprint; a brief warm orange explosion, short bright electrical arcs, '
                      'heavy grey dust and dark smoke expand only within the padded area. Real rigid metal debris follows short ballistic arcs then falls to the base. '
                      'From 1.7 to 3.5 seconds the smoke clears to a low heap of wreckage; by 4 seconds only still broken dark panels and a few small fading embers remain. '
                      'No new building forms, no rebuilding, no endless fireball, no expanding camera view.',48,24,'ground',None),
        }
        for state,(action,count,fps,first,last) in actions.items():
            job={**common,'id':f'{slug}-{state}','state':state,'prompt':BASE+action,
                 'references':[{'type':'first_frame','path':f'pipeline/v5/references/{slug}-{first}.png'}],
                 'outputFrames':count,'playbackFps':fps,'loop':state=='work'}
            if last:
                job['references'].append({'type':'last_frame','path':f'pipeline/v5/references/{slug}-{last}.png'})
            target=HERE/'jobs'/job['id']
            target.mkdir(parents=True,exist_ok=True)
            request=target/'request.json'
            if (target/'attempt.json').exists():
                raise RuntimeError(f'Cannot change a submitted request: {job["id"]}')
            save(request,job)
            jobs.append(job['id'])
    existing=json.loads((HERE/'building-jobs.json').read_text(encoding='utf-8')) if (HERE/'building-jobs.json').exists() else []
    save(HERE/'building-jobs.json',list(dict.fromkeys(existing+jobs)))
    print(json.dumps({'prepared':len(jobs),'references':len(BUILDINGS)*2,'paidSubmissions':0}))

if __name__=='__main__':
    parser=argparse.ArgumentParser()
    parser.add_argument('--only',nargs='+')
    main(parser.parse_args().only)
