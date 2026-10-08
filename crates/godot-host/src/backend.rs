//! GodotBackend(02 §4.1):forge 线程([rpc] / [wsr])侧的 RenderBackend + PipelinedRender。
//! 只碰纯 Rust 的 HostFrameSink(两条通道的 SubmitBox / FrameBus)与本文件的 Shared;RS 调用全在 [gmain](host.rs)。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use engine_host::{
    BackendInfo, Capabilities, Channel, ControlMsg, Coverage, FrameBus, FrameChannels, FramePath, FrameRequest,
    HostFrameSink, LegSet, MaxDraws, PipelinedRender, RenderBackend, RenderList, StatsCaps,
};

fn plock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// [gmain] 与 forge 线程共享的状态(全部叶子锁,I3)。
#[derive(Default)]
pub struct Shared {
    pub sink: HostFrameSink,
    ready: AtomicBool,
    channels: Mutex<FrameChannels>,
    device_name: Mutex<Option<String>>,
    /// [gmain] 已处理的 ShareDetach 个数(detach_share 等它前进)。
    detached: Mutex<u64>,
    detached_cv: Condvar,
    asset_generation: AtomicU64,
    /// [gmain] 的 frame_post_draw 计数(L2 回调据此算延迟)。
    frame_no: AtomicU64,
}

impl Shared {
    pub fn set_frame_no(&self, n: u64) {
        self.frame_no.store(n, Ordering::SeqCst);
    }

    pub fn frame_no(&self) -> u64 {
        self.frame_no.load(Ordering::SeqCst)
    }

    pub fn set_ready(&self) {
        self.ready.store(true, Ordering::SeqCst);
    }

    pub fn set_device_name(&self, name: &str) {
        let mut g = plock(&self.device_name);
        if g.is_none() {
            *g = Some(name.to_string());
        }
    }

    pub fn update_channels(&self, f: impl FnOnce(&mut FrameChannels)) {
        f(&mut plock(&self.channels));
    }

    /// [gmain]:处理完一条 ShareDetach 后调用。
    pub fn detach_done(&self) {
        *plock(&self.detached) += 1;
        self.detached_cv.notify_all();
    }
}

pub struct GodotBackend {
    info: BackendInfo,
    caps: Capabilities,
    shared: Arc<Shared>,
}

impl GodotBackend {
    /// zero_copy = L1 可用(D3D12 + RD + LUID 与缺省 adapter 相同,01 X1)。
    pub fn new(info: BackendInfo, zero_copy: bool, shared: Arc<Shared>) -> Self {
        let caps = Capabilities {
            // 三条渲染路径全开；尚未验收的映射及效果差异仍在 coverage 里如实列出。
            legs: LegSet { sprite_mesh: true, model: true, sentinels_v6: true },
            pipelined: true,
            preview: true,
            // 与 rurix 同一判据(FORGE_GPU_PARTICLES = on | 1);基本映射见 particles.rs。
            particles: engine_host::particles_enabled(),
            cpu_rgba8: true,
            shared_d3d12: cfg!(windows),
            zero_copy,
            stats: StatsCaps { nonzero: true, triangles: true, truncated: false, mesh_fallbacks: true, mesh_classes: false },
            max_draws: MaxDraws { sprite_mesh: None, model: None, sentinels_v6: None },
        };
        GodotBackend { info, caps, shared }
    }
}

impl RenderBackend for GodotBackend {
    fn info(&self) -> &BackendInfo {
        &self.info
    }

    fn capabilities(&self) -> &Capabilities {
        &self.caps
    }

    fn path(&self) -> FramePath<'_> {
        FramePath::Pipelined(self)
    }

    /// asset.reload:[gmain] 丢弃旧代次建的 RS 网格(新清单的 asset_generation 也会触发全量重建)。
    fn invalidate_assets(&self) {
        let generation = self.shared.asset_generation.fetch_add(1, Ordering::SeqCst) + 1;
        self.shared.sink.main_box.control(ControlMsg::InvalidateAssets { generation });
    }

    fn device_name(&self) -> Option<String> {
        plock(&self.shared.device_name).clone()
    }

    fn frame_channels(&self) -> Option<FrameChannels> {
        Some(plock(&self.shared.channels).clone())
    }

    /// 已声明的腿里仍跳过 / 打折的内容,带原因(render.capabilities.coverage);unsupported = 本渲染方式按 01 §7 K1
    /// 忽略的 Stage 5 特性(字段照收、不报错,画面上没有效果)。
    fn coverage(&self) -> Option<Coverage> {
        use engine_host::RenderMethod::GlCompatibility;
        let mut skipped = vec![
            "V6 draws 口径:rurix 恒报 4(每个 pass 一次),Godot 报 RS 实例数(MultiMesh + 地形 + 精灵);粒子叠加层与体积类实例与 rurix 一样不计入 draws",
            "VoxelGI:只能经节点烘焙(VoxelGI::bake 要 MeshInstance3D 节点树),不进 schema;GI 用 Environment.sdfgi*",
            "Compositor:RD 的自定义后处理扩展口,forge 没有对应语义,不进 schema",
            "时间性效果(SDFGI 收敛、自动曝光、体积雾时间重投影、TAA、反射探针 updateMode=once 的逐帧渲染)需要连续多帧;单次取帧看到的是当前收敛程度,同一场景前几帧之间不保证逐字节相同",
            "Environment.adjustmentColorCorrection:Texture2D 按 1D LUT 用(Godot 语义),3D LUT 本版不接",
        ];
        if self.info.method == Some(GlCompatibility) {
            skipped.push(
                "Light.castShadow(gl_compatibility):GLES3 把投影灯放进附加 pass、各 pass 分别 tonemap 后在 sRGB 里相加,Reinhard 下亮部过曝,故不开阴影",
            );
        }
        Some(Coverage { skipped })
    }

    /// 本渲染方式按 01 §7 K1 忽略的 Stage 5 特性(g5 测试断言:这些配置下开关前后画面逐字节不变)。
    fn unsupported_features(&self) -> Vec<(&'static str, &'static str)> {
        use engine_host::RenderMethod::{GlCompatibility, Mobile};
        match self.info.method {
            Some(Mobile) => vec![
                ("Environment.ssao", "Mobile 不支持 SSAO(01 §7 K1)"),
                ("Environment.ssil", "Mobile 不支持 SSIL"),
                ("Environment.ssr", "Mobile 不支持 SSR"),
                ("Environment.sdfgi", "Mobile 不支持 SDFGI"),
                ("Environment.volumetricFog", "Mobile 不支持体积雾"),
                ("FogVolume", "依赖体积雾,Mobile 不支持"),
                ("CameraAttributes.autoExposure", "Mobile 不支持自动曝光"),
                ("RenderSettings.taa", "TAA 只有 Forward+ 支持"),
                ("RenderSettings.scaling3dMode", "Mobile 下 FSR / FSR2 不生效,回退到 bilinear"),
                ("RenderSettings.debanding", "Mobile 的 3D 视口改用 HDR 2D 缓冲(Stage 5 有意修正 2),Godot 只在非 HDR 目标上做 debanding"),
            ],
            Some(GlCompatibility) => vec![
                ("Environment.ssil", "Compatibility 不支持 SSIL"),
                ("Environment.ssr", "Compatibility 不支持 SSR"),
                ("Environment.sdfgi", "Compatibility 不支持 SDFGI"),
                ("Environment.volumetricFog", "Compatibility 不支持体积雾"),
                ("FogVolume", "依赖体积雾,Compatibility 不支持"),
                ("CameraAttributes.dof", "Compatibility 不支持 DOF"),
                ("CameraAttributes.autoExposure", "Compatibility 不支持自动曝光"),
                ("Decal", "Compatibility 不支持贴花"),
                ("ReflectionProbe", "Compatibility 下实测不生效(g5_volumes:镜面开关探针前后逐字节相同;K1 文档只说每 mesh 最多 2 个,原因未查清)"),
                ("RenderSettings.taa", "TAA 只有 Forward+ 支持"),
                ("RenderSettings.screenSpaceAA", "Compatibility 不支持 FXAA / SMAA"),
                ("RenderSettings.debanding", "Compatibility 不支持 debanding"),
                ("RenderSettings.scaling3dMode", "Compatibility 不支持 FSR / FSR2"),
            ],
            _ => Vec::new(),
        }
    }

    /// 能用、但实现 / 效果与 Forward+ 不同的特性。
    fn limited_features(&self) -> Vec<(&'static str, &'static str)> {
        let mut features = vec![
            (
                "SentinelsV6Batch.sprites",
                "四配置已验证地面图集裁切/热重载及非地面建筑图集/pivot/混合/关闭重开；单位和特效旋转、重叠排序及与 Rurix 的像素一致性仍待独立验收",
            ),
            (
                "ModelRenderer.material",
                "Godot 与 Rurix 的 BRDF 不同；缺省环境仅对全金属表面补环境项，部分金属和透明材质不保证逐像素一致",
            ),
            (
                "Sprite.sortingOrder",
                "Canvas 和透明 3D quad 已验证排序；不透明 3D quad 使用深度缓冲，sortingOrder 不改变同深度遮挡，需调整深度或使用 alpha 混合",
            ),
            (
                "Sprite.canvasEnvironment",
                "纯正交 2D Canvas 在 Environment 后合成，四配置实测不参与 Glow；Mobile 的 HDR 2D 缓冲只保证输出精度，不代表 2D Glow 已接入。需后处理的精灵应使用 3D quad 路径",
            ),
            (
                "Sprite.blendMode",
                "Forward+ 的 3D 精灵及 Mobile 的 Canvas/3D 精灵在 HDR 线性空间混合，其他已测精灵路径在编码颜色空间混合；alpha/additive 已验证有效，但不保证与 Rurix 或各渲染方式逐像素一致",
            ),
        ];
        if self.info.method == Some(engine_host::RenderMethod::GlCompatibility) {
            features.extend([
                ("Environment.ssao", "GLES3 是另一套实现(s4ao:按深度估算)，效果与 Forward+ 不同"),
                ("Environment.fogBackground", "GLES 雾背景已补偿 linear/reinhard 的颜色转换及能量；filmic/aces/agx 和零曝光沿用 Godot 原生表现"),
            ]);
        }
        features
    }
}

impl PipelinedRender for GodotBackend {
    /// 两条通道共用一个序号源(Preview 要求 seq 相等,序号全局唯一即可)。
    fn next_seq(&self) -> u64 {
        self.shared.sink.main.next_seq()
    }

    fn submit(&self, ch: Channel, list: Arc<RenderList>, req: Option<FrameRequest>) -> Result<(), String> {
        self.shared.sink.submit_box(ch).put(list, req);
        Ok(())
    }

    fn bus(&self, ch: Channel) -> &FrameBus {
        self.shared.sink.bus(ch)
    }

    fn control(&self, msg: ControlMsg) -> Result<(), String> {
        self.shared.sink.main_box.control(msg);
        Ok(())
    }

    fn ready(&self) -> bool {
        self.shared.ready.load(Ordering::SeqCst)
    }

    fn detach_share(&self, timeout: Duration) -> bool {
        let start = *plock(&self.shared.detached);
        self.shared.sink.main_box.control(ControlMsg::ShareDetach);
        let deadline = Instant::now() + timeout;
        let mut g = plock(&self.shared.detached);
        while *g == start {
            let remain = deadline.saturating_duration_since(Instant::now());
            if remain.is_zero() {
                return false;
            }
            g = self.shared.detached_cv.wait_timeout(g, remain).unwrap_or_else(|e| e.into_inner()).0;
        }
        true
    }
}
