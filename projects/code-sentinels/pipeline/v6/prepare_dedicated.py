"""Create independently prompted basic attacks and skills after composite-pilot review."""
from media import *
actions={
 'kimi':('one rapid short forward pen strike','a forceful directional dash strike in place, bending low then thrusting the pen and scroll forward'),
 'claude':('one precise forward palm shot','a broad defensive gesture spreading both palms to create a forward cone interception barrier, light contained near palms'),
 'gpt':('one compact forward open-palm bolt attack','an area repair and armor blessing: raise both palms upward then spread arms gently, small lavender glyph above hands'),
 'deepseek':('one forward hand flick that sends a small linked blue pulse','a sweeping directional penetration skill: lean into a clear forward two-handed wave, whale tail counterbalances behind'),
 'gemini':('one quick two-finger forward dual-beam firing gesture','an area bombardment command: point one hand upward and the other toward target, hold a clearly different commanding spell pose'),
 'minimax':('one quick forward microphone soundwave attack','an area regeneration field: raise microphone, spread free arm in a wide arc while sustaining a clearly visible vocal casting pose'),
 'glm':('one quick forward pen-point shot','a targeted support buff: open the book, trace a bright compact blue glyph above it and extend palm outward to bless ally'),
}
for char in CHARACTERS:
 for direction in DIRS:
  for action,description in zip(['attack','cast'],actions[char]):
   id=f'{char}-{direction}-{action}-v1';first=HERE/'first-frames'/f'{char}-{direction}.png'
   prompt=(f'One exact {char} game character from this reference, maintain identity hair costume colors ornaments props and exact facing. '
    'Fixed elevated orthographic full-body camera, character stays centered at the exact source scale, never turns to camera, never crosses frame edges. '
    'Pure flat chroma-key green RGB0,255,0 backdrop, no floor, environment, camera motion, zoom, cuts, titles, other people or large effects. '
    f'Perform ONLY this ONE action clearly: {description}. '
    'Begin the windup immediately. Perform the full distinct motion within the first three seconds; then return to initial neutral pose and gently hold. '
    'Both feet remain planted at the same ground anchor except a small weight shift. Do not walk, run, get hit, fall or die. '
    'Movement must be visibly readable at game sprite scale. Preserve whole silhouette inside central two-thirds of frame. A compact hand emission is allowed but no backdrop glow or particles filling the frame.')
   count=16 if action=='attack' else 24
   save(HERE/'jobs'/id/'request.json',{'id':id,'category':'character','character':char,'direction':direction,'action':action,
    'firstFrame':str(first.relative_to(PROJECT)).replace('\\','/'),'prompt':prompt,'frames':124,'width':384,'height':384,'fps':24,
    'segments':{action:[0,124,count,False]},'productionReason':'Composite pilot merged basic attack and skill; independently sampled action avoids relabeling the same gesture.'})
emit({'dedicatedCharacterJobs':112,'actions':['attack','cast'],'paidCloudCreates':0})
