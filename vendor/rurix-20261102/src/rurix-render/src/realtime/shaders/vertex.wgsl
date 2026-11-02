@vertex fn main(@builtin(vertex_index) vi:u32,@builtin(instance_index) ii:u32)->Varying{
 let inst=instances[ii];let v=vertex_at(vi);let world=inst.model*vec4<f32>(v.p,1.);var o:Varying;
 o.cur_clip=frame.vp*world;o.clip=o.cur_clip;o.clip.x+=frame.jitter.x*2./frame.extent.x*o.clip.w;o.clip.y+=frame.jitter.y*2./frame.extent.y*o.clip.w;
 o.prev_clip=frame.previous_vp*inst.previous*vec4<f32>(v.p,1.);o.pos=world.xyz;
 o.n=normal_transform(inst.model,v.n);let determinant=dot(inst.model[0].xyz,cross(inst.model[1].xyz,inst.model[2].xyz));
 o.tangent=vec4<f32>(safe_normalize((inst.model*vec4<f32>(v.t.xyz,0.)).xyz),v.t.w*sign(determinant));o.uv=v.uv;o.material=inst.info.z;o.instance=ii;return o;
}
