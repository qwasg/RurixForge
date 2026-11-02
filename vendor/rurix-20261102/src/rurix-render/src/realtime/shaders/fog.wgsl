fn volume_interval(f:FogVolume,origin:vec3<f32>,direction:vec3<f32>)->vec2<f32>{
 let o=(f.inverse_model*vec4<f32>(origin,1.)).xyz/f.size_density.xyz;let d=(f.inverse_model*vec4<f32>(direction,0.)).xyz/f.size_density.xyz;
 if(f.emission_shape.w>0.5){let a=dot(d,d);let b=dot(o,d);let c=dot(o,o)-1.;let discriminant=b*b-a*c;if(discriminant<0.){return vec2<f32>(1.,-1.);}let root=sqrt(discriminant);return vec2<f32>((-b-root)/a,(-b+root)/a);}
 let safe_d=select(vec3<f32>(-1.),vec3<f32>(1.),d>=vec3<f32>(0.))*max(abs(d),vec3<f32>(1e-8));let a=(-vec3<f32>(1.)-o)/safe_d;let b=(vec3<f32>(1.)-o)/safe_d;let lo=min(a,b);let hi=max(a,b);return vec2<f32>(max(lo.x,max(lo.y,lo.z)),min(hi.x,min(hi.y,hi.z)));
}
@compute @workgroup_size(8,8,1) fn main(@builtin(global_invocation_id) id:vec3<u32>){
 let half=(vec2<u32>(frame.extent.xy)+vec2<u32>(1u))/2u;if(any(id.xy>=half)){return;}let p=vec2<i32>(id.xy);let offset=vec2<u32>(u32(frame.post.w)%2u,(u32(frame.post.w)/2u)%2u);let full=min(id.xy*2u+offset,vec2<u32>(frame.extent.xy)-vec2<u32>(1u));
 let position=textureLoad(gpos,vec2<i32>(full));if(position.w==0.||frame.reserved.y<0.5){textureStore(volume_output,p,vec4<f32>(0.,0.,0.,1.));return;}
 let origin=frame.eye_time.xyz;let direction=safe_normalize(position.xyz-origin);let max_distance=length(position.xyz-origin);var begin=max_distance;var end=0.;
 for(var i=0u;i<u32(frame.reserved.y);i++){let interval=volume_interval(volumes[i],origin,direction);begin=min(begin,max(0.,interval.x));end=max(end,min(max_distance,interval.y));}
 if(end<=begin){textureStore(volume_output,p,vec4<f32>(0.,0.,0.,1.));return;}
 let step=(end-begin)/16.;let seed=hash(full.x+full.y*u32(frame.extent.x)+u32(frame.post.w)*18973u);var transmittance=1.;var scattering=vec3<f32>(0.);
 for(var sample_index=0u;sample_index<16u;sample_index++){let distance=begin+(f32(sample_index)+random(seed+sample_index))*step;let point=origin+direction*distance;var density=0.;var source=vec3<f32>(0.);
 for(var i=0u;i<u32(frame.reserved.y);i++){let f=volumes[i];let local=(f.inverse_model*vec4<f32>(point,1.)).xyz/f.size_density.xyz;let inside=select(all(abs(local)<=vec3<f32>(1.)),dot(local,local)<=1.,f.emission_shape.w>0.5);if(!inside){continue;}
 var incoming=frame.environment.rgb;let chosen=important_light(point,vec3<f32>(0.),seed+sample_index*13u+37u);if(chosen.y>0.){let light=lights[u32(chosen.x)];let ls=light_sample(light,point,seed+sample_index*53u);let g=f.albedo_g.w;let phase=(1.-g*g)/(12.5663706*pow(max(0.001,1.+g*g-2.*g*dot(direction,ls.xyz)),1.5));var visibility=1.;if(frame.features.w>0.5&&light.shape.w>0.5){let hit=trace_ray(point,ls.xyz,max(0.003,ls.w-0.004));if(hit.kind!=0u){visibility=0.;}}incoming+=light_radiance(light,point,ls.xyz,ls.w)*phase/chosen.y*visibility;}
 let sample_density=f.size_density.w*fog_edge_weight(local,f.size_density.xyz,f.emission_shape.w,f.edge.x);
 density+=sample_density;source+=sample_density*(f.albedo_g.rgb*incoming+f.emission_shape.rgb);
 }let segment_t=exp(-density*step);scattering+=transmittance*source*(1.-segment_t)/max(density,1e-6);transmittance*=segment_t;
 }textureStore(volume_output,p,vec4<f32>(min(scattering,vec3<f32>(65000.)),transmittance));
}
