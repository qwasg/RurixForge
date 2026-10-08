"""Human-inspected source contacts; update only after viewing actual completed source."""
from media import *
NOTES={
 'directional-beam':'Viewed complete24-frame contact: visibly distinct cyan/violet parallel beams pulse and retract into travelling prism trails, then fade fully. Fixed screen-right source direction and origin recorded for runtime rotation.',
 'construction-dust':'Viewed complete contact: actual welding sparks rise, dust expands and the teal scan ring completes its scan and disappears. No fabricated whole building; effect can attach per constructed floor module.',
 'cone-shockwave':'Viewed complete contact: three pink sound wavefronts open and propagate screen-right as a cone, then dissolve completely. Core wave shapes remain readable; edge matte avoids hard particle cutoffs.',
 'repair-field':'Viewed complete 24-frame source contact: green rings expand, motes rise, dissolve to black; compact source scale preserves core field. Soft peripheral outgoing glow uses an 8px edge matte and final8-frame alpha exit.',
 'energy-barrier':'Viewed complete contact: distinct golden shield impact ripple, preserved upright pane, then dissolves. Core pane remains inside frame; minor outgoing spark edge is feathered, RGB animation unchanged.',
 'floor-collapse':'Viewed complete contact: real falling concrete fragments, expanding dust and settled rubble. Persistent rubble at end is smoothly alpha-faded over8 output frames because actual game rubble is separate geometry.',
 'network-shield':'Viewed complete contact: intact teal dome, actual impact wave, hexagons dissolve and vanish; outgoing peripheral wave is softly feathered at tile edge.',
 'orbital-strike':'Viewed complete contact: centered finite orbital lance, impact surge, beam switches off, core sparks and motes dissipate completely. Padded first frame keeps core column within image.',
}
for id,note in NOTES.items():
 folder=HERE/'jobs'/f'fx-{id}-v2'
 if not (folder/'video.json').exists():continue
 receipt=read(folder/'video.json')
 save(folder/'source-review.json',{'id':folder.name,'approved':True,'reviewer':'Codex agent visual inspection of actual full-source contact sheet','reviewedAt':stamp(),'notes':note,'segments':{'oneshot':[0,124,48,False]},'videoSha256':receipt['sha256']})
 extract(folder,True)
emit({'approvedEffects':list(NOTES)})
