@group(0) @binding(0) var<storage,read_write> destination:array<u32>;
@group(0) @binding(1) var source:texture_storage_2d<rgba8unorm,read>;
struct Control {row_words:u32};var<push_constant> control:Control;
@compute @workgroup_size(8,8,1) fn main(@builtin(global_invocation_id) id:vec3<u32>){let size=textureDimensions(source);if(any(id.xy>=size)){return;}let p=vec4<u32>(round(clamp(textureLoad(source,vec2<i32>(id.xy)),vec4<f32>(0.),vec4<f32>(1.))*255.));destination[id.y*control.row_words+id.x]=p.x|(p.y<<8u)|(p.z<<16u)|(p.w<<24u);}
