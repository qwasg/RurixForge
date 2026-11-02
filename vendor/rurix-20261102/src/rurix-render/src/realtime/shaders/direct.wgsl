@compute @workgroup_size(8,8,1) fn main(@builtin(global_invocation_id) id:vec3<u32>){
 if(any(id.xy>=vec2<u32>(frame.extent.xy))){return;}let p=vec2<i32>(id.xy);let pos=textureLoad(gpos,p);if(pos.w==0.){textureStore(output0,p,vec4<f32>(frame.environment.rgb,1.));textureStore(output1,p,vec4<f32>(0.));return;}
 let nr=textureLoad(gnrm,p);let ba=textureLoad(gbase,p);let em=textureLoad(gemit,p);let s=Surface(pos.xyz,safe_normalize(nr.xyz),ba.rgb,em.rgb,nr.w,ba.w,1.,abs(em.w),0u);
 var c=em.rgb;if(em.w>=0.){c+=s.base*(1.-s.metal)*frame.environment.rgb*s.ao;}
 if(em.w>=0.) {let v=safe_normalize(frame.eye_time.xyz-s.p);let seed=hash(id.x+id.y*u32(frame.extent.x)+u32(frame.post.w)*72677u);
 if(frame.environment.w>0.5){let tile=(id.y/8u)*((u32(frame.extent.x)+7u)/8u)+id.x/8u;for(var word=0u;word<(u32(frame.features.x)+31u)/32u;word++){var bits=tile_masks[tile*8u+word];while(bits!=0u){let bit=firstTrailingBit(bits);let i=word*32u+bit;if(i<u32(frame.features.x)){c+=shade_light(s,v,i,seed+i*251u,frame.features.w>0.5);}bits=bits&(bits-1u);}}}
 else{for(var i=0u;i<u32(frame.features.x);i++){c+=shade_light(s,v,i,seed+i*251u,frame.features.w>0.5);}}}
 textureStore(output0,p,vec4<f32>(min(c,vec3<f32>(65000.)),1.));textureStore(output1,p,vec4<f32>(0.));
}
