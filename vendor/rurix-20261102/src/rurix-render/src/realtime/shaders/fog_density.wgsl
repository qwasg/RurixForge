// Godot 4.7.2 FogMaterial: pow(clamp(-2*SDF/min(SIZE),0,1),edge_fade).
// Shape SDFs match environment/volumetric_fog.glsl (box/ellipsoid only).
// local is normalized by the volume half-size. The ellipsoid center uses its
// finite shortest-radius limit instead of evaluating the upstream 0/0.
fn fog_edge_weight(local:vec3<f32>,half_size:vec3<f32>,shape:f32,edge_fade:f32)->f32{
 if(edge_fade==0.){return 1.;}
 let minimum=min(half_size.x,min(half_size.y,half_size.z));
 let q=abs(local*half_size)-half_size;
 var sdf=length(max(q,vec3<f32>(0.)))+min(max(q.x,max(q.y,q.z)),0.);
 if(shape>0.5){let k0=length(local);let k1=length(local/half_size);sdf=-minimum;if(k1>1e-12){sdf=k0*(k0-1.)/k1;}}
 return pow(clamp(-sdf/minimum,0.,1.),edge_fade);
}
