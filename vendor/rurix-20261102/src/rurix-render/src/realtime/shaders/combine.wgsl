@group(0) @binding(0) var direct:texture_storage_2d<rgba16float,read>;
@group(0) @binding(1) var gi:texture_storage_2d<rgba16float,read>;
@group(0) @binding(2) var refl:texture_storage_2d<rgba16float,read>;
@group(0) @binding(3) var gpos:texture_storage_2d<rgba32float,read>;
@group(0) @binding(4) var gnrm:texture_storage_2d<rgba16float,read>;
@group(0) @binding(5) var hdr:texture_storage_2d<rgba16float,write>;
@group(0) @binding(6) var volume:texture_storage_2d<rgba16float,read>;
@group(0) @binding(7) var<uniform> frame:Frame;
@compute @workgroup_size(8,8,1) fn main(@builtin(global_invocation_id) id:vec3<u32>){
 let size=vec2<i32>(frame.extent.xy);if(any(id.xy>=vec2<u32>(size))){return;}let p=vec2<i32>(id.xy);let center=textureLoad(gpos,p);let normal=textureLoad(gnrm,p);var sum=vec3<f32>(0.);var weight=0.;
 var volume_sum=vec4<f32>(0.);if(center.w>0.){let half=(size+vec2<i32>(1))/2;let offset=vec2<i32>(i32(frame.post.w)%2,(i32(frame.post.w)/2)%2);let base=p/2;
 for(var y=-1;y<=1;y++){for(var x=-1;x<=1;x++){let h=clamp(base+vec2<i32>(x,y),vec2<i32>(0),half-vec2<i32>(1));let q=min(h*2+offset,size-vec2<i32>(1));let pos=textureLoad(gpos,q);let n=textureLoad(gnrm,q);
 let distance_weight=1./(1.+dot(pos.xyz-center.xyz,pos.xyz-center.xyz)*100.);let w=pow(max(dot(n.xyz,normal.xyz),0.),16.)*distance_weight*select(0.,1.,pos.w>0.);
 sum+=(textureLoad(gi,h).rgb+textureLoad(refl,h).rgb)*w;volume_sum+=textureLoad(volume,h)*w;weight+=w;}}}
 var color=textureLoad(direct,p).rgb+sum/max(weight,0.00001);if(weight>0.){let vol=volume_sum/weight;color=color*vol.a+vol.rgb;}
 let distance=select(80.,length(center.xyz-frame.eye_time.xyz),center.w>0.);let transmittance=exp(-frame.fog.w*distance);color=mix(frame.fog.rgb,color,transmittance);
 textureStore(hdr,p,vec4<f32>(min(max(color,vec3<f32>(0.)),vec3<f32>(65000.)),1.));
}
