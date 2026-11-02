@group(0) @binding(0) var<storage,read> lights:array<Light>;
@group(0) @binding(1) var<storage,read_write> masks:array<u32>;
@group(0) @binding(2) var gpos:texture_storage_2d<rgba32float,read>;
@group(0) @binding(3) var<uniform> frame:Frame;
var<workgroup> minimums:array<vec3<f32>,64>;
var<workgroup> maximums:array<vec3<f32>,64>;
@compute @workgroup_size(8,8,1) fn main(@builtin(global_invocation_id) id:vec3<u32>,@builtin(workgroup_id) group:vec3<u32>,@builtin(local_invocation_index) lane:u32){
 let tile=group.y*((u32(frame.extent.x)+7u)/8u)+group.x;
 if(frame.environment.w<0.5){if(lane==0u){for(var word=0u;word<8u;word++){masks[tile*8u+word]=0xffffffffu;}}return;}
 var lo=vec3<f32>(1e30);var hi=vec3<f32>(-1e30);if(all(id.xy<vec2<u32>(frame.extent.xy))){let p=textureLoad(gpos,vec2<i32>(id.xy));if(p.w>0.){lo=p.xyz;hi=p.xyz;}}
 minimums[lane]=lo;maximums[lane]=hi;workgroupBarrier();
 for(var stride=32u;stride>0u;stride=stride>>1u){if(lane<stride){minimums[lane]=min(minimums[lane],minimums[lane+stride]);maximums[lane]=max(maximums[lane],maximums[lane+stride]);}workgroupBarrier();}
 if(lane!=0u){return;}for(var word=0u;word<8u;word++){masks[tile*8u+word]=0u;}
 let lower=minimums[0];let upper=maximums[0];if(lower.x>upper.x){return;}
 for(var i=0u;i<u32(frame.features.x);i++){let light=lights[i];let center=light.pos_range.xyz;let delta=max(max(lower-center,center-upper),vec3<f32>(0.));
 // Inflate the radius for f32 subtraction/squared-distance error. This is a
 // conservative candidate list; exact range/cone/BRDF tests remain in shading.
 let scale=max(max(abs(center.x),max(abs(center.y),abs(center.z))),max(max(abs(lower.x),max(abs(lower.y),abs(lower.z))),max(abs(upper.x),max(abs(upper.y),abs(upper.z)))));
 let radius=light.pos_range.w+16.*1.1920928955078125e-7*(scale+light.pos_range.w+1.);
 if(dot(delta,delta)<=radius*radius&&!(DROP_FIRST_LIGHT&&i==0u)){let word=tile*8u+i/32u;masks[word]=masks[word]|(1u<<(i%32u));}}
}
