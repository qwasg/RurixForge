struct SignalHistory {diffuse:vec4<f32>,specular:vec4<f32>,position:vec4<f32>,normal:vec4<f32>,moments:vec4<f32>};
@group(0) @binding(0) var<storage,read> old_history:array<SignalHistory>;
@group(0) @binding(1) var<storage,read_write> new_history:array<SignalHistory>;
@group(0) @binding(2) var diffuse:texture_storage_2d<rgba16float,read>;
@group(0) @binding(3) var specular:texture_storage_2d<rgba16float,read>;
@group(0) @binding(4) var gpos:texture_storage_2d<rgba32float,read>;
@group(0) @binding(5) var gnrm:texture_storage_2d<rgba16float,read>;
@group(0) @binding(6) var motion:texture_storage_2d<rgba16float,read>;
@group(0) @binding(7) var out_diffuse:texture_storage_2d<rgba16float,write>;
@group(0) @binding(8) var out_specular:texture_storage_2d<rgba16float,write>;
@group(0) @binding(9) var<uniform> frame:Frame;
fn luma(c:vec3<f32>)->f32{return dot(c,vec3<f32>(0.2126,0.7152,0.0722));}
fn radiance_changed(p:vec2<i32>,previous_pixel:vec2<i32>,identity:f32,normal:vec3<f32>,half:vec2<u32>,offset:vec2<u32>)->vec2<bool>{
 var total=vec2<f32>(0.);var squares=vec2<f32>(0.);var old_total=vec2<f32>(0.);var old_variance=vec2<f32>(0.);var count=0.;
 for(var y=-1;y<=1;y++){for(var x=-1;x<=1;x++){let q=p+vec2<i32>(x,y);let old_q=previous_pixel+vec2<i32>(x,y);if(any(q<vec2<i32>(0))||any(q>=vec2<i32>(half))||any(old_q<vec2<i32>(0))||any(old_q>=vec2<i32>(half))){continue;}
 let full=min(vec2<u32>(q)*2u+offset,vec2<u32>(frame.extent.xy)-vec2<u32>(1u));let position=textureLoad(gpos,vec2<i32>(full));let n=textureLoad(gnrm,vec2<i32>(full));let old=old_history[u32(old_q.y)*half.x+u32(old_q.x)];if(position.w!=identity||old.position.w!=identity||dot(normal,n.xyz)<0.9){continue;}
 let value=vec2<f32>(luma(textureLoad(diffuse,q).rgb),luma(textureLoad(specular,q).rgb));total+=value;squares+=value*value;old_total+=vec2<f32>(luma(old.diffuse.rgb),luma(old.specular.rgb));old_variance+=max(vec2<f32>(old.moments.y-old.moments.x*old.moments.x,old.moments.w-old.moments.z*old.moments.z),vec2<f32>(0.));count+=1.;
 }}if(count<4.){return vec2<bool>(false);}let current_mean=total/count;let previous_mean=old_total/count;let current_variance=max(squares/count-current_mean*current_mean,vec2<f32>(0.));let uncertainty=3.*sqrt((current_variance+old_variance/count)/count);let threshold=max(uncertainty,0.1*max(current_mean,previous_mean)+vec2<f32>(0.01));return abs(current_mean-previous_mean)>threshold;
}
@compute @workgroup_size(8,8,1) fn main(@builtin(global_invocation_id) id:vec3<u32>){
 let half=(vec2<u32>(frame.extent.xy)+vec2<u32>(1u))/2u;if(any(id.xy>=half)){return;}let at=id.y*half.x+id.x;let p=vec2<i32>(id.xy);let offset=vec2<u32>(u32(frame.post.w)%2u,(u32(frame.post.w)/2u)%2u);let full=min(id.xy*2u+offset,vec2<u32>(frame.extent.xy)-vec2<u32>(1u));
 let position=textureLoad(gpos,vec2<i32>(full));let normal=textureLoad(gnrm,vec2<i32>(full));let mv=textureLoad(motion,vec2<i32>(full));var d=textureLoad(diffuse,p).rgb;let raw_s=textureLoad(specular,p);var s=raw_s.rgb;var age=1.;var moments=vec4<f32>(luma(d),luma(d)*luma(d),luma(s),luma(s)*luma(s));
 let previous_uv=(vec2<f32>(full)+vec2<f32>(0.5))/frame.extent.xy+mv.xy;
 if(frame.jitter.z<0.5&&position.w>0.&&all(previous_uv>=vec2<f32>(0.))&&all(previous_uv<vec2<f32>(1.))&&mv.w<1.5){
 let previous_pixel=min(vec2<u32>(previous_uv*vec2<f32>(half)),half-vec2<u32>(1u));let old=old_history[previous_pixel.y*half.x+previous_pixel.x];
 // A half-resolution history sample represents a 2x2 footprint whose film
 // offset rotates every frame. Validate within that projected world footprint,
 // rather than a fixed world-space distance that rejects low-resolution views.
 let vertical_projection=length(vec3<f32>(frame.vp[0].y,frame.vp[1].y,frame.vp[2].y));let footprint=6.*mv.z/max(frame.extent.y*vertical_projection,1.);
 let surface_valid=old.position.w==position.w&&distance(old.position.xyz,position.xyz)<max(0.025,footprint)&&dot(old.normal.xyz,normal.xyz)>0.9&&abs(old.normal.w-normal.w)<0.15;
 if(surface_valid){age=min(old.diffuse.w+1.,256.);let alpha=max(1./age,frame.reserved.w);let motion_alpha=clamp(length(mv.xy)*8.,0.,0.5);var da=max(alpha,motion_alpha);var sa=da;
 if(old.diffuse.w>=8.){let changed=radiance_changed(p,vec2<i32>(previous_pixel),position.w,normal.xyz,half,offset);if(changed.x){da=max(da,0.5);}if(changed.y){sa=max(sa,0.5);}}
 // A static glossy surface integrates its stochastic lobe. Changing ray hit
 // distance alone is not motion and must not throw those samples away.
 if(raw_s.w<0.||old.specular.w<0.){sa=max(sa,0.35);}
 if(normal.w<0.08&&motion_alpha>0.001&&abs(abs(old.specular.w)-abs(raw_s.w))>max(0.2,0.1*abs(raw_s.w))){sa=max(sa,0.5);}
 d=mix(old.diffuse.rgb,d,da);s=mix(old.specular.rgb,s,sa);moments=vec4<f32>(mix(old.moments.xy,moments.xy,da),mix(old.moments.zw,moments.zw,sa));
 }}
 new_history[at]=SignalHistory(vec4<f32>(d,age),vec4<f32>(s,raw_s.w),position,normal,moments);textureStore(out_diffuse,p,vec4<f32>(d,age));textureStore(out_specular,p,vec4<f32>(s,raw_s.w));
}
