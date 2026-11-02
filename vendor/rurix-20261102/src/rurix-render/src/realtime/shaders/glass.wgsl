struct GlassOutput {@location(0) color:vec4<f32>,@location(1) motion:vec4<f32>};
@fragment fn main(v:Varying,@builtin(front_facing) front:bool)->GlassOutput{
 let m=mats[v.material];if(!front&&m.flags.z==0u){discard;}var n=v.n;if(!front){n=-n;}let s=material_surface(v.material,v.pos,n,v.tangent,v.uv,0.);
 let view=safe_normalize(frame.eye_time.xyz-v.pos);var c=s.emission+s.base*frame.environment.rgb;
 for(var i=0u;i<u32(frame.features.x);i++){let light=lights[i];let delta=light.pos_range.xyz-s.p;let distance=length(delta);let l=delta/max(distance,0.001);var a=pow(max(0.,1.-pow(distance/light.pos_range.w,4.)),2.)/max(distance*distance,0.01);
 if(light.dir_kind.w>0.5&&light.dir_kind.w<1.5){a*=smoothstep(light.shape.z,max(light.shape.y,light.shape.z+0.0001),dot(safe_normalize(light.dir_kind.xyz),-l));}
 if(light.dir_kind.w>1.5){a*=max(dot(safe_normalize(light.dir_kind.xyz),-l),0.);}
 c+=ggx_brdf(s.n,view,l,s.base,s.rough,s.metal)*light.color_intensity.rgb*light.color_intensity.w*a;}
 if(s.unlit==1u){c=s.base+s.emission;}
 let fresnel=0.04+0.96*pow(1.-max(dot(n,view),0.),5.);let alpha=clamp(s.alpha+fresnel*(1.-s.alpha),0.,1.);
 let transmittance=exp(-frame.fog.w*length(s.p-frame.eye_time.xyz));c=mix(frame.fog.rgb,c,transmittance);
 let current=v.cur_clip.xy/v.cur_clip.w*vec2<f32>(0.5,-0.5)+vec2<f32>(0.5);let previous=v.prev_clip.xy/v.prev_clip.w*vec2<f32>(0.5,-0.5)+vec2<f32>(0.5);
 // Motion attachment alpha=1 replaces its previous value even when the color
 // attachment uses partial alpha. The final history marks this pixel reactive.
 return GlassOutput(vec4<f32>(c,alpha),vec4<f32>(previous-current,length(s.p-frame.eye_time.xyz),1.));
}
