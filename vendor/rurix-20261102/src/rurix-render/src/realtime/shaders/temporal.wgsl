@group(0) @binding(0) var current:texture_storage_2d<rgba16float,read>;
@group(0) @binding(1) var gpos:texture_storage_2d<rgba32float,read>;
@group(0) @binding(2) var motion:texture_storage_2d<rgba16float,read>;
@group(0) @binding(3) var previous:texture_storage_2d<rgba16float,read>;
@group(0) @binding(4) var previous_pos:texture_storage_2d<rgba32float,read>;
@group(0) @binding(5) var resolved:texture_storage_2d<rgba16float,write>;
@group(0) @binding(6) var resolved_pos:texture_storage_2d<rgba32float,write>;
@group(0) @binding(7) var<uniform> frame:Frame;
fn current_sample(p:vec2<f32>)->vec3<f32>{let size=vec2<i32>(frame.extent.xy);let a=vec2<i32>(floor(p));let f=fract(p);let lo=vec2<i32>(0);let hi=size-vec2<i32>(1);return mix(mix(textureLoad(current,clamp(a,lo,hi)).rgb,textureLoad(current,clamp(a+vec2<i32>(1,0),lo,hi)).rgb,f.x),mix(textureLoad(current,clamp(a+vec2<i32>(0,1),lo,hi)).rgb,textureLoad(current,clamp(a+vec2<i32>(1,1),lo,hi)).rgb,f.x),f.y);}
fn history_sample(p:vec2<f32>)->vec3<f32>{let size=vec2<i32>(frame.extent.zw);let a=vec2<i32>(floor(p));let f=fract(p);let lo=vec2<i32>(0);let hi=size-vec2<i32>(1);return mix(mix(textureLoad(previous,clamp(a,lo,hi)).rgb,textureLoad(previous,clamp(a+vec2<i32>(1,0),lo,hi)).rgb,f.x),mix(textureLoad(previous,clamp(a+vec2<i32>(0,1),lo,hi)).rgb,textureLoad(previous,clamp(a+vec2<i32>(1,1),lo,hi)).rgb,f.x),f.y);}
@compute @workgroup_size(8,8,1) fn main(@builtin(global_invocation_id) id:vec3<u32>){
 if(any(id.xy>=vec2<u32>(frame.extent.zw))){return;}let p=vec2<i32>(id.xy);let uv=(vec2<f32>(id.xy)+vec2<f32>(0.5))/frame.extent.zw;
 let input=uv*frame.extent.xy-vec2<f32>(0.5)+vec2<f32>(frame.jitter.x,-frame.jitter.y);let nearest=clamp(vec2<i32>(round(input)),vec2<i32>(0),vec2<i32>(frame.extent.xy)-vec2<i32>(1));
 let position=textureLoad(gpos,nearest);let mv=textureLoad(motion,nearest);var color=current_sample(input);let old_uv=uv+mv.xy;
 if(frame.jitter.z<0.5&&position.w>0.&&mv.w<0.5&&all(old_uv>=vec2<f32>(0.))&&all(old_uv<vec2<f32>(1.))){
 let oldpixel=clamp(vec2<i32>(old_uv*frame.extent.zw),vec2<i32>(0),vec2<i32>(frame.extent.zw)-vec2<i32>(1));let oldpos=textureLoad(previous_pos,oldpixel);
 if(oldpos.w==position.w&&distance(oldpos.xyz,position.xyz)<max(0.015,0.003*mv.z)){
 var low=vec3<f32>(65000.);var high=vec3<f32>(0.);for(var y=-1;y<=1;y++){for(var x=-1;x<=1;x++){let tap=current_sample(input+vec2<f32>(f32(x),f32(y)));low=min(low,tap);high=max(high,tap);}}
 let old=clamp(history_sample(old_uv*frame.extent.zw-vec2<f32>(0.5)),low,high);let alpha=max(max(0.12,frame.reserved.w),clamp(length(mv.xy)*12.,0.,0.65));color=mix(old,color,alpha);
 }}textureStore(resolved,p,vec4<f32>(color,1.));textureStore(resolved_pos,p,vec4<f32>(position.xyz,select(position.w,-position.w,mv.w>=0.5)));
}
