//! Viewport 模块(F1 wave.2,RD-F1-001 回填):编辑器相机 + rurix-rt render_exec(Vulkan)
//! 场景实渲染 + Readback 回读 + 射线点选。
//!
//! 架构:
//! - 实体(MeshRenderer enabled)以相机 UBO(viewProj)+ 逐实体 push constants
//!   (model 64B + color 16B = 80B ≤ 128)经 GPU 光栅化(Depth32Float,LESS_OR_EQUAL),
//!   `Readback::Texture` 回读 RGBA8。该帧既供 canvas 回退腿,也供 D3D12 共享纹理生产者作帧源。
//! - 网格(2026-08-28 资产→视口断链接线):`MeshRenderer.mesh` 引用经 [`crate::meshres`]
//!   解析到项目 .rxmesh 构建产物 → 展平顶点缓冲(pos3+normal3);解析失败/超上限诚实回退
//!   内置 cube 并计入 `meshFallbacks`。pass 槽位按网格类静态绑定 VB(类槽位按当帧实体
//!   计数分配,cube 类兜底占余量),实体帧内按类序稳定占槽;网格类布局变化触发会话重建
//!   (与改尺寸同路径,编辑期人手尺度,代价有界)。
//! - 固定 pass 图:[`MAX_DRAW_SLOTS`] 个 draw pass 常驻;未占槽以「远埋微缩」模型矩阵消隐
//!   (有限值,避免 NaN 顶点未定义光栅化)。每帧仅经 `FrameUpdate`(buffer_uploads 相机 +
//!   push_constant_overrides 实体)驱动,provenance 可机验。
//! - 着色器:WGSL 源码经 naga 纯 Rust 编译为 SPIR-V(缓存钉版 =25.0.1)。
//! - 诚实三态:vulkan loader/能力缺失 → `DEV_ENV_DEGRADE:` 前缀错误,绝不伪造帧。
//!
//! 会话描述块(resources/passes/barriers/readbacks)借给 `DeviceFrameSession<'static>`,
//! 经 `Box::leak` 提升;重建发生在视口改尺寸、实体数超档升档(F6 wave.5,只升不降)或
//! 网格类布局变化,代价有界。

#[cfg(feature = "backend-rurix")]
use std::sync::{Mutex, OnceLock};

use base64::Engine as _;
#[cfg(feature = "backend-rurix")]
use forge_scene::{Scene, Transform};
#[cfg(feature = "backend-rurix")]
use rurix_rt::render_exec as rex;
#[cfg(feature = "backend-rurix")]
use rurix_rt::vk;
#[cfg(feature = "backend-rurix")]
use serde_json::Value;

#[cfg(feature = "backend-rurix")]
use crate::meshres::{self, MeshGpu};
#[cfg(feature = "backend-rurix")]
use crate::render_core::assets::entity_tint;
#[cfg_attr(not(feature = "backend-rurix"), allow(unused_imports))]
pub(crate) use crate::render_core::assets::{cube_mesh_bytes, material_albedo_guid, ASSET_GENERATION};
#[cfg_attr(not(feature = "backend-rurix"), allow(unused_imports))]
pub use crate::render_core::assets::{load_tex_static_cached, sprite_doc_cached, TexGpu};
pub use crate::render_core::camera::{scene_camera_ray, scene_camera_view_proj, EditorCamera};
#[cfg(feature = "backend-rurix")]
use crate::render_core::cull::sprite_offscreen;
pub(crate) use crate::render_core::math::{m4_mul, trs_model, M4};
pub use crate::render_core::pick::pick_entity;
#[cfg(all(test, feature = "backend-rurix"))]
pub(crate) use crate::render_core::pick::ray_unit_cube;
#[cfg(feature = "backend-rurix")]
pub use crate::render_core::sprite::SpriteRenderInfo;
#[cfg(feature = "backend-rurix")]
pub(crate) use crate::render_core::sprite::sprite_render_transform;
#[cfg(feature = "backend-rurix")]
use crate::render_core::sprite::{
    entity_mesh_ref, is_renderable, sprite_bool, sprite_component, sprite_compositing, sprite_sorting_order,
    SpriteBlend, FULL_UV_RECT,
};

// rurix 渲染腿(DeviceFrameSession 会话、WGSL→SPIR-V、槽位 / UBO / push constants 打包)在 viewport/rurix.rs(02 §5.2);
// 它经 `use super::*` 共用本模块的导入。其余模块经下面的再导出调用,路径不变。
#[cfg(feature = "backend-rurix")]
pub(crate) mod rurix;
#[cfg(feature = "backend-rurix")]
pub(crate) use rurix::{compile_wgsl, m4_col_bytes, render_scene_frame, MAX_DRAW_SLOTS};

pub(crate) fn invalidate_assets() {
    ASSET_GENERATION.fetch_add(1,std::sync::atomic::Ordering::Relaxed);
    // rurix 部分:清视口会话(与拆分前同一顺序:先代次 +1,再清会话)。
    #[cfg(feature = "backend-rurix")]
    rurix::reset_renderer();
}

/// 一帧产物(rgba8 紧凑字节 + 诊断面)。
pub struct FramePixels {
    pub width: u32,
    pub height: u32,
    pub rgba8: Vec<u8>,
    pub device_name: String,
    pub draws: usize,
    pub truncated: bool,
    pub nonzero: usize,
    /// 本帧实际绘制的三角形总数(cube=12/实体;真实网格 = 其 triangle_count)。
    pub triangles: usize,
    /// 因网格解析失败/超上限而回退 cube 的实体数(诚实诊断面)。
    pub mesh_fallbacks: usize,
    /// 本帧使用的不同网格类数(不含内置 cube)。
    pub mesh_classes: usize,
    /// 本帧会话是否直渲进 D3D12 共享纹理(F1 wave.3 零拷贝档证据面)。
    pub imported: bool,
}

impl FramePixels {
    pub fn pixels_b64(&self) -> String {
        base64::engine::general_purpose::STANDARD.encode(&self.rgba8)
    }
}

// ─────────────────────────── 测试(host 腿恒跑;device 腿见 tests/f1_viewport.rs) ───────────────────────────

/// Called inside the shared RPC fixture OnceLock, before its only env write.
/// PNG decoding and SpriteRenderInfo below stay entirely on the CPU.
/// (Stage 3:自 tests 模块原样移出——tests 模块只在 backend-rurix 下编译,rpc 测试夹具在两种 feature 集下都要它。)
#[cfg(test)]
pub(crate) fn write_sprite_variant_fixture(root: &std::path::Path) {
    use base64::Engine;
    let dir=root.join("Content/Sprites");
    std::fs::create_dir_all(&dir).unwrap();
    for (name,png) in [
        ("a","iVBORw0KGgoAAAANSUhEUgAAAAgAAAAECAYAAACzzX7wAAAAEklEQVR4nGO4I8f1Hx9moL0CAJTkQGFPOmAjAAAAAElFTkSuQmCC"),
        ("b","iVBORw0KGgoAAAANSUhEUgAAAAQAAAAICAYAAADeM14FAAAAEklEQVR4nGPgarrTgIwZBkYAABm4PQFOXeZLAAAAAElFTkSuQmCC"),
    ] {
        std::fs::write(dir.join(format!("variant-{name}.png")),base64::engine::general_purpose::STANDARD.decode(png).unwrap()).unwrap();
        std::fs::write(dir.join(format!("variant-{name}.png.meta")),format!("guid: test-v5-texture-{name}\ntype: texture\nimporter: texture\nbuild_state: current\n")).unwrap();
        let mut frames=serde_json::Map::new();
        for i in 0..128 {
            let frame=match (name,i) {
                ("a",127)=>serde_json::json!({"bbox":[2,0,6,4],"pivot":[0.25,0.75]}),
                ("a",_)=>serde_json::json!({"bbox":[0,0,2,4]}),
                ("b",127)=>serde_json::json!({"bbox":[0,2,4,6],"pivot":[0.75,0.25]}),
                _=>serde_json::json!({"bbox":[0,0,4,2]}),
            };
            frames.insert(format!("frame_{i:03}"),frame);
        }
        let doc=serde_json::json!({"version":1,"texture":format!("test-v5-texture-{name}"),
            "pivot":if name=="a"{[0.5,1.0]}else{[0.5,0.5]},"frames":frames,"clips":{}});
        std::fs::write(dir.join(format!("variant-{name}.rxsprite")),serde_json::to_vec(&doc).unwrap()).unwrap();
        std::fs::write(dir.join(format!("variant-{name}.rxsprite.meta")),format!("guid: test-v5-sprite-{name}\ntype: sprite\nimporter: sprite\nbuild_state: current\n")).unwrap();
    }
}

#[cfg(all(test, feature = "backend-rurix"))]
pub(crate) mod tests {
    use super::*;
    use crate::render_core::math::{v3_norm, v3_sub, V3};
    use super::rurix::*;

    #[test]
    fn sprite_variants_cpu_frames_pivots_legacy_and_stable_inventory() {
        crate::rpc::tests::test_project_root();
        let mut c=forge_scene::Component::new("Sprite",serde_json::json!({
            "spriteVariants":["test-v5-sprite-a","test-v5-sprite-b"],"variantStride":128,
            "frame":0,"chromaKey":"none","blendMode":"alpha","pixelsPerUnit":256}));
        let cases=[
            (0,[8,4],[0.,0.,0.25,1.],[2.,4.],[0.5,1.]),
            (127,[8,4],[0.25,0.,0.75,1.],[6.,4.],[0.25,0.75]),
            (128,[4,8],[0.,0.,1.,0.25],[4.,2.],[0.5,0.5]),
            (255,[4,8],[0.,0.25,1.,0.75],[4.,6.],[0.75,0.25]),
        ];
        for (packed,size,uv,frame_px,pivot) in cases {
            c.props["frame"]=serde_json::json!(packed);
            let info=resolve_sprite_render(&c).expect("valid packed family/frame");
            assert_eq!([info.tex.w,info.tex.h],size);
            assert_eq!(info.uv_rect,uv,"packed frame {packed}");
            assert_eq!(info.frame_px,frame_px);
            assert_eq!(info.pivot,pivot);
        }
        for stride in [0.,-1.,0.5,128.5] {
            c.props["variantStride"]=serde_json::json!(stride);
            assert!(resolve_sprite_render(&c).is_none(),"invalid stride must fail closed");
        }
        c.props["variantStride"]=serde_json::json!(128);
        c.props["frame"]=serde_json::json!(256);
        assert!(resolve_sprite_render(&c).is_none(),"out-of-range family must not draw another family");
        let mut legacy=c.clone();
        legacy.props["spriteVariants"]=serde_json::json!([]);
        legacy.props["sprite"]=serde_json::json!("test-v5-sprite-a");
        legacy.props["frame"]=serde_json::json!(127);
        legacy.props["variantStride"]=serde_json::json!(0);
        assert_eq!(resolve_sprite_render(&legacy).unwrap().pivot,[0.25,0.75]);

        let mut scene=Scene::with_mode("variant-inventory",forge_scene::SCENE_MODE_2D);
        c.props["frame"]=serde_json::json!(128);
        scene.entities.push(forge_scene::Entity { entity_guid: None,id:1,name:"variant".into(),transform:Transform::default(),components:vec![c]});
        let inventory=stable_rgba_inventory(&scene).unwrap();
        assert_eq!(inventory.iter().map(|t|[t.w,t.h]).collect::<Vec<_>>(),vec![[8,4],[4,8]],"declared order must precede current family");
        for packed in [0,127,255,128,0] {
            scene.entities[0].components[0].props["frame"]=serde_json::json!(packed);
            let next=stable_rgba_inventory(&scene).unwrap();
            assert_eq!(next.len(),inventory.len());
            assert!(next.iter().zip(&inventory).all(|(a,b)|std::ptr::eq(*a,*b)),"changing current family must not reorder resident textures");
        }
    }

    #[test]
    fn sprite_compositing_defaults_preserve_legacy_and_rgba_is_explicit() {
        let mut entity = forge_scene::Entity { entity_guid: None, id:1, name:"test".into(), transform:Transform::default(),
            components:vec![forge_scene::Component::new("Sprite",serde_json::json!({}))] };
        assert_eq!(sprite_blend(&entity),rex::BlendMode::Opaque);
        assert_eq!(sprite_compositing(&entity),[1.,0.,0.,0.]);
        entity.components[0].props=serde_json::json!({"chromaKey":"none","blendMode":"alpha"});
        assert_eq!(sprite_blend(&entity),rex::BlendMode::Alpha);
        assert_eq!(sprite_compositing(&entity),[0.,1.,0.,0.]);
    }

    #[test]
    #[ignore = "requires real Vulkan; checks the shared-texture pack shader after blending"]
    fn blended_frame_pack_real_gpu() {
        let vs=compile_wgsl(r#"@vertex fn main(@builtin(vertex_index)i:u32)->@builtin(position) vec4<f32>{
            var p=array<vec2<f32>,3>(vec2<f32>(-1.0,-1.0),vec2<f32>(3.0,-1.0),vec2<f32>(-1.0,3.0));
            return vec4<f32>(p[i],0.0,1.0);}"#,"pack-proof-vs").unwrap();
        let fs=compile_wgsl(r#"@fragment fn main()->@location(0) vec4<f32>{return vec4<f32>(0.25,0.5,0.75,0.5);}"#,"pack-proof-fs").unwrap();
        let (width,height,row_words)=(19u32,11u32,64u32);
        let resources=[rex::ResourceDesc::Texture(rex::TextureDesc{width,height,format:rex::TexFormat::Rgba8Unorm,
            usage:rex::TextureUsage{color:true,storage:true,..Default::default()},data:None}),
            rex::ResourceDesc::Buffer(rex::BufferDesc{size:(row_words*height*4)as u64,
                usage:rex::BufferUsage{storage:true,..Default::default()},data:None,device_local:true})];
        let pc:Vec<u8>=[width,height,row_words,0].into_iter().flat_map(u32::to_le_bytes).collect();
        let passes=[rex::Pass::Raster(rex::RasterPass{blend:rex::BlendMode::Alpha,name:"blended-pack-source",
            vs_spirv:vs,fs_spirv:fs,vertex:rex::VertexData::Pull,
            draw:rex::DrawSpec::Direct{vertex_count:3,instance_count:1,first_vertex:0,first_instance:0},
            colors:vec![rex::ColorAttachmentRef{res:0,clear:Some([0.1,0.2,0.3,1.])}],depth:None,viewport:None,
            bindings:Default::default(),conservative:None}),
            rex::Pass::Compute(rex::ComputePass{name:"forge_viewport_pack",spirv:pack_shader_bytes().unwrap(),entry:None,
                dispatch:rex::DispatchSpec::Direct([width.div_ceil(8),height.div_ceil(8),1]),
                bindings:rex::Bindings{storage_buffers:vec![1],storage_images:vec![0],push_constants:pc,..Default::default()}})];
        let raster_barriers=[(0,rex::TargetState::ColorAttachmentWrite)];
        let pack_barriers=[(0,rex::TargetState::StorageImageReadWrite),(1,rex::TargetState::StorageReadWrite)];
        let barriers:[&[(u32,rex::TargetState)];2]=[&raster_barriers,&pack_barriers];
        let readbacks=[rex::Readback::Texture{res:0},rex::Readback::Buffer{res:1,offset:0,size:(row_words*height*4)as u64}];
        let mut session=rex::DeviceFrameSession::new(&resources,&passes,&barriers,&readbacks,2).unwrap();
        let update=rex::FrameUpdate{readback_subset:Some(vec![0,1]),..Default::default()};
        let provenance=session.next_provenance_with_update(&update).unwrap();
        let output=session.execute_with_frame_update(&provenance,&update).unwrap();
        for y in 0..height as usize { for x in 0..width as usize {
            let image=(y*width as usize+x)*4;let packed=(y*row_words as usize+x)*4;
            assert_eq!(&output.readbacks[0][image..image+4],&output.readbacks[1][packed..packed+4],"RGBA pack drift at ({x},{y})");
        }}
        assert_eq!(output.readbacks[0][3],255,"composited alpha must survive pack");
        let record=serde_json::json!({"realGpu":true,"device":rex::probe_device_caps().unwrap().device_name,
            "source":"alpha-blended Rgba8Unorm attachment","packShader":"forge_viewport_pack",
            "width":width,"height":height,"rowPitchBytes":row_words*4,"rgbaBitExact":true,
            "scope":"GPU compute pack compatibility; does not claim a separate D3D12 handle-lifecycle test"});
        if let Some(dir)=std::env::var_os("FORGE_BLEND_EVIDENCE_DIR") {
            let dir=std::path::PathBuf::from(dir);std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("blended-pack-evidence.json"),serde_json::to_vec_pretty(&record).unwrap()).unwrap();
        }
        println!("{record}");
    }

    #[test]
    #[ignore = "requires real Vulkan and FORGE_PROJECT_ROOT pointing at the V2 game"]
    fn pooled_rgba_scene_real_gpu_no_rebuild() {
        let root=crate::rpc::project_root();
        let mut scene=Scene::from_json(&std::fs::read_to_string(root.join("Content/Scenes/Main.rxscene")).unwrap()).unwrap();
        assert!(stable_rgba_inventory(&scene).is_some(),"test scene must use explicit RGBA Sprite modes");
        let camera=EditorCamera {target:[0.,0.,0.],yaw_deg:0.,pitch_deg:0.,ortho:true,ortho_half_h:7.,..Default::default()};
        let terrain=scene.entities.iter_mut().find(|e|e.name=="CS_Terrain0").unwrap();
        terrain.transform.translation=[0.,0.,0.];
        invalidate_assets();
        render_scene_frame(&scene,&camera,None,1280,720,true,false,None).unwrap();
        let mut timings=Vec::new();let mut max_draws=0;let mut last_frame=Vec::new();
        for frame in 0..60usize {
            for i in 0..64usize {
                let enemy=scene.entities.iter_mut().find(|e|e.name==format!("CS_Enemy{i}")).unwrap();
                enemy.transform.translation=if i<=frame {[(i%12)as f32*1.6-8.8,(i/12)as f32*1.5-3.,0.]}else{[-100.,-100.,0.]};
                enemy.transform.scale=[0.7,0.7,1.];
            }
            for i in 0..24usize {
                let actor=scene.entities.iter_mut().find(|e|e.name==format!("CS_Actor{i}_{}",i%4+1)).unwrap();
                actor.transform.translation=[(i%8)as f32*2.5-8.75,(i/8)as f32*3.-3.,0.];
            }
            for i in 0..32usize {
                let shot=scene.entities.iter_mut().find(|e|e.name==format!("CS_Pulse{i}")).unwrap();
                shot.transform.translation=if frame%2==0 {[(i%16)as f32-7.5,(i/16)as f32*2.-1.,0.]}else{[-100.,-100.,0.]};
            }
            for i in 0..8usize {
                let gpu=scene.entities.iter_mut().find(|e|e.name==format!("CS_GPUVisual{i}")).unwrap();
                gpu.transform.translation=[-11.,4.9-i as f32*1.4,0.];
            }
            let effect=scene.entities.iter_mut().find(|e|e.name=="CS_VFXOverlay0").unwrap();
            effect.transform.translation=if frame%3==0 {[2.,1.,0.]}else{[-100.,-100.,0.]};
            let time=std::time::Instant::now();
            let result=render_scene_frame(&scene,&camera,None,1280,720,true,false,None).unwrap();
            timings.push(time.elapsed().as_secs_f64()*1000.);
            assert!(!REBUILD_FLAG.load(std::sync::atomic::Ordering::Relaxed),"pooled visibility must not rebuild on frame {frame}");
            assert!(!result.truncated);
            max_draws=max_draws.max(result.draws);last_frame=result.rgba8;
        }
        let mean=timings.iter().sum::<f64>()/timings.len()as f64;
        timings.sort_by(f64::total_cmp);let p95=timings[56];
        let record=serde_json::json!({"realGpu":true,"device":rex::probe_device_caps().unwrap().device_name,
            "resolution":[1280,720],"frames":60,"poolVisibilityChanges":true,"rebuildsAfterWarmup":0,
            "reservedAlphaSlots":192,"reservedAdditiveSlots":64,"maxDrawnSprites":max_draws,
            "meanRenderReadbackMs":mean,"p95RenderReadbackMs":p95,"scope":"renderer only; excludes game update and WebSocket delivery"});
        if let Some(dir)=std::env::var_os("FORGE_BLEND_EVIDENCE_DIR") {
            let dir=std::path::PathBuf::from(dir);std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("pooled-rgba-performance.json"),serde_json::to_vec_pretty(&record).unwrap()).unwrap();
            std::fs::write(dir.join("pooled-rgba-frame.rgba"),last_frame).unwrap();
        }
        println!("{record}");
        assert!(p95<33.4,"renderer alone must leave a 30fps frame budget: p95={p95:.2}ms");
    }

    /// Actual textured Sprite shader + actual Rurix fixed-function PSOs. An
    /// alpha return with blending disabled or a blend-agnostic PSO cache fails.
    #[test]
    #[ignore = "requires real Vulkan; no software fallback"]
    fn sprite_compositing_real_gpu() {
        const SIZE:u32=64;
        let make_tex=|pixels:Vec<u8>| -> &'static TexGpu {
            Box::leak(Box::new(TexGpu { w:2,h:1,rgba:Box::leak(pixels.into_boxed_slice()) }))
        };
        let blue=make_tex(vec![0,0,255,255,0,0,255,255]);
        // Deliberately magenta: chromaKey=none must retain purple V2 effects.
        let purple=make_tex(vec![255,0,255,0,255,0,255,128]);
        let green=make_tex(vec![0,255,0,128,0,255,0,128]);
        let mut results=Vec::new();
        for (label,mode,legacy,tint,add_green,expected) in [
            ("alpha",rex::BlendMode::Alpha,false,1.0,false,[128u8,0,255,255]),
            ("alpha-additive",rex::BlendMode::Alpha,false,1.0,true,[128,128,255,255]),
            ("alpha-tint",rex::BlendMode::Alpha,false,0.5,false,[64,0,255,255]),
            ("legacy-chroma",rex::BlendMode::Opaque,true,1.0,false,[0,0,255,255]),
            ("none-opaque",rex::BlendMode::Opaque,false,1.0,false,[255,0,255,255]),
        ] {
            let textures=if add_green {vec![Some(blue),Some(purple),Some(green)]} else {vec![Some(blue),Some(purple)]};
            let modes=if add_green {vec![rex::BlendMode::Opaque,mode,rex::BlendMode::Additive]} else {vec![rex::BlendMode::Opaque,mode]};
            let n=textures.len();
            let mut renderer=build_session_with(SIZE,SIZE,None,n,&vec![None;n],&textures,&modes,&[],0).unwrap();
            let identity=[[1.,0.,0.,0.],[0.,1.,0.,0.],[0.,0.,1.,0.],[0.,0.,0.,1.]];
            let model=trs_model(&Transform {scale:[2.,2.,1.], ..Transform::default()});
            let mut update=rex::FrameUpdate {buffer_uploads:vec![(rex::StableResourceId(2),0,m4_col_bytes(identity).to_vec())],
                readback_subset:Some(vec![0]),..Default::default()};
            for slot in 0..n {
                let mut pc=m4_col_bytes(model).to_vec();
                let color=[1.0f32,1.,1.,if slot==1 {tint} else {1.}];
                for f in color {pc.extend_from_slice(&f.to_le_bytes());}
                pc.extend_from_slice(&2u32.to_le_bytes());pc.extend_from_slice(&1u32.to_le_bytes());
                for f in [0.0f32,0.] {pc.extend_from_slice(&f.to_le_bytes());}
                for f in FULL_UV_RECT {pc.extend_from_slice(&f.to_le_bytes());}
                let flags=[if slot==1 && legacy {1.0f32}else{0.},if modes[slot]==rex::BlendMode::Opaque{0.}else{1.},0.,0.];
                for f in flags {pc.extend_from_slice(&f.to_le_bytes());}
                assert_eq!(pc.len(),SPRITE_PC_LEN);
                update.push_constant_overrides.push((slot as u32,pc));
            }
            let provenance=renderer.session.next_provenance_with_update(&update).unwrap();
            let output=renderer.session.execute_with_frame_update(&provenance,&update).unwrap();
            let rgba=&output.readbacks[0];
            let sample=|x:usize| {let i=((SIZE as usize/2)*SIZE as usize+x)*4;[rgba[i],rgba[i+1],rgba[i+2],rgba[i+3]]};
            let left=sample(16);let right=sample(48);
            for c in 0..4 { assert!((right[c]as i16-expected[c]as i16).abs()<=2,"{label}: actual {right:?}, expected {expected:?}"); }
            let left_expected=if add_green {[0u8,128,255,255]}else{[0,0,255,255]};
            for c in 0..4 {assert!((left[c]as i16-left_expected[c]as i16).abs()<=2,"transparent texel changed background in {label}: {left:?}");}
            if let Some(dir)=std::env::var_os("FORGE_BLEND_EVIDENCE_DIR") {
                let dir=std::path::PathBuf::from(dir);std::fs::create_dir_all(&dir).unwrap();
                std::fs::write(dir.join(format!("sprite-{label}.rgba")),rgba).unwrap();
            }
            results.push(serde_json::json!({"case":label,"left":left,"right":right,"expectedRight":expected,"width":SIZE,"height":SIZE}));
        }
        let evidence=serde_json::json!({"realGpu":true,"device":rex::probe_device_caps().unwrap().device_name,
            "pipelineBlending":true,"preservesStraightAlpha":true,"purpleChromaOptOut":true,
            "blendCacheKeysDistinct":true,"cases":results});
        if let Some(dir)=std::env::var_os("FORGE_BLEND_EVIDENCE_DIR") {
            std::fs::write(std::path::PathBuf::from(dir).join("sprite-blend-evidence.json"),serde_json::to_vec_pretty(&evidence).unwrap()).unwrap();
        }
        println!("{evidence}");
    }

    #[test]
    fn v2_draw_tiers_cover_every_legal_entity_without_truncation() {
        for n in [24, 25, 64, 97, 128, 129, 156, 192, 193, 256] {
            assert!(slot_tier(n) >= n, "draw tier must cover {n} scene entities");
            assert!(slot_tier(n) <= MAX_DRAW_SLOTS);
        }
        assert_eq!(slot_tier(257), MAX_DRAW_SLOTS);
    }

    fn cam() -> EditorCamera {
        EditorCamera::default()
    }

    #[test]
    fn camera_center_ray_points_at_target() {
        let c = cam();
        let (o, d) = c.ray(0.0, 0.0, 16.0 / 9.0);
        let to_target = v3_norm(v3_sub(c.target, o));
        for i in 0..3 {
            assert!((d[i] - to_target[i]).abs() < 1e-5, "中心射线应指向 target");
        }
    }

    #[test]
    fn obb_hit_miss_and_nearest_order() {
        let tr = Transform {
            translation: [0.0, 0.0, -5.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0, 1.0, 1.0],
        };
        // 正面命中:t ≈ 4.5(前面 z=-4.5)。
        let t = ray_unit_cube([0.0, 0.0, 0.0], [0.0, 0.0, -1.0], &tr).expect("应命中");
        assert!((t - 4.5).abs() < 1e-4, "命中参数错:{t}");
        // 偏离未命中。
        assert!(ray_unit_cube([3.0, 0.0, 0.0], [0.0, 0.0, -1.0], &tr).is_none());
        // 背后命中拒绝(tmax<0)。
        assert!(ray_unit_cube([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], &tr).is_none());
        // 缩放×2 后前面 z=-4。
        let tr2 = Transform { scale: [2.0, 2.0, 2.0], ..tr };
        let t2 = ray_unit_cube([0.0, 0.0, 0.0], [0.0, 0.0, -1.0], &tr2).expect("应命中");
        assert!((t2 - 4.0).abs() < 1e-4, "缩放后命中参数错:{t2}");
    }

    #[test]
    fn pick_returns_nearest_entity() {
        let mut scene = Scene::new("t");
        let mk = |id: u64, x: f32, z: f32| forge_scene::Entity { entity_guid: None,
            id,
            name: format!("e{id}"),
            transform: Transform {
                translation: [x, 0.0, z],
                rotation: [0.0, 0.0, 0.0, 1.0],
                scale: [1.0, 1.0, 1.0],
            },
            components: vec![forge_scene::Component::new(
                "MeshRenderer",
                serde_json::json!({"mesh": "cube", "material": "m"}),
            )],
        };
        // 近者(z=-4)应遮挡远者(z=-8);x=0 位于画面中心附近。
        scene.entities.push(mk(7, 0.0, -8.0));
        scene.entities.push(mk(3, 0.0, -4.0));
        let mut c = cam();
        c.target = [0.0, 0.0, -4.0];
        c.dist = 4.0;
        c.pitch_deg = 0.0;
        c.yaw_deg = 0.0; // 眼在 target 后 +z 向(z=0),看向 -z
        let hit = pick_entity(&scene, &c, 320.0, 180.0, 640, 360).expect("应命中");
        assert_eq!(hit.0, 3, "应取近者");
    }

    #[test]
    fn mesh_and_shader_bytes_wellformed() {
        assert_eq!(cube_mesh_bytes().len(), 36 * 24);
        let (vs, fs) = shader_bytes().expect("着色器编译应成功");
        // SPIR-V magic 0x07230203 小端。
        assert_eq!(&vs[..4], &[0x03, 0x02, 0x23, 0x07]);
        assert_eq!(&fs[..4], &[0x03, 0x02, 0x23, 0x07]);
    }

    #[test]
    fn tex_shader_bytes_wellformed() {
        let (vs, fs) = shader_tex_bytes().expect("精灵着色器编译应成功");
        assert_eq!(&vs[..4], &[0x03, 0x02, 0x23, 0x07]);
        assert_eq!(&fs[..4], &[0x03, 0x02, 0x23, 0x07]);
    }

    #[test]
    fn ortho_view_proj_maps_extents_to_ndc_edges() {
        // F-GAME-3:正交相机(yaw=0,pitch=0,眼在 target 正 +z)下半高=orthoSize,
        // target 正上方 half_h 处应落在 NDC y=+1(显示面顶行),右侧 half_w 落在 x=+1。
        let mut c = cam();
        c.ortho = true;
        c.ortho_half_h = 5.0;
        c.yaw_deg = 0.0;
        c.pitch_deg = 0.0;
        c.target = [0.0, 0.0, 0.0];
        c.dist = 10.0;
        let aspect = 16.0 / 9.0;
        let vp = c.view_proj(aspect);
        let xform = |p: V3| -> V3 {
            let v = [
                vp[0][0] * p[0] + vp[0][1] * p[1] + vp[0][2] * p[2] + vp[0][3],
                vp[1][0] * p[0] + vp[1][1] * p[1] + vp[1][2] * p[2] + vp[1][3],
                vp[2][0] * p[0] + vp[2][1] * p[1] + vp[2][2] * p[2] + vp[2][3],
            ];
            let w = vp[3][0] * p[0] + vp[3][1] * p[1] + vp[3][2] * p[2] + vp[3][3];
            [v[0] / w, v[1] / w, v[2] / w]
        };
        let top = xform([0.0, 5.0, 0.0]);
        assert!((top[1] - 1.0).abs() < 1e-4, "半高点应达 NDC 顶:{top:?}");
        let right = xform([5.0 * aspect, 0.0, 0.0]);
        assert!((right[0] - 1.0).abs() < 1e-4, "半宽点应达 NDC 右:{right:?}");
        let center = xform([0.0, 0.0, 0.0]);
        assert!(center[0].abs() < 1e-5 && center[1].abs() < 1e-5, "target 应居中:{center:?}");
        // 正交无透视形变:同 y 不同 z 的两点 NDC xy 相同。
        let a = xform([1.0, 2.0, -3.0]);
        let b = xform([1.0, 2.0, -8.0]);
        assert!((a[0] - b[0]).abs() < 1e-5 && (a[1] - b[1]).abs() < 1e-5, "正交下深度不改 xy:{a:?} vs {b:?}");
    }

    #[test]
    fn ortho_ray_is_parallel() {
        // F-GAME-3:正交射线互相平行(dir 恒为前向),原点随屏幕位置平移。
        let mut c = cam();
        c.ortho = true;
        c.ortho_half_h = 5.0;
        c.yaw_deg = 0.0;
        c.pitch_deg = 0.0;
        c.target = [0.0, 0.0, 0.0];
        c.dist = 10.0;
        let aspect = 16.0 / 9.0;
        let (o0, d0) = c.ray(0.0, 0.0, aspect);
        let (o1, d1) = c.ray(1.0, 1.0, aspect);
        for i in 0..3 {
            assert!((d0[i] - d1[i]).abs() < 1e-6, "正交射线方向须一致");
            assert!((d0[i] - [0.0, 0.0, -1.0][i]).abs() < 1e-6, "yaw0/pitch0 前向须为 -z");
        }
        // 右上角射线的原点应偏移 (+half_w, +half_h) 于眼位 xy。
        assert!((o1[0] - (o0[0] + 5.0 * aspect)).abs() < 1e-4, "x 偏移 = 半宽");
        assert!((o1[1] - (o0[1] + 5.0)).abs() < 1e-4, "y 偏移 = 半高");
    }

    #[test]
    fn scene_camera_orthographic_branch() {
        // F-GAME-3:Camera 组件 projection=orthographic → PIE 正交;缺省仍透视。
        let mk_scene = |proj: Option<&str>| {
            let mut s = Scene::new("t");
            let props = match proj {
                Some(p) => serde_json::json!({"projection": p, "orthoSize": 4.0, "fov": 60.0, "near": 0.1, "far": 100.0}),
                None => serde_json::json!({"fov": 60.0, "near": 0.1, "far": 100.0}),
            };
            s.entities.push(forge_scene::Entity { entity_guid: None,
                id: 1,
                name: "cam".into(),
                transform: Transform {
                    translation: [0.0, 0.0, 10.0],
                    rotation: [0.0, 0.0, 0.0, 1.0],
                    scale: [1.0; 3],
                },
                components: vec![forge_scene::Component::new("Camera", props)],
            });
            s
        };
        let ortho = scene_camera_view_proj(&mk_scene(Some("orthographic")), 16.0 / 9.0).expect("有相机");
        // 正交矩阵 m[3][2]=0(无透视除法项),m[1][1] 经 y-flip 后为 +1/half_h。
        assert!(ortho[3][2].abs() < 1e-7, "正交无 w 透视项:{ortho:?}");
        assert!((ortho[1][1] - 0.25).abs() < 1e-5, "orthoSize=4 → 1/half_h=0.25:{}", ortho[1][1]);
        let persp = scene_camera_view_proj(&mk_scene(None), 16.0 / 9.0).expect("有相机");
        assert!((persp[3][2] + 1.0).abs() < 1e-5, "缺省须为透视(m[3][2]=-1):{persp:?}");
        assert!(scene_camera_view_proj(&Scene::new("空"), 1.0).is_none(), "无相机实体 → None");
    }

    fn ortho_cam_scene(ortho_size: f32) -> Scene {
        let mut s = Scene::with_mode("t", "2d");
        s.entities.push(forge_scene::Entity { entity_guid: None,
            id: 1,
            name: "cam".into(),
            transform: Transform {
                translation: [0.0, 0.0, 10.0],
                rotation: [0.0, 0.0, 0.0, 1.0],
                scale: [1.0; 3],
            },
            components: vec![forge_scene::Component::new(
                "Camera",
                serde_json::json!({"projection": "orthographic", "orthoSize": ortho_size, "fov": 60.0, "near": 0.1, "far": 100.0}),
            )],
        });
        s
    }

    /// 指针反投影腿:正交场景相机下 NDC 角点 → 半宽/半高偏移的平行射线(朝 -z),
    /// 屏幕上方(ny=+1)对应世界 +y——与画面(HUD 在上)一致。
    #[test]
    fn scene_camera_ray_orthographic_maps_ndc_to_world_plane() {
        let s = ortho_cam_scene(6.2);
        let aspect = 16.0 / 9.0;
        let (o, d) = scene_camera_ray(&s, 0.0, 0.0, aspect).expect("有相机");
        assert!((o[0]).abs() < 1e-5 && (o[1]).abs() < 1e-5 && (o[2] - 10.0).abs() < 1e-5);
        assert!((d[2] + 1.0).abs() < 1e-5, "正交射线沿 -z:{d:?}");
        let (o1, _) = scene_camera_ray(&s, 1.0, 1.0, aspect).expect("有相机");
        assert!((o1[0] - 6.2 * aspect).abs() < 1e-3, "nx=1 → x=半宽:{}", o1[0]);
        assert!((o1[1] - 6.2).abs() < 1e-4, "ny=1 → y=半高(屏幕上=世界上):{}", o1[1]);
        assert!(scene_camera_ray(&Scene::new("空"), 0.0, 0.0, 1.0).is_none(), "无相机 → None");
    }

    /// 屏外裁剪:停在 y=-60 的池子精灵四角全在裁剪空间下方 → 屏外;屏内精灵不裁;
    /// 非 Sprite 实体恒不裁(3D 网格行为不变)。
    #[test]
    fn sprite_offscreen_culls_parked_pool_entities_only() {
        let s = ortho_cam_scene(6.2);
        let vp = scene_camera_view_proj(&s, 16.0 / 9.0).expect("有相机");
        let sprite = |id: u64, y: f32| forge_scene::Entity { entity_guid: None,
            id,
            name: format!("s{id}"),
            transform: Transform {
                translation: [0.0, y, 0.0],
                rotation: [0.0, 0.0, 0.0, 1.0],
                scale: [1.0; 3],
            },
            components: vec![forge_scene::Component::new(
                "Sprite",
                serde_json::json!({"texture": "", "sprite": "", "pixelsPerUnit": 100.0}),
            )],
        };
        assert!(sprite_offscreen(&sprite(1, -60.0), &vp), "y=-60 的池子精灵须判屏外");
        assert!(!sprite_offscreen(&sprite(2, 0.0), &vp), "屏中精灵不裁");
        assert!(!sprite_offscreen(&sprite(3, 6.0), &vp), "贴边(半高 6.2 内)精灵不裁");
        assert!(!sprite_offscreen(&sprite(5, -12.0), &vp), "刚出屏(3 倍边距内)不裁,免会话重建抖动");
        let cube = forge_scene::Entity { entity_guid: None,
            id: 4,
            name: "cube".into(),
            transform: Transform {
                translation: [0.0, -60.0, 0.0],
                rotation: [0.0, 0.0, 0.0, 1.0],
                scale: [1.0; 3],
            },
            components: vec![forge_scene::Component::new("MeshRenderer", serde_json::json!({}))],
        };
        assert!(!sprite_offscreen(&cube, &vp), "非 Sprite 实体不裁");
    }
}
