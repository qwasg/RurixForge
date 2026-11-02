fn trace_ray(origin:vec3<f32>,direction:vec3<f32>,distance:f32)->RayIntersection{
 var query:ray_query;rayQueryInitialize(&query,tlas,RayDesc(2u,255u,0.002,max(distance,0.003),origin,direction));
 while(rayQueryProceed(&query)){let c=rayQueryGetCandidateIntersection(&query);if(c.kind==1u){let inst=instances[c.instance_custom_data];let m=mats[inst.info.z];var accept=m.flags.x!=2u;if(!c.front_face&&m.flags.z==0u){accept=false;}if(accept&&m.flags.x==1u){accept=surface_hit(c,origin,direction).alpha>=m.controls.w;}if(accept){rayQueryConfirmIntersection(&query);}}}
 return rayQueryGetCommittedIntersection(&query);
}
fn light_sample(light:Light,p:vec3<f32>,seed:u32)->vec4<f32>{
 var destination_image=light.pos_range.xyz;let normal=safe_normalize(light.dir_kind.xyz);let axes=basis(normal);
 if(light.dir_kind.w>1.5){destination_image+=axes[0]*(random(seed)-0.5)*light.rect.x+axes[1]*(random(seed+7u)-0.5)*light.rect.y;}
 else if(light.shape.x>0.){let disk=basis(safe_normalize(p-destination_image));let phi=6.2831853*random(seed);let r=sqrt(random(seed+7u))*light.shape.x;destination_image+=r*(disk[0]*cos(phi)+disk[1]*sin(phi));}
 let delta=destination_image-p;let distance=length(delta);return vec4<f32>(delta/max(distance,1e-5),distance);
}
fn light_radiance(light:Light,p:vec3<f32>,l:vec3<f32>,distance:f32)->vec3<f32>{
 let d2=max(distance*distance,max(light.shape.x*light.shape.x,0.01));var att=pow(max(0.,1.-pow(distance/light.pos_range.w,4.)),2.)/d2;
 if(light.dir_kind.w>0.5&&light.dir_kind.w<1.5){let cone=dot(safe_normalize(light.dir_kind.xyz),-l);att*=smoothstep(light.shape.z,max(light.shape.y,light.shape.z+0.0001),cone);}
 if(light.dir_kind.w>1.5){att*=max(dot(safe_normalize(light.dir_kind.xyz),-l),0.);}
 return light.color_intensity.rgb*light.color_intensity.w*att;
}
fn shade_light(s:Surface,v:vec3<f32>,index:u32,seed:u32,shadows:bool)->vec3<f32>{
 let light=lights[index];if(distance(light.pos_range.xyz,s.p)>light.pos_range.w){return vec3<f32>(0.);}
 let sample=light_sample(light,s.p,seed);let incoming=light_radiance(light,s.p,sample.xyz,sample.w);
 if(dot(s.n,sample.xyz)<=0.||max(incoming.r,max(incoming.g,incoming.b))<1e-5){return vec3<f32>(0.);}
 if(shadows&&light.shape.w>0.5){let hit=trace_ray(s.p+s.n*0.003,sample.xyz,max(0.003,sample.w-0.006));if(hit.kind!=0u){return vec3<f32>(0.);}}
 return ggx_brdf(s.n,v,sample.xyz,s.base,s.rough,s.metal)*incoming;
}
// Weighted reservoir selection; the returned probability belongs to the
// selected authored light. Visibility is evaluated only after selection.
fn important_light(p:vec3<f32>,n:vec3<f32>,seed:u32)->vec2<f32>{
 var total=0.;var selected=0u;var selected_weight=0.;
 for(var i=0u;i<u32(frame.features.x);i++){let light=lights[i];let delta=light.pos_range.xyz-p;let distance=length(delta);if(distance>=light.pos_range.w||light.color_intensity.w<=0.){continue;}
 let direction=delta/max(distance,0.001);let li=light_radiance(light,p,direction,distance);let weight=max(max(li.r,max(li.g,li.b))*(0.05+max(dot(n,direction),0.)),1e-7);total+=weight;
 if(random(seed+i*739u)*total<weight){selected=i;selected_weight=weight;}}
 return vec2<f32>(f32(selected),select(0.,selected_weight/max(total,1e-20),total>0.));
}
fn incident(hit:RayIntersection,origin:vec3<f32>,direction:vec3<f32>,seed:u32)->vec3<f32>{
 if(hit.kind==0u){return frame.environment.rgb;}
 let s=surface_hit(hit,origin,direction);if(s.unlit==1u){return s.base+s.emission;}
 var c=s.emission+s.base*(1.-s.metal)*frame.environment.rgb*s.ao;let chosen=important_light(s.p,s.n,seed);
 if(chosen.y>0.){c+=shade_light(s,-direction,u32(chosen.x),seed+99u,frame.features.w>0.5)/chosen.y;}
 return c;
}
