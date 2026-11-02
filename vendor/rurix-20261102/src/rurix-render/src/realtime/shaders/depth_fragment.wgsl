@fragment fn main(v:DepthVarying,@builtin(front_facing) front:bool){
 let m=mats[v.material];if(!front&&m.flags.z==0u){discard;}var size=1.;if(m.tex0.x!=0xffffffffu){let b=texture_header(m.tex0.x);size=f32(max(tex[b],tex[b+1u]));}
 // Derivatives are outside material control flow, exactly as in the GBuffer.
 let lod=log2(max(max(length(dpdx(v.uv)),length(dpdy(v.uv)))*size,1.));
 if(m.flags.x==1u&&m.base.a*sample_tex(m.tex0.x,v.uv,lod).a<m.controls.w){discard;}
}
