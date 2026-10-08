from media import *
motions={
 'construction-dust':'Welding sparks rise briefly, circular construction dust expands gently and then dissipates completely. A teal scanning ring completes one turn and vanishes. No building appears.',
 'floor-collapse':'Concrete fragments fall downward under gravity, scatter locally and settle while a localized dust cloud expands and dissipates. Do not create a full building.',
 'network-shield':'The teal hexagonal shield pulses, takes one visible impact ripple, its hexagons flicker and dissolve smoothly into data motes. No background environment.',
 'orbital-strike':'The white-gold orbital lance surges brightly for one second, a localized shock ring expands, then the beam shuts off and sparks and dust completely fade. The vertical beam remains centered.',
 'cone-shockwave':'Three pink-white sonic wavefronts expand outward toward screen right as a readable cone and fade completely, fine luminous particles follow the curved wavefronts.',
 'directional-beam':'The two cyan and violet beams fire one crisp burst toward screen right, ripple with light, then retract and fade completely with their prism sparks.',
 'energy-barrier':'The golden shield forms a stronger rim, deflects one localized impact with a visible ripple, then gently dims and dissolves to a few golden motes.',
 'repair-field':'The green repair ring gently expands as healing sparks rise in a spiral, glows softly then dissipates completely to black.'}
for id,motion in motions.items():
 source=PROJECT/'Content/UI/v6/effect-references'/f'{id}.png';ref=HERE/'first-frames'/f'fx-{id}.png'
 Image.open(source).convert('RGB').resize((384,384),Image.Resampling.LANCZOS).save(ref)
 job='fx-'+id+'-v1';prompt=('Animate this EXACT isolated game visual effect from the supplied first frame. '+motion+' Complete this one-shot effect within four seconds and hold entirely empty black for the final second. Fixed orthographic elevated camera. Pure black RGB0,0,0 background throughout. Keep all particles inside the frame. No camera movement, zoom, floor, buildings, characters, scenery, text or other objects. Preserve the exact initial effect palette, style and geometry. Detailed high quality particle motion, no still image.')
 save(HERE/'jobs'/job/'request.json',{'id':job,'category':'effect','effect':id,'firstFrame':str(ref.relative_to(PROJECT)).replace('\\','/'),'prompt':prompt,'frames':124,'width':384,'height':384,'fps':24,'chromaKey':'black','segments':{'oneshot':[0,124,48,False]},'priorityFront':True})
 submit(HERE/'jobs'/job)
status()
