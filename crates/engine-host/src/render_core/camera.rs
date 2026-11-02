//! 相机(02 §3.1):编辑器相机、场景相机 view_proj / 射线、模型腿 eye,以及给 Godot 腿用的 `ViewSetup`。
//! 场景相机的投影、射线与着色眼位共享世界变换，包含 Parent 链与 roll。

use forge_scene::Scene;
use serde_json::Value;

use super::math::{
    look_at_rh, m4_mul, orthographic_vk, perspective_vk, v3_add, v3_cross, v3_norm, v3_sub,
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
fn scene_camera_basis(scene: &Scene) -> Option<(&forge_scene::Entity, &Value, V3, V3, V3, V3)> {
    let (cam, component) = scene.entities.iter().find_map(|e|e.components.iter().find(|c|c.ctype=="Camera"&&c.enabled).map(|c|(e,c)))?;
    let props = &component.props;
    let world = crate::modelrt::entity_world(scene, cam).ok()?;
    let eye = crate::modelrt::point(world, [0.; 3]);
    let fwd = v3_norm(crate::modelrt::vector(world, [0., 0., -1.]));
    let up_hint = v3_norm(crate::modelrt::vector(world, [0., 1., 0.]));
    let right = v3_norm(v3_cross(fwd, up_hint));
    let up = v3_cross(right, fwd);
    if !eye.iter().chain(fwd.iter()).chain(up.iter()).all(|v| v.is_finite()) || right.iter().map(|v| v*v).sum::<f32>() < 0.5 { return None; }
    Some((cam, props, eye, right, up, fwd))
}

pub fn scene_camera_ray(scene: &Scene, nx: f32, ny: f32, aspect: f32) -> Option<(V3, V3)> {
    let (_, props, eye, right, up, fwd) = scene_camera_basis(scene)?;
    let aspect = aspect.max(1e-6);
    let projection = props.get("projection").and_then(Value::as_str).unwrap_or("perspective");
    if projection == "orthographic" {
        let hh = (props.get("orthoSize").and_then(Value::as_f64).unwrap_or(5.) as f32).max(1e-4);
        return Some((std::array::from_fn(|i| eye[i] + right[i]*nx*hh*aspect + up[i]*ny*hh), fwd));
    }
    let fov = props.get("fov").and_then(Value::as_f64).unwrap_or(60.) as f32;
    let t = (fov.to_radians()*0.5).tan();
    Some((eye, v3_norm(std::array::from_fn(|i| right[i]*nx*t*aspect + up[i]*ny*t + fwd[i]))))
}

/// 场景内置相机实体(首个启用 Camera 组件)的 view_proj。
/// PIE 期间视口以此驱动(F-GAME-2:游戏画面 = 游戏相机画面);无相机实体返回 None。
pub fn scene_camera_view_proj(scene: &Scene, aspect: f32) -> Option<M4> {
    let (_, props, eye, _, up, fwd) = scene_camera_basis(scene)?;
    let near = props.get("near").and_then(Value::as_f64).unwrap_or(0.1) as f32;
    let far = props.get("far").and_then(Value::as_f64).unwrap_or(500.) as f32;
    let mut proj = if props.get("projection").and_then(Value::as_str) == Some("orthographic") {
        orthographic_vk((props.get("orthoSize").and_then(Value::as_f64).unwrap_or(5.) as f32).max(1e-4), aspect.max(1e-6), near, far)
    } else {
        perspective_vk((props.get("fov").and_then(Value::as_f64).unwrap_or(60.) as f32).to_radians(), aspect.max(1e-6), near, far)
    };
    proj[1][1] = -proj[1][1];
    Some(m4_mul(proj, look_at_rh(eye, v3_add(eye, fwd), up)))
}


/// 模型腿 eye(自 modelrender::render 开头逐字搬来):`use_scene_camera`(调用方传 `vp.is_some()`)时
/// 取首个"首个 Camera 组件启用"的实体,经 `modelrt::entity_world`(走 Parent 链)的世界原点;
/// 取不到或不用场景相机时退编辑器相机眼位。
#[cfg_attr(not(feature = "backend-rurix"), allow(dead_code))]
pub(crate) fn model_eye(scene: &Scene, cam: &EditorCamera, use_scene_camera: bool) -> V3 {
    if use_scene_camera { scene_camera_basis(scene).map(|(_, _, eye, _, _, _)| eye).unwrap_or_else(|| cam.eye()) } else { cam.eye() }
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
    /// World-space up, including scene-camera roll.
    pub up: V3,
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
        up: [0.,1.,0.],
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

/// 场景相机 → ViewSetup:实体选择与世界变换同 `scene_camera_view_proj`(Parent 与 roll),
/// `view_proj` 直接取 `scene_camera_view_proj(scene, aspect)`。无相机实体返回 None。
#[cfg_attr(not(test), allow(dead_code))]
pub fn scene_view(scene: &Scene, aspect: f32) -> Option<ViewSetup> {
    let (cam, props, eye, _, up, fwd) = scene_camera_basis(scene)?;
    Some(ViewSetup {
        source: ViewSource::SceneCamera { entity: cam.id }, eye, center: v3_add(eye, fwd), up,
        projection: if props.get("projection").and_then(Value::as_str) == Some("orthographic") {
            Projection::Orthographic { half_h: (props.get("orthoSize").and_then(Value::as_f64).unwrap_or(5.) as f32).max(1e-4) }
        } else { Projection::Perspective { fov_y_deg: props.get("fov").and_then(Value::as_f64).unwrap_or(60.) as f32 } },
        near: props.get("near").and_then(Value::as_f64).unwrap_or(0.1) as f32,
        far: props.get("far").and_then(Value::as_f64).unwrap_or(500.) as f32,
        aspect, view_proj: scene_camera_view_proj(scene, aspect)?,
    })
}

/// ViewSetup 的相机基 (right, up, forward),与 `look_at_rh` 同式:
/// f = norm(center - eye),s = norm(f × +Y),u = s × f。
#[cfg_attr(not(test), allow(dead_code))]
pub fn view_basis(v: &ViewSetup) -> (V3, V3, V3) {
    let f = v3_norm(v3_sub(v.center, v.eye));
    let s = v3_norm(v3_cross(f, v.up));
    let u = v3_cross(s, f);
    (s, u, f)
}

#[cfg(test)]
mod camera_contract_tests {
    use super::*;
    use forge_scene::{Component,Entity,Transform};
    use serde_json::json;
    fn near(a:V3,b:V3){for i in 0..3{assert!((a[i]-b[i]).abs()<0.00001,"{a:?} != {b:?}");}}
    #[test]
    fn parent_roll_projection_ray_and_eye_share_world_frame(){
        let mut scene=Scene::new("camera");
        let half=std::f32::consts::FRAC_1_SQRT_2;
        scene.entities.push(Entity{entity_guid:None,id:1,name:"parent".into(),transform:Transform{translation:[10.,0.,0.],rotation:[0.,0.,half,half],scale:[1.;3]},components:vec![]});
        scene.entities.push(Entity{entity_guid:None,id:2,name:"camera".into(),transform:Transform{translation:[2.,0.,5.],..Transform::default()},components:vec![Component::new("Parent",json!({"entity":1})),Component::new("Camera",json!({"fov":90.}))]});
        let view=scene_view(&scene,1.).unwrap();
        near(view.eye,[10.,2.,5.]); near(view.up,[-1.,0.,0.]);
        near(model_eye(&scene,&EditorCamera::default(),true),view.eye);
        let (origin,dir)=scene_camera_ray(&scene,1.,0.,1.).unwrap();near(origin,view.eye);near(dir,[0.,half,-half]);
        let point=std::array::from_fn::<_,3,_>(|i|origin[i]+dir[i]*4.);
        let clip: [f32;4]=std::array::from_fn(|r|view.view_proj[r][0]*point[0]+view.view_proj[r][1]*point[1]+view.view_proj[r][2]*point[2]+view.view_proj[r][3]);
        assert!((clip[0]/clip[3]-1.).abs()<0.00001);assert!((clip[1]/clip[3]).abs()<0.00001);
        let (right,up,forward)=view_basis(&view);near(right,[0.,1.,0.]);near(up,[-1.,0.,0.]);near(forward,[0.,0.,-1.]);
    }
}
