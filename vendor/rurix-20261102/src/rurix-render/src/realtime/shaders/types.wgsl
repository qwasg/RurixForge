struct Frame {
 vp:mat4x4<f32>, previous_vp:mat4x4<f32>, eye_time:vec4<f32>, extent:vec4<f32>,
 environment:vec4<f32>, fog:vec4<f32>, post:vec4<f32>, jitter:vec4<f32>,
 features:vec4<f32>, reserved:vec4<f32>,
};
struct Instance { model:mat4x4<f32>, previous:mat4x4<f32>, info:vec4<u32> };
struct Material { base:vec4<f32>, emission_rough:vec4<f32>, controls:vec4<f32>, flags:vec4<u32>, tex0:vec4<u32>,tex1:vec4<u32> };
struct Light { pos_range:vec4<f32>,dir_kind:vec4<f32>,color_intensity:vec4<f32>,shape:vec4<f32>,rect:vec4<f32> };
struct Vertex {p:vec3<f32>,n:vec3<f32>,uv:vec2<f32>,t:vec4<f32>};
struct Surface {p:vec3<f32>,n:vec3<f32>,base:vec3<f32>,emission:vec3<f32>,rough:f32,metal:f32,alpha:f32,ao:f32,unlit:u32};
struct Varying {
 @builtin(position) clip:vec4<f32>, @location(0) pos:vec3<f32>,@location(1) n:vec3<f32>,
 @location(2) uv:vec2<f32>,@location(3) tangent:vec4<f32>,@location(4) prev_clip:vec4<f32>,
 @location(5) cur_clip:vec4<f32>,@location(6) @interpolate(flat) material:u32,
 @location(7) @interpolate(flat) instance:u32,
};
struct DepthVarying {@builtin(position) clip:vec4<f32>,@location(0) uv:vec2<f32>,@location(1) @interpolate(flat) material:u32};
fn safe_normalize(v:vec3<f32>)->vec3<f32>{return v*inverseSqrt(max(dot(v,v),1e-16));}
fn normal_transform(m:mat4x4<f32>,n:vec3<f32>)->vec3<f32>{let a=m[0].xyz;let b=m[1].xyz;let c=m[2].xyz;let det=dot(a,cross(b,c));return safe_normalize((cross(b,c)*n.x+cross(c,a)*n.y+cross(a,b)*n.z)/det);}
fn linear(c:vec3<f32>)->vec3<f32>{return select(c/12.92,pow((c+vec3<f32>(0.055))/1.055,vec3<f32>(2.4)),c>vec3<f32>(0.04045));}
fn encode_srgb(c:vec3<f32>)->vec3<f32>{return select(c*12.92,1.055*pow(max(c,vec3<f32>(0.)),vec3<f32>(1./2.4))-0.055,c>vec3<f32>(0.0031308));}
fn hash(v:u32)->u32{var x=v;x=(x^(x>>16u))*0x7feb352du;x=(x^(x>>15u))*0x846ca68bu;return x^(x>>16u);}
fn random(seed:u32)->f32{return f32(hash(seed)&0x00ffffffu)/16777216.;}
fn basis(n:vec3<f32>)->mat3x3<f32>{let a=select(vec3<f32>(0.,1.,0.),vec3<f32>(1.,0.,0.),abs(n.y)>0.95);let t=safe_normalize(cross(a,n));return mat3x3<f32>(t,cross(n,t),n);}
fn cosine_direction(n:vec3<f32>,seed:u32)->vec3<f32>{let a=6.2831853*random(seed);let r=sqrt(random(seed+17u));return basis(n)*vec3<f32>(r*cos(a),r*sin(a),sqrt(max(0.,1.-r*r)));}
// Heitz 2018 equations 3-6, matching the already independently checked
// ci/hq_vndf_codegen.py proposal. Only the sampling formula is shared; this
// real-time lane keeps its own single-bounce resources and exact Smith BRDF.
fn ggx_visible_half(n:vec3<f32>,v:vec3<f32>,rough:f32,uv:vec2<f32>)->vec3<f32>{
 let alpha=rough*rough;let axes=basis(n);let local_view=vec3<f32>(dot(v,axes[0]),dot(v,axes[1]),dot(v,n));let stretched=safe_normalize(vec3<f32>(alpha*local_view.xy,local_view.z));
 let horizontal=length(stretched.xy);var first=vec3<f32>(1.,0.,0.);if(horizontal>0.){first=vec3<f32>(-stretched.y,stretched.x,0.)/horizontal;}let second=cross(stretched,first);
 let radius=sqrt(uv.y);let phi=6.2831853*uv.x;let x=radius*cos(phi);let blend=(1.+stretched.z)*0.5;let y=(1.-blend)*sqrt(max(0.,1.-x*x))+blend*radius*sin(phi);let z=sqrt(max(0.,1.-x*x-y*y));let h=first*x+second*y+stretched*z;
 return safe_normalize(axes*safe_normalize(vec3<f32>(alpha*h.xy,max(h.z,0.))));
}
fn ggx_visible_weight(n:vec3<f32>,v:vec3<f32>,l:vec3<f32>,h:vec3<f32>,base:vec3<f32>,metal:f32,rough:f32)->vec3<f32>{
 let nv=dot(n,v);let nl=dot(n,l);let vh=dot(v,h);if(nv<=0.||nl<=0.||vh<=0.){return vec3<f32>(0.);}let alpha=rough*rough;let g1l=2.*nl/(nl+sqrt(alpha*alpha+(1.-alpha*alpha)*nl*nl));let f0=mix(vec3<f32>(0.04),base,metal);let f=f0+(vec3<f32>(1.)-f0)*pow(1.-clamp(vh,0.,1.),5.);
 // f_spec*NL / p_L, p_L=D(H)*G1(V)/(4*NV). The exact Smith
 // factors cancel to F*G1(L), without a firefly/throughput clamp.
 return f*g1l;
}
fn ggx_brdf(n:vec3<f32>,v:vec3<f32>,l:vec3<f32>,base:vec3<f32>,rough:f32,metal:f32)->vec3<f32>{
 let nv=max(dot(n,v),0.001);let nl=max(dot(n,l),0.);let h=safe_normalize(v+l);let nh=max(dot(n,h),0.);let vh=max(dot(v,h),0.);
 let a=rough*rough;let a2=a*a;let den=nh*nh*(a2-1.)+1.;let d=a2/max(3.14159265*den*den,1e-7);let k=pow(rough+1.,2.)/8.;
 let g=nv/(nv*(1.-k)+k)*nl/(nl*(1.-k)+k);let f0=mix(vec3<f32>(0.04),base,metal);let f=f0+(vec3<f32>(1.)-f0)*pow(1.-vh,5.);
 return ((vec3<f32>(1.)-f)*(1.-metal)*base/3.14159265+d*g*f/max(4.*nv*nl,0.001))*nl;
}
