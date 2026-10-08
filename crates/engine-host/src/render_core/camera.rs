//! 相机(02 §3.1):编辑器相机、场景相机 view_proj / 射线、模型腿 eye,以及给 Godot 腿用的 `ViewSetup`。
//! 前三者自 viewport.rs / modelrender.rs 逐字搬来。三处不对称都是现状,rurix 必须保持:
//! 场景相机 view_proj 丢 roll(ray 含 roll);场景相机用实体本地 transform、不走 Parent 链;
//! 模型腿 eye 在 PIE 时经 `modelrt::entity_world` 走 Parent 链。

use forge_scene::Scene;
use serde_json::Value;

use super::math::{
    look_at_rh, m3_apply, m4_mul, orthographic_vk, perspective_vk, quat_to_mat3, v3_add, v3_cross, v3_norm, v3_sub,
    M4, V3,
};

// ─────────────────────────── 编辑器相机 ───────────────────────────

/// 环绕式编辑器相机(07 §2:Alt+左键环绕 / 滚轮缩放 / F 聚焦 target)。
/// F-GAME-3:ortho=true 切换正交投影(2D 模式;ortho_half_h 为半高世界单位,
/// dist 仍决定眼位/近远裁剪,客户端 2D 手势把 yaw/pitch 归零得正对 XY 平面视图)。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EditorCamera {
    pub target: V3,
    pub yaw_deg: f32,
    pub pitch_deg: f32,
    pub dist: f32,
    pub fov_y_deg: f32,
    pub ortho: bool,
    pub ortho_half_h: f32,
}

impl Default for EditorCamera {
    fn default() -> Self {
        EditorCamera {
            target: [0.0, 0.5, 0.0],
            yaw_deg: 35.0,
            pitch_deg: 28.0,
            dist: 9.0,
            fov_y_deg: 50.0,
            ortho: false,
            ortho_half_h: 5.0,
        }
    }
}

impl EditorCamera {
    /// 眼位(绕 target 球坐标;yaw 绕 Y,pitch 抬升)。
    pub fn eye(&self) -> V3 {
        let yaw = self.yaw_deg.to_radians();
        let pitch = self.pitch_deg.to_radians();
        let (sp, cp) = pitch.sin_cos();
        let (sy, cy) = yaw.sin_cos();
        [
            self.target[0] + self.dist * cp * sy,
            self.target[1] + self.dist * sp,
            self.target[2] + self.dist * cp * cy,
        ]
    }

    /// 相机基 (right, up, forward)。
    fn basis(&self) -> (V3, V3, V3) {
        let eye = self.eye();
        let f = v3_norm(v3_sub(self.target, eye));
        let r = v3_norm(v3_cross(f, [0.0, 1.0, 0.0]));
        let u = v3_cross(r, f);
        (r, u, f)
    }

    /// viewProj(列向量约定;aspect = w/h)。
    /// proj Y 对角元取负:Vulkan readback/共享纹理行序底朝上,显示面(浏览器 canvas/
    /// 视频)按顶行先行呈现——投影侧统一垂直翻转后显示直立,且与 viewport_pick 的
    /// 屏幕→射线映射(yny 向上为正)同向;right 轴不动,不引入水平镜像(F-GAME-2)。
    pub fn view_proj(&self, aspect: f32) -> M4 {
        let mut proj = if self.ortho {
            orthographic_vk(self.ortho_half_h, aspect.max(1e-6), 0.05, 500.0)
        } else {
            perspective_vk(self.fov_y_deg.to_radians(), aspect.max(1e-6), 0.05, 500.0)
        };
        proj[1][1] = -proj[1][1];
        let view = look_at_rh(self.eye(), self.target, [0.0, 1.0, 0.0]);
        m4_mul(proj, view)
    }

    /// 屏幕归一化坐标 (nx,ny ∈ [-1,1],y 向上为正) → 世界射线 (origin, dir 归一)。
    /// 正交分支(F-GAME-3):平行射线——原点 = 眼平面偏移点,方向 = 相机前向。
    pub fn ray(&self, nx: f32, ny: f32, aspect: f32) -> (V3, V3) {
        let (r, u, f) = self.basis();
        if self.ortho {
            let hh = self.ortho_half_h.max(1e-4);
            let hw = hh * aspect.max(1e-6);
            let eye = self.eye();
            let origin = [
                eye[0] + r[0] * nx * hw + u[0] * ny * hh,
                eye[1] + r[1] * nx * hw + u[1] * ny * hh,
                eye[2] + r[2] * nx * hw + u[2] * ny * hh,
            ];
            return (origin, f);
        }
        let t = (self.fov_y_deg.to_radians() * 0.5).tan();
        let dir = v3_norm([
            r[0] * nx * t * aspect + u[0] * ny * t + f[0],
            r[1] * nx * t * aspect + u[1] * ny * t + f[1],
            r[2] * nx * t * aspect + u[2] * ny * t + f[2],
        ]);
        (self.eye(), dir)
    }

    pub fn to_json(&self) -> Value {
        serde_json::json!({
            "target": self.target,
            "yaw": self.yaw_deg,
            "pitch": self.pitch_deg,
            "dist": self.dist,
            "fovY": self.fov_y_deg,
            "ortho": self.ortho,
            "orthoSize": self.ortho_half_h,
        })
    }
}

/// 场景相机实体(首个启用 Camera 组件)在归一化设备坐标 (nx, ny ∈ [-1,1],y 向上)处的
/// 世界射线 (origin, dir)。与 [`scene_camera_view_proj`] 同一套相机参数解析,供 play 态
/// 指针输入反投影(logic.inject_pointer):正交 = 平行射线从相机平面出发;透视 = 自相机眼
/// 发散。无相机实体返回 None(调用方退回编辑器相机)。
pub fn scene_camera_ray(scene: &Scene, nx: f32, ny: f32, aspect: f32) -> Option<(V3, V3)> {
    let cam = scene.entities.iter().find(|e| {
        e.components
            .iter()
            .any(|c| c.ctype == "Camera" && c.enabled)
    })?;
    let props = cam
        .components
        .iter()
        .find(|c| c.ctype == "Camera")
        .map(|c| &c.props)?;
    let rot = quat_to_mat3(cam.transform.rotation);
    let right = v3_norm(m3_apply(rot, [1.0, 0.0, 0.0]));
    let up = v3_norm(m3_apply(rot, [0.0, 1.0, 0.0]));
    let fwd = v3_norm(m3_apply(rot, [0.0, 0.0, -1.0]));
    let eye = cam.transform.translation;
    let projection = props
        .get("projection")
        .and_then(|v| v.as_str())
        .unwrap_or("perspective");
    if projection == "orthographic" {
        let hh = props.get("orthoSize").and_then(|v| v.as_f64()).unwrap_or(5.0) as f32;
        let hh = hh.max(1e-4);
        let hw = hh * aspect.max(1e-6);
        let origin = [
            eye[0] + right[0] * nx * hw + up[0] * ny * hh,
            eye[1] + right[1] * nx * hw + up[1] * ny * hh,
            eye[2] + right[2] * nx * hw + up[2] * ny * hh,
        ];
        return Some((origin, fwd));
    }
    let fov_deg = props.get("fov").and_then(|v| v.as_f64()).unwrap_or(60.0) as f32;
    let t = (fov_deg.to_radians() * 0.5).tan();
    let dir = v3_norm([
        right[0] * nx * t * aspect + up[0] * ny * t + fwd[0],
        right[1] * nx * t * aspect + up[1] * ny * t + fwd[1],
        right[2] * nx * t * aspect + up[2] * ny * t + fwd[2],
    ]);
    Some((eye, dir))
}

/// 场景内置相机实体(首个启用 Camera 组件)的 view_proj。
/// PIE 期间视口以此驱动(F-GAME-2:游戏画面 = 游戏相机画面);无相机实体返回 None。
pub fn scene_camera_view_proj(scene: &Scene, aspect: f32) -> Option<M4> {
    let cam = scene.entities.iter().find(|e| {
        e.components
            .iter()
            .any(|c| c.ctype == "Camera" && c.enabled)
    })?;
    let props = cam
        .components
        .iter()
        .find(|c| c.ctype == "Camera")
        .map(|c| &c.props)?;
    let fov_deg = props.get("fov").and_then(|v| v.as_f64()).unwrap_or(60.0) as f32;
    let near = props.get("near").and_then(|v| v.as_f64()).unwrap_or(0.1) as f32;
    let far = props.get("far").and_then(|v| v.as_f64()).unwrap_or(500.0) as f32;
    // F-GAME-3:projection=orthographic 走正交(2D 游戏相机),orthoSize=半高(世界单位)。
    let projection = props
        .get("projection")
        .and_then(|v| v.as_str())
        .unwrap_or("perspective");
    let ortho_size = props.get("orthoSize").and_then(|v| v.as_f64()).unwrap_or(5.0) as f32;
    let t = &cam.transform;
    let rot = quat_to_mat3(t.rotation);
    let fwd = v3_norm(m3_apply(rot, [0.0, 0.0, -1.0]));
    let eye = t.translation;
    let center = v3_add(eye, fwd);
    // proj Y 对角元取负:与 EditorCamera::view_proj 同一显示朝向约定(见其注释)。
    let mut proj = if projection == "orthographic" {
        orthographic_vk(ortho_size, aspect.max(1e-6), near, far)
    } else {
        perspective_vk(fov_deg.to_radians(), aspect.max(1e-6), near, far)
    };
    proj[1][1] = -proj[1][1];
    Some(m4_mul(proj, look_at_rh(eye, center, [0.0, 1.0, 0.0])))
}


/// 模型腿 eye(自 modelrender::render 开头逐字搬来):`use_scene_camera`(调用方传 `vp.is_some()`)时
/// 取首个"首个 Camera 组件启用"的实体,经 `modelrt::entity_world`(走 Parent 链)的世界原点;
/// 取不到或不用场景相机时退编辑器相机眼位。
#[cfg_attr(not(feature = "backend-rurix"), allow(dead_code))]
pub(crate) fn model_eye(scene: &Scene, cam: &EditorCamera, use_scene_camera: bool) -> V3 {
    if use_scene_camera {
        scene
            .entities
            .iter()
            .find(|e| e.component("Camera").is_some_and(|c| c.enabled))
            .and_then(|e| crate::modelrt::entity_world(scene, e).ok())
            .map(|m| crate::modelrt::point(m, [0.; 3]))
            .unwrap_or_else(|| cam.eye())
    } else {
        cam.eye()
    }
}

// ─────────────────────────── ViewSetup(Godot 腿消费,§4;落地前非测试构建无调用方) ───────────────────────────

/// 投影参数。
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Projection {
    /// 竖直视场角(度)。
    Perspective { fov_y_deg: f32 },
    /// 半高(世界单位),与 Camera.orthoSize / EditorCamera.ortho_half_h 同义。
    Orthographic { half_h: f32 },
}

/// 这一帧的相机来自哪里。
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewSource {
    Editor,
    SceneCamera { entity: u64 },
}

/// 分解后的相机:Godot 腿用前 7 个字段调 RS;`view_proj` 与 rurix 同算法(逐位相等,单测锁定)。
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewSetup {
    pub source: ViewSource,
    pub eye: V3,
    /// look_at 注视点:Editor = cam.target;SceneCamera = eye + fwd。
    pub center: V3,
    pub projection: Projection,
    /// Editor 恒 0.05 / far 恒 500;SceneCamera 取组件 near/far(缺省 0.1 / 500)。
    pub near: f32,
    pub far: f32,
    /// width / height.max(1),与 render_scene_frame 同式。
    pub aspect: f32,
    pub view_proj: M4,
}

/// 编辑器相机 → ViewSetup;`view_proj = cam.view_proj(aspect)`。
#[cfg_attr(not(test), allow(dead_code))]
pub fn editor_view(cam: &EditorCamera, aspect: f32) -> ViewSetup {
    ViewSetup {
        source: ViewSource::Editor,
        eye: cam.eye(),
        center: cam.target,
        projection: if cam.ortho {
            Projection::Orthographic { half_h: cam.ortho_half_h }
        } else {
            Projection::Perspective { fov_y_deg: cam.fov_y_deg }
        },
        near: 0.05,
        far: 500.0,
        aspect,
        view_proj: cam.view_proj(aspect),
    }
}

/// 场景相机 → ViewSetup:实体选择与 props 解析同 `scene_camera_view_proj`(本地 transform、丢 roll),
/// `view_proj` 直接取 `scene_camera_view_proj(scene, aspect)`。无相机实体返回 None。
#[cfg_attr(not(test), allow(dead_code))]
pub fn scene_view(scene: &Scene, aspect: f32) -> Option<ViewSetup> {
    let cam = scene.entities.iter().find(|e| {
        e.components
            .iter()
            .any(|c| c.ctype == "Camera" && c.enabled)
    })?;
    let props = cam
        .components
        .iter()
        .find(|c| c.ctype == "Camera")
        .map(|c| &c.props)?;
    let fov_deg = props.get("fov").and_then(|v| v.as_f64()).unwrap_or(60.0) as f32;
    let near = props.get("near").and_then(|v| v.as_f64()).unwrap_or(0.1) as f32;
    let far = props.get("far").and_then(|v| v.as_f64()).unwrap_or(500.0) as f32;
    let projection = props
        .get("projection")
        .and_then(|v| v.as_str())
        .unwrap_or("perspective");
    let ortho_size = props.get("orthoSize").and_then(|v| v.as_f64()).unwrap_or(5.0) as f32;
    let rot = quat_to_mat3(cam.transform.rotation);
    let fwd = v3_norm(m3_apply(rot, [0.0, 0.0, -1.0]));
    let eye = cam.transform.translation;
    Some(ViewSetup {
        source: ViewSource::SceneCamera { entity: cam.id },
        eye,
        center: v3_add(eye, fwd),
        projection: if projection == "orthographic" {
            Projection::Orthographic { half_h: ortho_size }
        } else {
            Projection::Perspective { fov_y_deg: fov_deg }
        },
        near,
        far,
        aspect,
        view_proj: scene_camera_view_proj(scene, aspect)?,
    })
}

/// ViewSetup 的相机基 (right, up, forward),与 `look_at_rh` 同式:
/// f = norm(center - eye),s = norm(f × +Y),u = s × f。
#[cfg_attr(not(test), allow(dead_code))]
pub fn view_basis(v: &ViewSetup) -> (V3, V3, V3) {
    let f = v3_norm(v3_sub(v.center, v.eye));
    let s = v3_norm(v3_cross(f, [0.0, 1.0, 0.0]));
    let u = v3_cross(s, f);
    (s, u, f)
}
