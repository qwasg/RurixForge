// Test-only GPU sampler probe; the production functions are included verbatim.
@group(0) @binding(0) var<storage,read_write> samples:array<vec4<f32>>;
struct Probe {rough:f32,nv:f32,count:u32,seed:u32};var<push_constant> probe:Probe;
@compute @workgroup_size(64,1,1) fn main(@builtin(global_invocation_id) id:vec3<u32>){
 if(id.x>=probe.count){return;}let n=vec3<f32>(0.,0.,1.);let v=vec3<f32>(sqrt(1.-probe.nv*probe.nv),0.,probe.nv);let uv=vec2<f32>(random(id.x*3u+probe.seed),random(id.x*3u+probe.seed+1u));
 var h=ggx_visible_half(n,v,probe.rough,uv);if(NDF_RED){let alpha=probe.rough*probe.rough;let cosine=sqrt((1.-uv.y)/(1.+(alpha*alpha-1.)*uv.y));let sine=sqrt(max(0.,1.-cosine*cosine));h=vec3<f32>(sine*cos(6.2831853*uv.x),sine*sin(6.2831853*uv.x),cosine);}
 let l=reflect(-v,h);let weight=ggx_visible_weight(n,v,l,h,vec3<f32>(1.),1.,probe.rough);samples[id.x]=vec4<f32>(h,weight.x);
}
