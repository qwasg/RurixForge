@compute @workgroup_size(8,8,1) fn main(@builtin(global_invocation_id) id:vec3<u32>){
 if(any(id.xy>=vec2<u32>(frame.extent.xy))){return;}let p=vec2<i32>(id.xy);let pos=textureLoad(gpos,p);if(pos.w==0.){return;}
 var nr=textureLoad(gnrm,p);var ba=textureLoad(gbase,p);var em=textureLoad(gemit,p);
 for(var i=0u;i<u32(frame.reserved.z);i++){let decal=decals[i];let local=(decal.inverse_model*vec4<f32>(pos.xyz,1.)).xyz;
 if(any(abs(local)>decal.size_opacity.xyz)){continue;}
 let projector_normal=safe_normalize(decal.model[1].xyz);let facing=smoothstep(0.15,0.5,dot(nr.xyz,projector_normal));if(facing<=0.){continue;}
 let uv=local.xz/(decal.size_opacity.xz*2.)+vec2<f32>(0.5);let tangent=vec4<f32>(safe_normalize(decal.model[0].xyz),-1.);let s=material_surface(decal.info.x,pos.xyz,nr.xyz,tangent,uv,0.);
 let alpha=s.alpha*decal.size_opacity.w*facing;ba=vec4<f32>(mix(ba.rgb,s.base,alpha),mix(ba.w,s.metal,alpha));nr=vec4<f32>(safe_normalize(mix(nr.xyz,s.n,alpha)),mix(nr.w,s.rough,alpha));em=vec4<f32>(mix(em.rgb,s.emission,alpha),mix(em.w,s.ao,alpha));
 }textureStore(gnrm,p,nr);textureStore(gbase,p,ba);textureStore(gemit,p,em);
}
