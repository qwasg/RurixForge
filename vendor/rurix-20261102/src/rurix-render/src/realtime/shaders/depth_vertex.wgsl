@vertex fn main(@builtin(vertex_index) vi:u32)->DepthVarying{
 let reference=draw_references[vi];let inst=instances[reference.y];let v=vertex_at(reference.x);let world=inst.model*vec4<f32>(v.p,1.);var o:DepthVarying;
 o.clip=frame.vp*world;o.clip.x+=frame.jitter.x*2./frame.extent.x*o.clip.w;o.clip.y+=frame.jitter.y*2./frame.extent.y*o.clip.w;o.uv=v.uv;o.material=inst.info.z;return o;
}
