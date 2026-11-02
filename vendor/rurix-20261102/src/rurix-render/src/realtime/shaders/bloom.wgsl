@group(0) @binding(0) var source:texture_storage_2d<rgba16float,read>;
@group(0) @binding(1) var destination_image:texture_storage_2d<rgba16float,write>;
@group(0) @binding(2) var<uniform> frame:Frame;
struct Control {mode:u32};var<push_constant> control:Control;
@compute @workgroup_size(8,8,1) fn main(@builtin(global_invocation_id) id:vec3<u32>){
 let size=textureDimensions(destination_image);if(any(id.xy>=size)){return;}let p=vec2<i32>(id.xy);let srcsize=vec2<i32>(textureDimensions(source));var sum=vec3<f32>(0.);var weight=0.;
 if(control.mode==0u){for(var y=0;y<2;y++){for(var x=0;x<2;x++){let c=textureLoad(source,min(p*2+vec2<i32>(x,y),srcsize-vec2<i32>(1))).rgb;let peak=max(c.r,max(c.g,c.b));sum+=c*max(0.,peak-frame.post.y)/max(peak,0.00001);}}sum*=0.25;}
 else{for(var i=-8;i<=8;i++){let w=exp(-f32(i*i)/18.);let delta=select(vec2<i32>(0,i),vec2<i32>(i,0),control.mode==1u);sum+=textureLoad(source,clamp(p+delta,vec2<i32>(0),srcsize-vec2<i32>(1))).rgb*w;weight+=w;}sum/=weight;}
 textureStore(destination_image,p,vec4<f32>(sum,1.));
}
