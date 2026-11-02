struct GBuffer {@location(0) position:vec4<f32>,@location(1) normal:vec4<f32>,@location(2) base:vec4<f32>,@location(3) emission:vec4<f32>,@location(4) motion:vec4<f32>};
@fragment fn main(v:Varying,@builtin(front_facing) front:bool)->GBuffer{
 let m=mats[v.material];if(!front&&m.flags.z==0u){discard;}
 var n=v.n;if(!front){n=-n;}
 var size=1.;if(m.tex0.x!=0xffffffffu){size=f32(max(tex[texture_header(m.tex0.x)],tex[texture_header(m.tex0.x)+1u]));}
 let lod=log2(max(max(length(dpdx(v.uv)),length(dpdy(v.uv)))*size,1.));
 let s=material_surface(v.material,v.pos,n,v.tangent,v.uv,lod);
 if(m.flags.x==1u&&s.alpha<m.controls.w){discard;}
 var o:GBuffer;o.position=vec4<f32>(s.p,f32(v.instance+1u));o.normal=vec4<f32>(s.n,s.rough);o.base=vec4<f32>(s.base,s.metal);
 o.emission=vec4<f32>(s.emission,s.ao);if(s.unlit==1u){o.emission=vec4<f32>(s.base+s.emission,-1.);}
 let current=v.cur_clip.xy/v.cur_clip.w*vec2<f32>(0.5,-0.5)+vec2<f32>(0.5);let previous=v.prev_clip.xy/v.prev_clip.w*vec2<f32>(0.5,-0.5)+vec2<f32>(0.5);
 o.motion=vec4<f32>(previous-current,length(frame.eye_time.xyz-s.p),0.);return o;
}
