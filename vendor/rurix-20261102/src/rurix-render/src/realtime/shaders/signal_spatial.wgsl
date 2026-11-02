struct SignalHistory {diffuse:vec4<f32>,specular:vec4<f32>,position:vec4<f32>,normal:vec4<f32>,moments:vec4<f32>};
@group(0) @binding(0) var<storage,read> history:array<SignalHistory>;
@group(0) @binding(1) var diffuse:texture_storage_2d<rgba16float,read>;
@group(0) @binding(2) var specular:texture_storage_2d<rgba16float,read>;
@group(0) @binding(3) var out_diffuse:texture_storage_2d<rgba16float,write>;
@group(0) @binding(4) var out_specular:texture_storage_2d<rgba16float,write>;
@group(0) @binding(5) var<uniform> frame:Frame;
struct Control {step:u32};var<push_constant> control:Control;
fn luma(c:vec3<f32>)->f32{return dot(c,vec3<f32>(0.2126,0.7152,0.0722));}
@compute @workgroup_size(8,8,1) fn main(@builtin(global_invocation_id) id:vec3<u32>){
 let half=(vec2<u32>(frame.extent.xy)+vec2<u32>(1u))/2u;if(any(id.xy>=half)){return;}let p=vec2<i32>(id.xy);let center=history[id.y*half.x+id.x];let cd=textureLoad(diffuse,p);let cs=textureLoad(specular,p);
 var sum_d=vec3<f32>(0.);var sum_s=vec3<f32>(0.);var wd_total=0.;var ws_total=0.;let variance_d=max(center.moments.y-center.moments.x*center.moments.x,0.);let variance_s=max(center.moments.w-center.moments.z*center.moments.z,0.);
 for(var y=-2;y<=2;y++){for(var x=-2;x<=2;x++){let q=clamp(p+vec2<i32>(x,y)*i32(control.step),vec2<i32>(0),vec2<i32>(half)-vec2<i32>(1));let h=history[u32(q.y)*half.x+u32(q.x)];if(h.position.w!=center.position.w){continue;}
 let plane_distance=abs(dot(h.position.xyz-center.position.xyz,center.normal.xyz));let geometric=pow(max(dot(h.normal.xyz,center.normal.xyz),0.),32.)*exp(-plane_distance/0.04)*exp(-f32(x*x+y*y)/4.);
 let d=textureLoad(diffuse,q);let s=textureLoad(specular,q);let wd=geometric/(1.+abs(luma(d.rgb)-luma(cd.rgb))/max(0.08,sqrt(variance_d)*3.));
 var ws=geometric*exp(-abs(h.normal.w-center.normal.w)*20.)/(1.+abs(luma(s.rgb)-luma(cs.rgb))/max(0.12,sqrt(variance_s)*3.));
 if(center.normal.w<0.10){ws*=exp(-abs(abs(s.w)-abs(cs.w))/max(0.5,0.15*abs(cs.w)));}
 sum_d+=d.rgb*wd;wd_total+=wd;sum_s+=s.rgb*ws;ws_total+=ws;
 }}var filtered_s=sum_s/max(ws_total,1e-8);if(control.step>=4u){let broadness=smoothstep(0.08,0.3,center.normal.w);filtered_s=mix(cs.rgb,filtered_s,broadness);}
 textureStore(out_diffuse,p,vec4<f32>(sum_d/max(wd_total,1e-8),cd.a));textureStore(out_specular,p,vec4<f32>(filtered_s,cs.a));
}
