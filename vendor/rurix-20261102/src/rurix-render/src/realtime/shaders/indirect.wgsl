@compute @workgroup_size(8,8,1) fn main(@builtin(global_invocation_id) id:vec3<u32>){
 let half=(vec2<u32>(frame.extent.xy)+vec2<u32>(1u))/2u;if(any(id.xy>=half)){return;}
 let offset=vec2<u32>(u32(frame.post.w)%2u,(u32(frame.post.w)/2u)%2u);let full=min(id.xy*2u+offset,vec2<u32>(frame.extent.xy)-vec2<u32>(1u));let p=vec2<i32>(full);let outp=vec2<i32>(id.xy);
 let pos=textureLoad(gpos,p);if(pos.w==0.){textureStore(output0,outp,vec4<f32>(0.));textureStore(output1,outp,vec4<f32>(0.));return;}
 let nr=textureLoad(gnrm,p);let n=safe_normalize(nr.xyz);let ba=textureLoad(gbase,p);let em=textureLoad(gemit,p);let seed=hash(full.x+full.y*u32(frame.extent.x)+u32(frame.post.w)*23891u);let origin=pos.xyz+n*0.003;
 var gi=vec3<f32>(0.);if(frame.features.y>0.5&&em.w>=0.){let direction=cosine_direction(n,seed);let hit=trace_ray(origin,direction,60.);gi=incident(hit,origin,direction,seed+13u)*ba.rgb*(1.-ba.w)*0.96;}
 var refl=vec3<f32>(0.);var reflection_distance=0.;if(frame.features.z>0.5&&nr.w<0.85&&em.w>=0.){
 let view=safe_normalize(frame.eye_time.xyz-pos.xyz);
 if(dot(n,view)>0.){
 let h=ggx_visible_half(n,view,nr.w,vec2<f32>(random(seed+91u),random(seed+39u)));let direction=reflect(-view,h);
 if(dot(direction,n)>0.){let hit=trace_ray(origin,direction,80.);let weight=ggx_visible_weight(n,view,direction,h,ba.rgb,ba.w,nr.w);refl=incident(hit,origin,direction,seed+137u)*weight;reflection_distance=select(hit.t,80.,hit.kind==0u);
 if(hit.kind!=0u){let object=instances[hit.instance_custom_data];let moving=any(object.model[0]!=object.previous[0])||any(object.model[1]!=object.previous[1])||any(object.model[2]!=object.previous[2])||any(object.model[3]!=object.previous[3]);if(moving){reflection_distance=-max(hit.t,0.001);}}
 }}}
 textureStore(output0,outp,vec4<f32>(min(gi,vec3<f32>(65000.)),1.));textureStore(output1,outp,vec4<f32>(min(refl,vec3<f32>(65000.)),reflection_distance));
}
