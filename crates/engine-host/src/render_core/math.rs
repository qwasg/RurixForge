//! 向量 / 矩阵(手卷 f32,确定性),自 viewport.rs 逐字搬来(02 §3.1)。
//! `M4` 行主序存储、列向量约定 `v' = M · v`;`perspective_vk` / `orthographic_vk` 都先把 `m[1][1]` 取负。

use forge_scene::Transform;

pub(crate) type V3 = [f32; 3];
type M3 = [[f32; 3]; 3];
/// 行主序 4x4;列向量约定 `v' = M · v`。
pub type M4 = [[f32; 4]; 4];

pub(crate) fn v3_sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub(crate) fn v3_add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn v3_scale(a: V3, s: f32) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}

fn v3_dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub(crate) fn v3_cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

pub(crate) fn v3_norm(a: V3) -> V3 {
    let len = v3_dot(a, a).sqrt();
    if len < 1e-12 {
        [0.0, 0.0, 0.0]
    } else {
        v3_scale(a, 1.0 / len)
    }
}

pub(crate) fn m4_mul(a: M4, b: M4) -> M4 {
    let mut out = [[0.0f32; 4]; 4];
    for (r, row) in out.iter_mut().enumerate() {
        for (c, cell) in row.iter_mut().enumerate() {
            *cell = (0..4).map(|k| a[r][k] * b[k][c]).sum();
        }
    }
    out
}

/// 透视投影(RH,Vulkan NDC z∈[0,1];m[1][1] 取负做 y-flip 适配 attachment 行序)。
pub(crate) fn perspective_vk(fov_y_rad: f32, aspect: f32, near: f32, far: f32) -> M4 {
    let t = 1.0 / (fov_y_rad * 0.5).tan();
    let mut m = [[0.0f32; 4]; 4];
    m[0][0] = t / aspect;
    m[1][1] = -t;
    m[2][2] = far / (near - far);
    m[2][3] = far * near / (near - far);
    m[3][2] = -1.0;
    m
}

/// 正交投影(F-GAME-3 2D 支持;RH,Vulkan NDC z∈[0,1],y-flip 约定同 perspective_vk)。
/// half_h = 半高(世界单位),半宽 = half_h × aspect;z=-near→0、z=-far→1。
pub(crate) fn orthographic_vk(half_h: f32, aspect: f32, near: f32, far: f32) -> M4 {
    let hh = half_h.max(1e-4);
    let hw = (hh * aspect).max(1e-4);
    let mut m = [[0.0f32; 4]; 4];
    m[0][0] = 1.0 / hw;
    m[1][1] = -1.0 / hh;
    m[2][2] = 1.0 / (near - far);
    m[2][3] = near / (near - far);
    m[3][3] = 1.0;
    m
}

/// 观察矩阵(RH;-z 为前向)。
pub(crate) fn look_at_rh(eye: V3, center: V3, up: V3) -> M4 {
    let f = v3_norm(v3_sub(center, eye));
    let s = v3_norm(v3_cross(f, up));
    let u = v3_cross(s, f);
    [
        [s[0], s[1], s[2], -v3_dot(s, eye)],
        [u[0], u[1], u[2], -v3_dot(u, eye)],
        [-f[0], -f[1], -f[2], v3_dot(f, eye)],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

/// 四元数 [x,y,z,w] → 旋转矩阵(内部归一化,抗数值漂移)。
pub(crate) fn quat_to_mat3(q: [f32; 4]) -> M3 {
    let n = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    let (x, y, z, w) = if n < 1e-12 {
        (0.0, 0.0, 0.0, 1.0)
    } else {
        (q[0] / n, q[1] / n, q[2] / n, q[3] / n)
    };
    [
        [
            1.0 - 2.0 * (y * y + z * z),
            2.0 * (x * y - z * w),
            2.0 * (x * z + y * w),
        ],
        [
            2.0 * (x * y + z * w),
            1.0 - 2.0 * (x * x + z * z),
            2.0 * (y * z - x * w),
        ],
        [
            2.0 * (x * z - y * w),
            2.0 * (y * z + x * w),
            1.0 - 2.0 * (x * x + y * y),
        ],
    ]
}

pub(crate) fn m3_transpose(m: M3) -> M3 {
    [
        [m[0][0], m[1][0], m[2][0]],
        [m[0][1], m[1][1], m[2][1]],
        [m[0][2], m[1][2], m[2][2]],
    ]
}

pub(crate) fn m3_apply(m: M3, v: V3) -> V3 {
    [
        v3_dot(m[0], v),
        v3_dot(m[1], v),
        v3_dot(m[2], v),
    ]
}

/// TRS 模型矩阵(T·R·S;非均匀缩放按列施加)。
pub(crate) fn trs_model(t: &Transform) -> M4 {
    let r = quat_to_mat3(t.rotation);
    let mut m = [[0.0f32; 4]; 4];
    for rr in 0..3 {
        for cc in 0..3 {
            m[rr][cc] = r[rr][cc] * t.scale[cc];
        }
        m[rr][3] = t.translation[rr];
    }
    m[3][3] = 1.0;
    m
}
