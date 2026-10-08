"""Prepare eight explicit I2V effect requests from generated reference artwork."""
import json
from pathlib import Path
from PIL import Image

HERE=Path(__file__).resolve().parent
PROJECT=HERE.parents[1]
JOBS={
 'kinetic-hit':('black',32,32,'A very short kinetic bullet impact: initial small amber contact spark expands quickly into a sharp radial spray of a few glowing metal sparks, then all sparks arc a short distance and fade. Peak at 0.5 seconds, settle by 2 seconds, completely gone by 4 seconds.'),
 'plasma-hit':('black',32,24,'A concentrated cyan plasma bolt impacts: initial bright blue seed briefly compresses, then blooms into a coherent circular turquoise ion splash with a few white lightning filaments. Peak at 0.7 seconds, contract and dissipate smoothly until fully gone.'),
 'heavy-impact':('magenta',48,24,'A heavy shell impact: a compact dust seed bursts into dense ochre-grey dust, six to ten angular metal and rock fragments follow short outward ballistic arcs, and an orange shock spark flashes at the core. Debris falls quickly, dust disperses smoothly, and the effect disappears completely by the final frame. Preserve black smoke and dark fragment surfaces.'),
 'shield-hit':('black',32,32,'A precise hit absorbed by a shield: small cyan contact ripple spreads to a bright elliptical ring with a few angular hexagonal fragments and warm gold sparks; the ring ripples once, recoils and vanishes. The effect must feel physically responsive and remain local, not become a large sphere.'),
 'power-arc':('black',32,24,'A short electrical power fault: the compact initial blue-white spark splits into three branching lightning arcs, crackles twice, contracts to the center and extinguishes. Thin cyan filaments, bright white cores, a few orange ion sparks, controlled rather than continuously flashing.'),
 'repair':('black',48,24,'A gentle technical repair pulse: the small green-cyan seed grows into a ring of rotating luminous repair motes; tiny motes spiral upward briefly and reconverge to the center, one warm ivory confirmation glint, then every mote fades. No text, medical symbols or plus signs.'),
 'upgrade':('black',48,24,'A technology upgrade confirmation: the small initial amber-cyan light forms two ascending concentric geometric rings, fine gold data motes flow upward, the central point flashes once, then rings and motes dissolve gracefully to empty background. No text or symbols.'),
 'collapse-explosion':('magenta',48,24,'A building collapse impact: the initial small hot core bursts into a brief orange fireball followed by heavy dark smoke and grey dust; recognizable angular metal fragments tumble a short distance and fall. A low elliptical shockwave expands once. The fire dies quickly, and smoke dissipates fully by the final empty reference. Keep all fragments and soot inside the image.'),
}

def main():
    available=[]
    missing=[]
    for slug,(key,count,fps,action) in JOBS.items():
        source=PROJECT/'Content/UI/v5/vfx-references'/f'{slug}.png'
        if not source.exists():
            missing.append(slug)
            continue
        target=HERE/'jobs'/slug
        target.mkdir(parents=True,exist_ok=True)
        if (target/'attempt.json').exists():
            available.append(slug)
            continue
        blank=HERE/'references'/f'empty-{key}.png'
        if not blank.exists(): Image.new('RGB',(1024,1024),(255,0,255) if key=='magenta' else (0,0,0)).save(blank)
        prepared=HERE/'references'/f'{slug}-first.png'
        original=Image.open(source).convert('RGBA').resize((1024,1024),Image.Resampling.LANCZOS)
        canvas=Image.new('RGBA',(1024,1024),(255,0,255,255) if key=='magenta' else (0,0,0,255))
        canvas.alpha_composite(original)
        canvas.convert('RGB').save(prepared)
        background='uniform pure chroma magenta #FF00FF' if key=='magenta' else 'uniform pure black #000000'
        prompt=('Create a high quality 4-second one-shot real-time-strategy combat VFX sprite animation. '
                'Use the supplied small initial impact as frame one and the supplied empty background as the exact final frame. '
                'LOCKED fixed isometric top-down view, centered effect, no camera movement, no zoom, no environment, no horizon, no object, no people, no UI, no text. '
                f'Background must remain {background}, completely unchanged, with no gradient. '
                'Build a small anticipation/contact, a fast readable main burst, then continuous physical dissipation. '
                'Maximum effect diameter 66 percent of frame; leave at least 16 percent clean margins on all sides. '
                'Nothing may touch the frame edge, no extra shot, no repeating loop. '+action)
        spec={'id':slug,'category':'effect','model':'MiniMax/MiniMax-H3','resolution':'768P','durationSec':4,
              'key':key,'frameSize':256,'outputFrames':count,'playbackFps':fps,'loop':False,
              'sourceImage':str(source.relative_to(PROJECT)).replace('\\','/'),'prompt':prompt,
              'references':[{'type':'first_frame','path':str(prepared.relative_to(PROJECT)).replace('\\','/')},
                            {'type':'last_frame','path':str(blank.relative_to(PROJECT)).replace('\\','/')}],
              'referenceGeometry':{'canvas':[1024,1024],'maxEffectSpan':.66,'safeMargin':.16}}
        (target/'request.json').write_text(json.dumps(spec,ensure_ascii=False,indent=2),encoding='utf-8')
        available.append(slug)
    (HERE/'effect-jobs.json').write_text(json.dumps(list(JOBS),indent=2),encoding='utf-8')
    print(json.dumps({'available':available,'missing':missing,'paidSubmissions':0}))

if __name__=='__main__': main()
