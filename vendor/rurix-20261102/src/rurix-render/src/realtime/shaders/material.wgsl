fn texture_header(slot:u32)->u32{return 256u+slot*24u;}
fn vertex_at(index:u32)->Vertex {let a=vertices[index*3u];let b=vertices[index*3u+1u];return Vertex(a.xyz,vec3<f32>(a.w,b.xy),b.zw,vertices[index*3u+2u]);}
fn coord(i:i32,n:i32,mode:u32)->i32{if(mode==33071u){return clamp(i,0,n-1);}if(mode==33648u){let p=((i%(2*n))+2*n)%(2*n);return select(p,2*n-p-1,p>=n);}return ((i%n)+n)%n;}
fn pixel(slot:u32,level:u32,p:vec2<i32>)->vec4<f32>{let b=texture_header(slot);let w=max(1u,tex[b]>>level);let h=max(1u,tex[b+1u]>>level);let at=tex[b+8u+level]+u32(coord(p.y,i32(h),tex[b+3u]))*w+u32(coord(p.x,i32(w),tex[b+2u]));let v=tex[at];var color=vec4<f32>(f32(v&255u),f32((v>>8u)&255u),f32((v>>16u)&255u),f32(v>>24u))/255.;if(tex[b+5u]==0u){color=vec4<f32>(bitcast<f32>(tex[v&255u]),bitcast<f32>(tex[(v>>8u)&255u]),bitcast<f32>(tex[(v>>16u)&255u]),color.a);}return color;}
fn level_sample(slot:u32,level:u32,uv:vec2<f32>)->vec4<f32>{let b=texture_header(slot);if(max(1u,tex[b]>>level)==1u&&max(1u,tex[b+1u]>>level)==1u){return pixel(slot,level,vec2<i32>(0));}let wh=vec2<f32>(f32(max(1u,tex[b]>>level)),f32(max(1u,tex[b+1u]>>level)));let p=uv*wh-vec2<f32>(0.5);let i=vec2<i32>(floor(p));let f=fract(p);return mix(mix(pixel(slot,level,i),pixel(slot,level,i+vec2<i32>(1,0)),f.x),mix(pixel(slot,level,i+vec2<i32>(0,1)),pixel(slot,level,i+vec2<i32>(1,1)),f.x),f.y);}
fn sample_tex(slot:u32,uv:vec2<f32>,lod:f32)->vec4<f32>{if(slot==0xffffffffu){return vec4<f32>(1.);}let f=clamp(lod,0.,f32(tex[texture_header(slot)+4u]-1u));let a=u32(floor(f));let low=level_sample(slot,a,uv);if(fract(f)==0.){return low;}return mix(low,level_sample(slot,min(a+1u,tex[texture_header(slot)+4u]-1u),uv),fract(f));}
fn material_surface(mid:u32,p:vec3<f32>,normal:vec3<f32>,tangent:vec4<f32>,uv:vec2<f32>,lod:f32)->Surface{
 let m=mats[mid];let b=sample_tex(m.tex0.x,uv,lod);var n=safe_normalize(normal);let tx=safe_normalize(tangent.xyz-dot(tangent.xyz,n)*n);
 if(m.tex0.y!=0xffffffffu){var nm=sample_tex(m.tex0.y,uv,lod).xyz*2.-vec3<f32>(1.);nm=vec3<f32>(nm.xy*m.controls.y,nm.z);n=safe_normalize(tx*nm.x+cross(n,tx)*tangent.w*nm.y+n*nm.z);}
 let mr=sample_tex(m.tex0.z,uv,lod);var ao_sample=mr.r;if(m.tex0.z!=m.tex0.w){ao_sample=sample_tex(m.tex0.w,uv,lod).r;}let ao=mix(1.,ao_sample,m.controls.z);
 return Surface(p,n,m.base.rgb*b.rgb,m.emission_rough.xyz*sample_tex(m.tex1.x,uv,lod).rgb,clamp(m.emission_rough.w*mr.g,0.045,1.),clamp(m.controls.x*mr.b,0.,1.),m.base.a*b.a,ao,m.flags.y);
}
fn surface_hit(hit:RayIntersection,origin:vec3<f32>,direction:vec3<f32>)->Surface{
 let inst=instances[hit.instance_custom_data];let first=inst.info.x+hit.primitive_index*3u;let a=vertex_at(first);let b=vertex_at(first+1u);let c=vertex_at(first+2u);let w=vec3<f32>(1.-hit.barycentrics.x-hit.barycentrics.y,hit.barycentrics);
 var n=normal_transform(inst.model,a.n*w.x+b.n*w.y+c.n*w.z);if(dot(n,direction)>0.){n=-n;}
 let t=a.t*w.x+b.t*w.y+c.t*w.z;let world_t=vec4<f32>(safe_normalize((inst.model*vec4<f32>(t.xyz,0.)).xyz),t.w);
 let m=mats[inst.info.z];var lod=0.;if(m.tex0.x!=0xffffffffu){let edge1=(inst.model*vec4<f32>(b.p-a.p,0.)).xyz;let edge2=(inst.model*vec4<f32>(c.p-a.p,0.)).xyz;let du=b.uv-a.uv;let dv=c.uv-a.uv;let density=sqrt(abs(du.x*dv.y-du.y*dv.x)/max(length(cross(edge1,edge2)),1e-10));let footprint=2.*max(hit.t,0.1)/frame.extent.y;lod=log2(max(1.,f32(max(tex[texture_header(m.tex0.x)],tex[texture_header(m.tex0.x)+1u]))*density*footprint));}
 return material_surface(inst.info.z,origin+direction*hit.t,n,world_t,a.uv*w.x+b.uv*w.y+c.uv*w.z,lod);
}
