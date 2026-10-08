"""Authored V6 modular Blender models and eight-direction CPU sprite bakes.
Run with Blender --background --python bake_models.py -- [--only ID].
Geometry is genuine editable Blender geometry, exported as .blend and GLB.
"""
import bpy, math, json, sys, shutil, hashlib
from pathlib import Path
from mathutils import Vector
HERE=Path(__file__).resolve().parent;PROJECT=HERE.parents[1];ROOT=PROJECT.parents[1]
MODELS=PROJECT/'Content/Models/v6';OUT=PROJECT/'Content/UI/v6/model-bakes'
DIRS=['s','sw','w','nw','n','ne','e','se']
for p in [MODELS,OUT]:p.mkdir(parents=True,exist_ok=True)
bpy.ops.object.select_all(action='SELECT');bpy.ops.object.delete(use_global=False)
scene=bpy.context.scene;scene.render.engine='CYCLES';scene.cycles.device='CPU';scene.cycles.samples=12
scene.cycles.use_denoising=True;scene.render.threads_mode='FIXED';scene.render.threads=6
scene.render.resolution_x=384;scene.render.resolution_y=384;scene.render.resolution_percentage=100
scene.render.image_settings.file_format='PNG';scene.render.image_settings.color_mode='RGBA';scene.render.film_transparent=True
scene.world.color=(.17,.19,.22)
scene.view_settings.view_transform='AgX'
def material(name,color,metal=0.,emission=0.):
 m=bpy.data.materials.new(name);m.diffuse_color=(*color,1);m.use_nodes=True
 bs=m.node_tree.nodes.get('Principled BSDF');bs.inputs['Base Color'].default_value=(*color,1);bs.inputs['Metallic'].default_value=metal;bs.inputs['Roughness'].default_value=.35 if metal else .58
 if emission:bs.inputs['Emission Color'].default_value=(*color,1);bs.inputs['Emission Strength'].default_value=emission
 return m
STEEL=material('Graphite armored steel',(.115,.155,.19),.75)
EDGE=material('Brushed pale titanium',(.50,.59,.63),.8)
GOLD=material('Warm brass markings',(.62,.38,.10),.7)
DARK=material('Carbon rubber and vents',(.022,.035,.046),.12)
TEAL=material('Teal computing light',(.04,.72,.7),.45,2)
BLUE=material('Blue energy coils',(.04,.28,.9),.5,2)
ORANGE=material('Amber warning light',(.95,.29,.035),.2,1.6)
GLASS=material('Deep cyan console glass',(.04,.2,.25),.35)
CONCRETE=material('Light reinforced concrete',(.38,.42,.44),.08)
CURRENT=[]
def finish(ob,name,mat,bevel=.03):
 ob.name=name;ob.data.materials.append(mat)
 if bevel:
  mod=ob.modifiers.new('machined rounded edges','BEVEL');mod.width=bevel;mod.segments=2
  ob.modifiers.new('weighted normals','WEIGHTED_NORMAL')
 CURRENT.append(ob);return ob
def box(name,loc,scale,mat=STEEL,bevel=.035):
 bpy.ops.mesh.primitive_cube_add(size=1,location=loc);ob=bpy.context.object;ob.scale=scale;bpy.ops.object.transform_apply(location=False,rotation=False,scale=True);return finish(ob,name,mat,bevel)
def cyl(name,loc,radius,depth,mat=EDGE,vertices=16,rotation=None):
 bpy.ops.mesh.primitive_cylinder_add(vertices=vertices,radius=radius,depth=depth,location=loc,rotation=rotation or (0,0,0));return finish(bpy.context.object,name,mat,.015)
def cone(name,loc,r1,r2,depth,mat=STEEL,vertices=12):
 bpy.ops.mesh.primitive_cone_add(vertices=vertices,radius1=r1,radius2=r2,depth=depth,location=loc);return finish(bpy.context.object,name,mat,.018)
def sphere(name,loc,scale,mat=GLASS):
 bpy.ops.mesh.primitive_uv_sphere_add(segments=16,ring_count=8,radius=1,location=loc);ob=bpy.context.object;ob.scale=scale;return finish(ob,name,mat,0)
def beam_between(name,a,b,r,mat=EDGE):
 delta=Vector(b)-Vector(a);ob=cyl(name,(Vector(a)+Vector(b))/2,r,delta.length,mat);ob.rotation_euler=delta.to_track_quat('Z','Y').to_euler();return ob
def panel(loc,width=.65,height=.38):
 box('glowing status panel',loc,(width,.035,height),GLASS,.012)
 for i in range(4):box('telemetry line',(loc[0],loc[1]-.025,loc[2]-.12+i*.075),(width*.75,.015,.014),TEAL,.004)
def rivets(z=0.5,length=1.6):
 for x in [-.55,.55]:
  for y in [-length*.38,0,length*.38]:sphere('brass fastener',(x,y,z),(.035,.035,.018),GOLD)
def track(x,length=1.85):
 box('carbon continuous track',(x,0,.27),(.30,length,.47),DARK,.09)
 for i in range(11):box('individual armored tread',(x,-length*.46+i*length*.092,.49),(.32,.105,.06),EDGE,.012)
 for y in [-.64,-.32,0,.32,.64]:cyl('track road wheel',(x+(.17 if x>0 else -.17),y,.28),.155,.055,GOLD,16,(0,math.pi/2,0))
def wheel(x,y):cyl('tire',(x,y,.29),.27,.20,DARK,20,(0,math.pi/2,0));cyl('wheel hub',(x+(.11 if x>0 else -.11),y,.29),.135,.024,EDGE,12,(0,math.pi/2,0))
def barrel(x=0,z=1.04,length=.95,kind='kinetic'):
 if kind=='energy':
  for dx in [-.12,.12]:box('split accelerator rail',(x+dx,.46+length/2,z),(.095,length,.115),EDGE,.016)
  for y in [.5,.72,.94]:cyl('magnetic coil',(x,y,z),.16,.10,BLUE,16,(math.pi/2,0,0))
  box('energy core',(x,.53,z),(.20,.34,.12),TEAL,.012)
 else:
  beam_between('main cannon',(x,.3,z),(x,.55+length,z+.1),.073,EDGE)
  beam_between('muzzle brake',(x,.48+length,z+.095),(x,.62+length,z+.11),.10,DARK)
def missiles(x=0,z=1.05):
 for dx in [-.18,.18]:
  for dz in [0,.22]:
   box('missile cassette',(x+dx,.22,z+dz),(.19,.90,.18),STEEL,.035)
   beam_between('missile nose',(x+dx,.67,z+dz),(x+dx,.89,z+dz+.06),.065,ORANGE)
def turret(kind='kinetic',tier=1):
 cone('octagonal emplacement',(0,0,.16),.75,.65,.26,CONCRETE,8)
 cyl('azimuth bearing',(0,0,.36),.52,.18,DARK,24)
 box('armored turret',(0,0,.70),(1.0,.85,.52),STEEL,.12)
 box('gold identification bar',(0,-.44,.8),(.70,.035,.06),GOLD,.015)
 for x in [-.45,.45]:box('side armor module',(x,0,.74),(.12,.68,.35),EDGE,.025)
 if kind=='missile':missiles(z=1.0)
 elif kind=='mortar':
  beam_between('mortar barrel',(0,.1,.80),(0,.67,1.40),.17,DARK)
  beam_between('mortar rim',(0,.57,1.30),(0,.68,1.43),.20,EDGE)
 elif kind=='orbital':
  cyl('orbital lens',(0,0,1.04),.40,.20,BLUE,32)
  for a in range(0,360,60):
   x=math.cos(math.radians(a))*.65;y=math.sin(math.radians(a))*.65
   beam_between('phased emitter arm',(x*.5,y*.5,.80),(x,y,1.48),.07,EDGE);sphere('emitter crystal',(x,y,1.50),(.11,.11,.15),TEAL)
 else:
  barrel(z=.85,length=.6+.09*tier,kind=kind)
  if tier>=4:barrel(x=.25,z=.86,length=.6,kind=kind)
  cyl('sensor',(0,-.25,1.04),.10,.13,ORANGE)
def tank(kind='kinetic',tier=2,light=False):
 if light:
  for x in [-.63,.63]:
   for y in [-.53,.53]:wheel(x,y)
 else:track(-.64);track(.64)
 box('armored lower hull',(0,0,.48),(1.22,1.67,.39),STEEL,.12)
 box('sloped glacis',(0,.52,.66),(1.1,.52,.22),EDGE,.06).rotation_euler.x=.15
 rivets(.76)
 for x in [-.35,.35]:box('rear engine grill',(x,-.62,.70),(.23,.3,.04),DARK,.009)
 cyl('turret ring',(0,-.05,.79),.40,.12,DARK,24)
 box('turret upper armor',(0,-.05,.97),(.81,.73,.35),STEEL,.10)
 if kind=='missile':missiles(z=1.16)
 elif kind=='mortar':beam_between('howitzer',(0,.10,1.08),(0,1.02,1.38),.11,EDGE)
 else:barrel(z=1.08,length=.88 if tier<4 else 1.16,kind=kind)
 beam_between('communications whip',(.36,-.56,.73),(.36,-.56,1.45),.015,DARK)
 for x in [-.43,.43]:box('headlamp',(x,.847,.53),(.14,.025,.06),TEAL,.01)
 if tier>=4:
  for x in [-.47,.47]:box('advanced powerpack',(x,-.15,.86),(.20,.83,.22),BLUE if kind=='energy' else GOLD,.035)
def aircraft(kind='fighter',tier=3):
 if kind in ['micro-drone','distributed-array']:
  box('drone core',(0,0,.65),(.53,.73,.35),STEEL,.08);sphere('recon camera',(0,.41,.67),(.16,.12,.12),TEAL)
  for x,y in [(-.65,-.5),(.65,-.5),(-.65,.5),(.65,.5)]:
   beam_between('rotor strut',(0,0,.7),(x,y,.7),.055,EDGE);cyl('rotor hub',(x,y,.72),.10,.1,GOLD);box('rotor',(x,y,.8),(.58,.07,.025),DARK,.008)
  return
 if kind=='cargo-aircraft':
  box('wide pressurized cargo hold',(0,-.05,.69),(.77,1.68,.64),STEEL,.17)
  sphere('rounded flight deck',(0,.88,.69),(.38,.43,.32),EDGE)
  box('cockpit windshield',(0,1.095,.87),(.53,.19,.15),GLASS,.045)
  box('reinforced cargo belly',(0,-.08,.40),(.67,1.65,.16),DARK,.05)
  for sign in [-1,1]:
   vertices=[(sign*.21,.49,.94),(sign*1.36,.15,.94),(sign*1.30,-.18,.94),(sign*.25,-.30,.94)]
   mesh=bpy.data.meshes.new('high transport wing mesh');mesh.from_pydata(vertices,[],[(0,1,2,3)]);ob=bpy.data.objects.new('large high-mounted transport wing',mesh);bpy.context.collection.objects.link(ob);finish(ob,'large high-mounted transport wing',EDGE,0);solid=ob.modifiers.new('thick transport airfoil','SOLIDIFY');solid.thickness=.075
   beam_between('wing gold leading edge',vertices[0],vertices[1],.025,GOLD)
   for distance in [.62,1.10]:
    x=sign*distance;sphere('turboprop engine nacelle',(x,.36,.79),(.14,.30,.16),STEEL)
    cyl('propeller spinner',(x,.70,.79),.08,.15,GOLD,16,(math.pi/2,0,0))
    for a in [0,math.pi/2]:
     prop=box('transport propeller blade',(x,.75,.79),(.49,.025,.045),DARK,.007);prop.rotation_euler.y=a+.20
   box('wide horizontal tailplane',(sign*.46,-.94,.82),(.90,.32,.065),EDGE,.02)
   box('retracted main undercarriage fairing',(sign*.37,-.32,.41),(.18,.76,.18),DARK,.06)
  box('single tall vertical stabilizer',(0,-.94,1.08),(.095,.50,.63),STEEL,.06)
  box('tail identification brass strip',(0,-1.16,1.12),(.105,.04,.30),GOLD,.008)
  box('rear cargo ramp door',(0,-.925,.59),(.59,.055,.40),EDGE,.025)
  for x in [-.25,0,.25]:box('ramp reinforcement rib',(x,-.961,.59),(.027,.018,.33),DARK,.004)
  for y in [-.58,-.15,.28]:box('cargo hatch roof rib',(0,y,1.025),(.49,.045,.032),GOLD,.009)
  return
 sphere('streamlined fuselage',(0,0,.65),(.29,1.20,.25),STEEL)
 sphere('cockpit',(0,.37,.86),(.20,.45,.13),GLASS)
 # swept solid wing mesh, with thickness and bevel
 for sign in [-1,1]:
  vertices=[(sign*.16,.35,.63),(sign*1.17,-.46,.63),(sign*.97,-.77,.63),(sign*.13,-.30,.63)]
  mesh=bpy.data.meshes.new('swept wing mesh');mesh.from_pydata(vertices,[],[(0,1,2,3)]);ob=bpy.data.objects.new('swept main wing',mesh);bpy.context.collection.objects.link(ob);finish(ob,'swept main wing',EDGE,0)
  solid=ob.modifiers.new('wing thickness','SOLIDIFY');solid.thickness=.07
  beam_between('wing edge accent',vertices[0],vertices[1],.025,GOLD)
  box('tail stabilizer',(sign*.36,-.89,.73),(.08,.46,.39),STEEL,.04).rotation_euler.y=sign*.30
  if kind!='stealth-wing':beam_between('underwing missile',(sign*.66,-.15,.49),(sign*.66,.54,.49),.07,ORANGE)
  cyl('exhaust',(sign*.15,-1.05,.64),.115,.25,BLUE if tier>=4 else ORANGE,20,(math.pi/2,0,0))
 if tier>=5:
  for sign in [-1,1]:box('aerospace ion pod',(sign*.7,-.42,.72),(.19,.72,.20),BLUE,.06)
def truck(kind='cargo'):
 for x in [-.63,.63]:
  for y in [-.65,0,.65]:wheel(x,y)
 box('truck chassis',(0,0,.50),(1.15,1.96,.25),DARK,.06)
 box('cab',(0,.62,.82),(1.1,.63,.56),STEEL,.08);panel((0,.948,.89),.76,.25)
 if kind=='missile':missiles(z=.90)
 else:
  box('armored cargo container',(0,-.43,.91),(1.14,1.20,.67),STEEL,.045)
  for y in [-.85,-.55,-.25]:box('container reinforcement',(0,y,1.26),(1.18,.055,.04),EDGE,.009)
  panel((0,-1.047,.99),.65,.25)
def software_badge(id):
 source=PROJECT/('references/software/vscode-icons/visual-studio-code-icons/vscode.png' if id=='vscode' else 'references/software/pycharm.png')
 brand=MODELS/'branding';brand.mkdir(exist_ok=True);target=brand/(id+'.png');shutil.copy2(source,target)
 mat=bpy.data.materials.new(id+' actual software brand');mat.use_nodes=True
 nodes=mat.node_tree.nodes;bs=nodes.get('Principled BSDF');tex=nodes.new('ShaderNodeTexImage');tex.image=bpy.data.images.load(str(target),check_existing=True);tex.image.pack()
 mat.node_tree.links.new(tex.outputs['Color'],bs.inputs['Base Color']);mat.node_tree.links.new(tex.outputs['Alpha'],bs.inputs['Alpha']);bs.inputs['Roughness'].default_value=.5
 for side,center,u_axis in [('front',(.28,.441,.71),(-1,0,0)),('left',(-.516,-.03,.75),(0,-1,0)),('right',(.516,-.03,.75),(0,1,0))]:
  c=Vector(center);u=Vector(u_axis)*.12;v=Vector((0,0,.12));vertices=[c-u-v,c+u-v,c+u+v,c-u+v]
  mesh=bpy.data.meshes.new(id+' brand plate '+side);mesh.from_pydata(vertices,[],[(0,1,2,3)])
  ob=bpy.data.objects.new(id+' unmodified official icon '+side,mesh);bpy.context.collection.objects.link(ob);mesh.materials.append(mat)
  uv=mesh.uv_layers.new(name='UVMap');coords=[(0,0),(1,0),(1,1),(0,1)]
  for loop in mesh.loops:uv.data[loop.index].uv=coords[loop.vertex_index]
  CURRENT.append(ob)
def module(kind):
 if kind.startswith('plugin-'):
  _,branch,slot=kind.split('-');color={'speed':ORANGE,'security':BLUE,'algorithm':TEAL,'science':BLUE,'lightweight':GOLD}[branch]
  box('plugin circuit substrate',(0,0,.1),(.94,.80,.15),DARK,.035)
  for x in [-.48,.48]:
   for y in [-.27,-.09,.09,.27]:box('gold edge connector',(x,y,.1),(.14,.07,.08),GOLD,.006)
  for y in [-.41,.41]:
   for x in [-.3,-.1,.1,.3]:box('gold edge connector',(x,y,.1),(.08,.12,.08),GOLD,.006)
  box('processor block',(0,0,.26),(.44,.40,.20),color,.045)
  if slot=='core':
   for x in [-.15,-.075,0,.075,.15]:box('heatsink fin',(x,0,.44),(.035,.35,.22),EDGE,.008)
  elif slot=='attack':
   for x in [-.13,.13]:cyl('overdrive capacitor',(x,0,.49),.085,.35,EDGE);sphere('charged cap',(x,0,.69),(.072,.072,.04),color)
  else:cyl('support crystal ring',(0,0,.46),.26,.08,EDGE);cone('support crystal',(0,0,.67),.14,0,.38,color,6)
 elif kind in ['floor','foundation','roof','roof-corner']:
  box(kind+' slab',(0,0,.06),(1.0,1.0,.12),CONCRETE if kind=='foundation' else STEEL,.015)
  for x in [-.47,.47]:box('floor brass rim',(x,0,.13),(.025,1.,.02),GOLD,.004)
  for y in [-.47,.47]:box('floor joint',(0,y,.13),(1.,.025,.02),EDGE,.004)
  if kind=='roof':box('roof vent',(0,0,.17),(.35,.48,.10),DARK,.01)
 elif kind=='moat':
  box('open data trench bed',(0,0,-.065),(1.,.44,.07),DARK,.012)
  for y in [-.22,.22]:
   box('low retaining channel wall',(0,y,.025),(1.,.085,.22),STEEL,.018)
   box('channel brass coping',(0,y,.146),(1.,.094,.023),GOLD,.006)
   box('retaining wall reinforcement',(0,y*1.19,.03),(.72,.03,.07),EDGE,.009)
  box('recessed data conductor',(0,0,-.018),(.98,.28,.021),GLASS,.003)
  for x in [-.4,-.2,0,.2,.4]:box('submerged topology conductor',(x,0,.004),(.05,.245,.018),TEAL,.004)
 elif kind=='door':
  for sign in [-1,1]:
   box('armored door jamb',(sign*.415,0,.65),(.17,.25,1.30),EDGE,.025)
   box('hinged door leaf left' if sign<0 else 'hinged door leaf right',(sign*.15,-.08,.56),(.30,.07,1.05),STEEL,.018)
   box('door leaf reinforcement left' if sign<0 else 'door leaf reinforcement right',(sign*.15,-.12,.61),(.24,.025,.09),GOLD,.007)
   cyl('visible door hinge',(sign*.31,-.09,.68),.04,.74,DARK,12)
  box('door lintel',(0,0,1.235),(1.,.25,.17),EDGE,.02)
  box('access status indicator',(.414,-.14,.76),(.035,.018,.17),TEAL,.007)
 elif kind in ['wall','window-wall','cuda-wall','physical-wall']:
  box('reinforced panel',(0,0,.65),(1.,.18,1.3),STEEL,.04)
  for x in [-.46,.46]:box('vertical edge column',(x,0,.66),(.11,.27,1.34),EDGE,.02)
  if kind=='window-wall':box('armored glass',(0,-.101,.83),(.71,.028,.52),GLASS,.018)
  elif kind=='cuda-wall':panel((0,-.101,.75),.69,.65)
  else:box('center reinforcement',(0,-.105,.65),(.85,.04,.14),GOLD,.01)
 elif kind=='column':box('load bearing pillar',(0,0,.72),(.28,.28,1.44),CONCRETE,.025);box('column cap',(0,0,1.42),(.43,.43,.12),EDGE,.02)
 elif kind=='stairs':
  for i in range(8):box('stair tread',(0,-.5+i*.14,.075+i*.15),(.86,.14,.15),CONCRETE,.012)
  for x in [-.44,.44]:beam_between('stairs rail',(x,-.50,.4),(x,.57,1.6),.035,GOLD)
 elif kind=='elevator':
  box('lift platform',(0,0,.08),(1.,1.,.16),STEEL,.02)
  for x in [-.43,.43]:
   for y in [-.43,.43]:box('lift shaft rail',(x,y,.81),(.07,.07,1.62),EDGE,.01)
  box('lift carriage',(0,0,.78),(.76,.76,.13),GOLD,.02)
 elif kind in ['ramp','bridge']:
  if kind=='ramp':
   ob=box('vehicle access ramp',(0,0,.5),(1.5,2.2,.14),CONCRETE,.015);ob.rotation_euler.x=math.radians(24)
   for x in [-.74,.74]:beam_between('ramp guide',(x,-1.05,.18),(x,1.05,1.1),.045,GOLD)
  else:
   box('bridge deck',(0,0,.28),(1.4,2.2,.25),CONCRETE,.03)
   for x in [-.69,.69]:box('bridge parapet',(x,0,.50),(.12,2.2,.30),EDGE,.025)
 elif kind=='rack':
  box('server cabinet',(0,0,.70),(.55,.48,1.4),DARK,.028)
  for z in [.18,.4,.62,.84,1.06,1.28]:
   box('GPU blade',(0,-.25,z),(.49,.04,.15),EDGE,.01)
   for x in [-.16,0,.16]:box('GPU status LED',(x,-.276,z),(.075,.012,.019),TEAL,.002)
 elif kind=='research-console':
  box('lab desk',(0,0,.58),(.90,.66,.15),EDGE,.035);box('desk pedestal',(0,0,.26),(.46,.46,.50),STEEL,.035)
  box('scientific monitor',(0,.1,.98),(.78,.07,.6),DARK,.025);panel((0,.052,.98),.70,.48)
 elif kind=='depot':
  for x in [-.3,.3]:
   for y in [-.25,.25]:
    box('supply crate',(x,y,.3),(.52,.44,.55),STEEL,.04);box('crate band',(x,y,.59),(.09,.46,.025),GOLD,.004)
 elif kind=='generator':
  box('generator skid',(0,0,.13),(1.2,1.,.22),CONCRETE,.035);cyl('turbine housing',(0,0,.63),.37,.90,STEEL,24,(math.pi/2,0,0));cyl('turbine end',(0,-.47,.63),.31,.06,GOLD,24,(math.pi/2,0,0));panel((0,-.507,.64),.28,.21)
 elif kind=='wind-power':
  cone('wind tower',(0,0,.84),.12,.06,1.6,EDGE);sphere('nacelle',(0,0,1.66),(.15,.24,.12),STEEL)
  for a in [0,120,240]:
   ang=math.radians(a);beam_between('wind rotor',(0,-.25,1.66),(.78*math.cos(ang),-.25,1.66+.78*math.sin(ang)),.045,EDGE)
 elif kind=='hydro-power':
  box('hydro intake',(0,0,.4),(1.4,.8,.75),CONCRETE,.04);cyl('water turbine',(0,-.45,.45),.35,.15,TEAL,16,(math.pi/2,0,0));box('water spill',(0,-.70,.08),(.9,.50,.10),BLUE,.012)
  for a in range(0,360,60):
   angle=math.radians(a);beam_between('hydro rotor spoke',(0,-.54,.45),(.29*math.cos(angle),-.54,.45+.29*math.sin(angle)),.034,GOLD)
  cyl('hydro rotor hub',(0,-.56,.45),.08,.05,EDGE,16,(math.pi/2,0,0))
  sphere('hydro rotor phase marker',(.24,-.585,.45),(.035,.020,.035),ORANGE)
  cyl('hydro generator rotor',(.4,0,.83),.16,.09,GOLD,16)
  sphere('hydro generator rotor marker',(.52,0,.90),(.045,.035,.025),ORANGE)
 elif kind in ['coal-power','nuclear-power']:
  box('reactor hall',(0,0,.36),(1.35,1.25,.7),STEEL,.075)
  if kind=='nuclear-power':
   for x in [-.38,.38]:cone('cooling tower',(x,0,1.0),.31,.23,.85,CONCRETE,24);cyl('tower opening',(x,0,1.435),.17,.04,DARK,24)
  else:
   for x in [-.37,.37]:cyl('exhaust stack',(x,.23,1.08),.12,1.08,CONCRETE,16);cyl('safety stripe',(x,.23,1.42),.125,.10,GOLD,16)
 elif kind=='extractor':
  box('mining rig base',(0,0,.18),(1.1,1.,.32),STEEL,.05);cone('drill',(0,0,.65),.21,.08,.8,EDGE)
  for x in [-.42,.42]:beam_between('derrick',(x,0,.30),(x*.4,0,1.36),.07,GOLD)
  beam_between('crossbrace',(-.42,0,.70),(.2,0,1.32),.035,EDGE)
 elif kind=='mobile-relay':truck();beam_between('relay mast',(0,-.40,1.1),(0,-.40,2.05),.04,EDGE);sphere('relay radome',(0,-.40,2.15),(.25,.25,.17),TEAL)
 elif kind=='factory':
  box('robot machine base',(0,0,.17),(1.,1.,.32),STEEL,.04);beam_between('robot shoulder',(0,0,.4),(.24,0,1.05),.11,GOLD);beam_between('robot forearm',(.24,0,1.05),(0,.5,.91),.085,EDGE);box('tool claw',(0,.56,.86),(.28,.12,.21),DARK,.025)
 elif kind=='airfield':
  box('runway foundation',(0,0,.08),(2.1,2.5,.16),CONCRETE,.02);box('runway surface',(0,0,.18),(1.0,2.4,.025),DARK,.005)
  for y in [-.9,-.45,0,.45,.9]:box('runway center stripe',(0,y,.20),(.06,.22,.014),EDGE,.002)
  for x in [-.75,.75]:
   for y in [-.90,-.45,0,.45,.90]:sphere('runway beacon',(x,y,.24),(.035,.035,.055),TEAL)
  box('air control tower',(.80,.80,.60),(.42,.42,1.0),STEEL,.04);sphere('control radar',(.8,.8,1.22),(.30,.30,.20),GLASS)
 elif kind=='missile-silo':
  box('silo foundation',(0,0,.1),(1.6,1.6,.20),CONCRETE,.025);cyl('silo armored hatch',(0,0,.27),.57,.18,STEEL,24)
  for x in [-.38,.38]:box('hatch hinge',(x,0,.4),(.12,.96,.10),GOLD,.02)
  panel((0,-.82,.23),.70,.20)
 elif kind=='ammunition-workshop':
  box('ammo fabrication bench',(0,0,.42),(1.2,.65,.20),STEEL,.04)
  for x in [-.4,-.2,0,.2,.4]:cyl('shell casing',(x,0,.75),.065,.45,GOLD);cone('shell tip',(x,0,1.02),.065,0,.10,EDGE)
  for x in [-.48,.48]:box('bench support',(x,0,.20),(.15,.52,.42),EDGE,.02)
 elif kind=='network-defense':
  box('network appliance',(0,0,.46),(.85,.6,.80),STEEL,.06);panel((0,-.32,.50),.67,.54)
  for x in [-.30,.30]:beam_between('network antenna',(x,0,.84),(x,0,1.36),.022,EDGE);sphere('antenna node',(x,0,1.38),(.075,.075,.075),TEAL)
 elif kind=='energy-defense':
  cyl('shield powerbase',(0,0,.18),.55,.3,STEEL,16)
  for a in [0,120,240]:
   x=math.cos(math.radians(a))*.4;y=math.sin(math.radians(a))*.4
   beam_between('shield emitter tower',(x,y,.25),(x,y,1.15),.085,EDGE);sphere('shield emitter',(x,y,1.2),(.13,.13,.18),BLUE)
  sphere('energy core',(0,0,.65),(.19,.19,.29),TEAL)
 elif kind=='repair-bay':
  box('maintenance pad',(0,0,.08),(1.4,1.7,.15),CONCRETE,.02)
  for x in [-.48,.48]:
   beam_between('repair arm',(x,0,.18),(x,0,.94),.07,GOLD);beam_between('repair tool',(x,0,.94),(x*.3,.22,.94),.06,EDGE)
  box('service locker',(.55,-.63,.38),(.25,.34,.55),STEEL,.03);panel((.55,-.81,.40),.18,.31)
 elif kind=='orbital-control':
  module('research-console');beam_between('dish mast',(0,.42,.1),(0,.42,1.4),.065,EDGE)
  dish=sphere('orbital command dish',(0,.42,1.6),(.42,.42,.12),GLASS);dish.rotation_euler.x=.4
  beam_between('dish feed',(0,.42,1.6),(.14,.2,1.9),.025,GOLD)
 elif kind in ['drainage-pump','drilling-rig']:
  if kind=='drilling-rig':module('extractor')
  else:
   module('generator');beam_between('drainage pipe',(.4,0,.7),(.8,.4,.7),.12,BLUE);beam_between('intake pipe',(.8,.4,.7),(.8,.4,.1),.12,EDGE)
 elif kind in ['ore-node','coal-node']:
  mineral=material('Exposed mineral crystal' if kind=='ore-node' else 'Rough carbon seam',(.24,.39,.53) if kind=='ore-node' else (.045,.052,.062),.38 if kind=='ore-node' else .05,.12 if kind=='ore-node' else 0)
  cone('exposed broken seam',(0,0,.055),.96,.81,.11,DARK,11)
  for i,(x,y,s) in enumerate([(-.52,-.40,.25),(-.08,-.52,.31),(.43,-.38,.29),(-.53,.08,.34),(0,0,.48),(.52,.09,.31),(-.31,.49,.30),(.22,.48,.36),(.62,.48,.20)]):
   bpy.ops.mesh.primitive_ico_sphere_add(subdivisions=1,radius=1,location=(x,y,s*.58+.04));ob=bpy.context.object
   ob.scale=(s*(1.15 if i%2 else .90),s,s*(1.1 if kind=='ore-node' else .76));ob.rotation_euler=(i*.17,i*.11,i*.73)
   finish(ob,'individual exposed ore crystal' if kind=='ore-node' else 'individual rough coal fragment',mineral,0)
  if kind=='ore-node':
   for x,y in [(-.25,.15),(.33,-.12),(.09,.41)]:
    crystal=cone('raw rare mineral prism',(x,y,.58),.12,.025,.64,TEAL,5);crystal.rotation_euler.x=.15;crystal.rotation_euler.y=x*.6
 elif kind=='strategic-node':
  cone('neutral capture platform',(0,0,.075),.93,.93,.15,CONCRETE,12)
  cyl('capture terminal footing',(0,0,.19),.67,.12,STEEL,24)
  for a in range(0,360,45):
   r=math.radians(a);marker=box('capture zone brass marker',(math.cos(r)*.79,math.sin(r)*.79,.16),(.16,.05,.03),GOLD,.008);marker.rotation_euler.z=r+math.pi/2
  box('neutral uplink cabinet',(0,0,.67),(.66,.59,.96),STEEL,.06)
  panel((0,-.31,.65),.47,.43)
  for x in [-.34,.34]:box('cabinet side armor',(x,0,.66),(.08,.54,.80),EDGE,.025)
  cyl('beacon mast',(0,.04,1.28),.08,.40,EDGE,16)
  sphere('neutral amber capture beacon',(0,.04,1.51),(.16,.16,.13),ORANGE)
  for x in [-.40,.40]:
   beam_between('communication fork',(x*.5,.14,.96),(x,.14,1.35),.045,EDGE)
   cyl('uplink receive node',(x,.14,1.36),.055,.14,GOLD,12)
  box('access step',(0,-.70,.14),(.72,.26,.17),EDGE,.02)
 elif kind=='command-core':
  box('command dais',(0,0,.17),(1.5,1.5,.30),CONCRETE,.06)
  for x in [-.45,.45]:
   for y in [-.45,.45]:box('command tower',(x,y,.72),(.34,.34,1.2),STEEL,.06);box('core light',(x,y-.18,.90),(.2,.025,.40),TEAL,.016)
  sphere('command hologram',(0,0,1.0),(.27,.27,.38),TEAL)
 elif kind=='radar-array':
  box('radar console',(0,0,.34),(.88,.60,.58),STEEL,.06);panel((0,-.32,.38),.67,.36)
  beam_between('radar pedestal',(0,.10,.56),(0,.10,1.10),.08,EDGE)
  dish=sphere('phased radar face',(0,.10,1.32),(.46,.07,.40),GLASS)
  for x in [-.30,-.15,0,.15,.30]:beam_between('radar grid',(x,.015,1.04),(x,.015,1.59),.012,TEAL)
 elif kind=='logistics-belt':
  box('cargo transfer frame',(0,0,.33),(1.05,1.48,.44),STEEL,.03)
  for y in [-.60,-.40,-.20,0,.20,.40,.60]:cyl('conveyor roller',(0,y,.61),.09,.96,EDGE,12,(0,math.pi/2,0))
  box('cargo in transit',(0,.22,.88),(.62,.60,.48),GOLD,.035)
  for x in [-.54,.54]:beam_between('cargo guide',(x,-.69,.77),(x,.69,.77),.025,TEAL)
 elif kind=='secure-switch':
  box('secured network vault',(0,0,.70),(.90,.68,1.36),STEEL,.065)
  for z in [.28,.49,.70,.91,1.12]:box('switch blade',(0,-.36,z),(.74,.035,.14),DARK,.008)
  for x in [-.3,-.15,0,.15,.3]:box('encrypted link port',(x,-.39,.90),(.08,.028,.055),BLUE,.009)
  box('tamper lock',(0,-.395,.51),(.23,.035,.25),GOLD,.035)
 elif kind=='fire-control':
  cone('fire control tripod',(0,0,.31),.42,.16,.6,STEEL,8)
  box('rangefinder head',(0,0,.92),(1.0,.54,.38),EDGE,.08)
  for x in [-.30,.30]:cyl('rangefinder optic',(x,.31,.96),.14,.25,DARK,20,(math.pi/2,0,0));cyl('rangefinder lens',(x,.45,.96),.12,.025,TEAL,20,(math.pi/2,0,0))
  sphere('tracking camera',(0,0,1.20),(.13,.16,.12),GLASS)
 elif kind=='particle-foundry':
  box('particle furnace base',(0,0,.15),(1.1,1.1,.27),STEEL,.04)
  cyl('containment chamber',(0,0,.73),.30,1.02,GLASS,24)
  for z in [.34,.61,.88,1.15]:cyl('accelerator coil',(0,0,z),.37,.08,GOLD,24)
  sphere('suspended plasma',(0,0,.8),(.18,.18,.44),BLUE)
  for x in [-.5,.5]:beam_between('power conduit',(x,0,.15),(x,0,1.18),.055,EDGE)
 elif kind=='modular-workshop':
  box('modular assembly table',(0,0,.62),(1.20,.88,.18),EDGE,.035)
  for x in [-.45,.45]:box('table leg',(x,0,.30),(.16,.66,.58),STEEL,.02)
  for x,y in [(-.32,-.15),(0,.15),(.32,-.15)]:box('swappable module',(x,y,.78),(.24,.23,.15),GOLD,.018)
  beam_between('precision arm',(.5,.30,.72),(.5,.30,1.14),.04,TEAL);beam_between('assembly probe',(.5,.3,1.14),(0,0,1.1),.035,EDGE)
 elif kind=='launchpad':
  box('launch foundation',(0,0,.11),(2.,2.,.22),CONCRETE,.045);cyl('launch mount',(0,0,.35),.55,.26,STEEL,16)
  cyl('launch vehicle',(0,0,1.01),.18,1.18,EDGE,20);cone('launch nose',(0,0,1.75),.18,0,.35,GOLD,20)
  for x in [-.75,.75]:
   beam_between('launch gantry',(x,.60,.22),(x,.60,1.70),.055,STEEL)
   beam_between('gantry cross brace',(x,.60,.25),(-x,.60,1.68),.025,GOLD)
  beam_between('gantry upper bridge',(-.75,.60,1.70),(.75,.60,1.70),.05,EDGE)
 else:turret('energy',3)
def look(ob,point):ob.rotation_euler=(Vector(point)-ob.location).to_track_quat('-Z','Y').to_euler()
bpy.ops.object.camera_add(location=(6,-6,5.54898));camera=bpy.context.object;camera.data.type='ORTHO';camera.data.ortho_scale=3.6;look(camera,(0,0,.65));scene.camera=camera
for name,loc,power,size,col in [('key',(-3,-4,7),800,5,(1,.90,.75)),('fill',(4,1,5),650,4,(.55,.82,1))]:
 bpy.ops.object.light_add(type='AREA',location=loc);o=bpy.context.object;o.name=name;o.data.energy=power;o.data.shape='DISK';o.data.size=size;o.data.color=col;look(o,(0,0,.5))
UNITS={
 'vscode':('turret','kinetic',1),'pycharm':('turret','mortar',1),'autocannon':('turret','kinetic',1),'interceptor':('turret','kinetic',1),'mortar':('turret','mortar',1),'pulse-turret':('turret','energy',1),
 'light-tank':('tank','kinetic',2),'shield-tank':('tank','kinetic',2),'artillery':('tank','mortar',2),'laser-tank':('tank','energy',2),'scout-buggy':('buggy','kinetic',1),
 'fighter':('air','fighter',3),'aa-turret':('turret','missile',2),'missile-truck':('truck','missile',3),'plasma-cannon':('turret','energy',3),'micro-drone':('air','micro-drone',2),
 'rail-tank':('tank','energy',4),'fortress-tank':('tank','mortar',4),'siege-launcher':('turret','missile',4),'particle-cannon':('turret','energy',4),'loiter-drone':('air','loiter-drone',3),
 'aerospace-fighter':('air','fighter',5),'aegis-array':('turret','orbital',5),'precision-strike':('turret','missile',5),'orbital-lance':('turret','orbital',5),'distributed-array':('air','distributed-array',5),'stealth-wing':('air','stealth-wing',4),
 'cargo-truck':('truck','cargo',1),'cargo-aircraft':('air','cargo-aircraft',2),'breach-tank':('tank','mortar',2),'anti-air':('turret','missile',2),'engineer-drone':('air','micro-drone',1),'resource-hauler':('truck','cargo',1)}
MODULES=['foundation','floor','roof','roof-corner','wall','window-wall','column','stairs','elevator','door','cuda-wall','physical-wall','rack','research-console','depot','generator','wind-power','hydro-power','coal-power','nuclear-power','extractor','mobile-relay','factory','network-defense','energy-defense','repair-bay','orbital-control','ramp','bridge','airfield','missile-silo','ammunition-workshop','drainage-pump','drilling-rig','command-core','radar-array','logistics-belt','secure-switch','fire-control','particle-foundry','modular-workshop','launchpad']+[f'plugin-{branch}-{slot}' for branch in ['speed','security','algorithm','science','lightweight'] for slot in ['core','attack','support']]
MODULES.extend(['strategic-node','ore-node','coal-node','moat'])
args=sys.argv[sys.argv.index('--')+1:] if '--' in sys.argv else [];only=args[args.index('--only')+1] if '--only' in args else None
index=[]
for id in list(UNITS)+MODULES:
 if only and id not in only.split(','):continue
 dest=OUT/id
 if (dest/'bake.json').exists() and '--force' not in args:index.append(json.loads((dest/'bake.json').read_text()));continue
 for ob in list(CURRENT):bpy.data.objects.remove(ob,do_unlink=True)
 CURRENT.clear()
 category='module'
 if id in UNITS:
  category,kind,tier=UNITS[id]
  if category=='turret':turret(kind,tier)
  elif category=='tank':tank(kind,tier)
  elif category=='buggy':tank(kind,tier,True)
  elif category=='air':aircraft(kind,tier)
  else:truck(kind)
 else:module(id)
 if id in ['vscode','pycharm']:software_badge(id)
 if not any(ob.type=='MESH' for ob in CURRENT):raise RuntimeError('Refusing to publish empty Blender geometry for '+id)
 root=bpy.data.objects.new(id+' assembly',None);bpy.context.collection.objects.link(root)
 for ob in CURRENT:ob.parent=root
 CURRENT.append(root)
 dest.mkdir(parents=True,exist_ok=True)
 bpy.ops.object.select_all(action='DESELECT')
 for ob in CURRENT:ob.select_set(True)
 bpy.context.view_layer.objects.active=root
 bpy.ops.export_scene.gltf(filepath=str(MODELS/(id+'.glb')),export_format='GLB',use_selection=True)
 bpy.ops.wm.save_as_mainfile(filepath=str(MODELS/(id+'.blend')),check_existing=False)
 for i,d in enumerate(DIRS):
  root.rotation_euler.z=math.radians(-135-i*45)
  scene.render.filepath=str(dest/(d+'.png'));bpy.ops.render.render(write_still=True)
 info={'id':id,'category':category,'directions':DIRS,'frameSize':[384,384],'pivot':[.5,.70],'sourceModel':str((MODELS/(id+'.blend')).relative_to(PROJECT)).replace('\\','/'),'glb':str((MODELS/(id+'.glb')).relative_to(PROJECT)).replace('\\','/'),'method':'Blender Cycles CPU render from authored 3D geometry','samples':12,'camera':'orthographic 30-degree isometric','status':'rendered-needs-review'}
 if id in ['vscode','pycharm']:info['softwareBrand']={'reference':f'Content/Models/v6/branding/{id}.png','sha256':hashlib.sha256((MODELS/'branding'/(id+'.png')).read_bytes()).hexdigest(),'source':'Previously verified Microsoft official branding bundle' if id=='vscode' else 'Previously verified JetBrains official brand PNG','bitmapModified':False}
 (dest/'bake.json').write_text(json.dumps(info,indent=2),encoding='utf-8');index.append(info)
 print('V6_BAKED '+id,flush=True)
(OUT/'index.json').write_text(json.dumps(index,indent=2),encoding='utf-8')
