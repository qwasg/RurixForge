@group(0) @binding(0) var hdr:texture_storage_2d<rgba16float,read>;
@group(0) @binding(1) var bloom:texture_storage_2d<rgba16float,read>;
@group(0) @binding(2) var output:texture_storage_2d<rgba8unorm,write>;
@group(0) @binding(3) var<uniform> frame:Frame;
@compute @workgroup_size(8,8,1) fn main(@builtin(global_invocation_id) id:vec3<u32>){
 if(any(id.xy>=vec2<u32>(frame.extent.zw))){return;}let p=vec2<i32>(id.xy);let bp=min(p/2,vec2<i32>(textureDimensions(bloom))-vec2<i32>(1));var c=(textureLoad(hdr,p).rgb+textureLoad(bloom,bp).rgb*frame.post.z)*frame.post.x;
 // ACES fitted SDR curve, applied once after HDR temporal resolution and bloom.
 if(frame.reserved.x>0.5){c=clamp((c*(2.51*c+vec3<f32>(0.03)))/(c*(2.43*c+vec3<f32>(0.59))+vec3<f32>(0.14)),vec3<f32>(0.),vec3<f32>(1.));}else{c=clamp(c,vec3<f32>(0.),vec3<f32>(1.));}
 let dither=(random(id.x+id.y*u32(frame.extent.z)+u32(frame.post.w)*5743u)-0.5)/255.;textureStore(output,p,vec4<f32>(clamp(encode_srgb(c)+vec3<f32>(dither),vec3<f32>(0.),vec3<f32>(1.)),1.));
}
