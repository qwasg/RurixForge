//! ParticleEmitter(01 §5.6、02 §9.5 Stage 5 step 6),只在 [gmain];只有 FORGE_GPU_PARTICLES 打开时 extract 才给出事件。
//! rurix(gpu_particles.rs COMPUTE / VERTEX / FRAGMENT)的轨迹是 (槽位, entity id, 中心, 年龄, 寿命, 样式) 的解析函数:
//! 没有时钟、没有累积状态。这里逐式复刻,不用 Godot 的粒子模拟:
//! - 一个 canvas item,静态三角形数组 4096 个粒子 × 6 个顶点(顶点 = (全局序号 i, 角序号)),顺序与 rurix 的实例序相同;
//! - canvas_item 着色器按同一个 hash / random、同一组样式公式算位置、尺寸、颜色,再用本帧的 view_proj 投到像素坐标;
//! - 每帧只上传 64 个发射器的事件(forge_em[128])+ 相机矩阵,所以同一场景两次取帧逐字节相同。
//!
//! 混合:rurix 把粒子加法混合在 UNORM 目标的 sRGB 编码值上(rgb·a + dst,不测深度、画在所有网格之后)。
//! 非 HDR 视口的 2D canvas 同样在 sRGB 编码值上混合(canvas.glsl 只在 HDR 2D 时转线性),所以叠加层画在 3D 之后,
//! 与 rurix 在同一个混合空间里。UV 用 1/w 手工做透视校正,与 rurix 的 3D quad 插值一致。

use godot::classes::RenderingServer;
use godot::prelude::*;

use engine_host::{ParticleItem, ViewSetup};

use crate::rid::Owned;

const EMITTERS: usize = 64;
const PER_EMITTER: usize = 64;

const SHADER: &str = r#"shader_type canvas_item;
render_mode blend_add, unshaded, skip_vertex_transform;
uniform mat4 forge_vp;
uniform vec2 forge_size;
uniform vec4 forge_em[128];
varying vec3 forge_uvw;
varying vec4 forge_col;
uint forge_hash(uint v) {
	uint h = v * 747796405u + 2891336453u;
	h = ((h >> ((h >> 28u) + 4u)) ^ h) * 277803737u;
	return (h >> 22u) ^ h;
}
float forge_random(uint v) {
	return float(forge_hash(v) & 16777215u) / 16777216.0;
}
void vertex() {
	int i = int(VERTEX.x + 0.5);
	int k = int(VERTEX.y + 0.5);
	vec4 ca = forge_em[2 * (i / 64)];
	vec4 ls = forge_em[2 * (i / 64) + 1];
	float life = ls.x;
	float age = ca.w;
	forge_uvw = vec3(0.0, 0.0, 1.0);
	forge_col = vec4(0.0);
	VERTEX = vec2(-100000.0);
	if (!(ls.w < 0.5 || life <= 0.0 || age >= life)) {
		uint seed = uint(i) + uint(ls.z) * 131u;
		float r0 = forge_random(seed);
		float r1 = forge_random(seed + 77u);
		float r2 = forge_random(seed + 911u);
		uint kind = uint(ls.y);
		float progress = clamp(age / life, 0.0, 1.0);
		float angle = float(i % 64) * 0.0981747704 + r0 * 0.55;
		vec2 direction = vec2(cos(angle), sin(angle));
		vec2 offset = direction * age * (0.9 + r1 * 2.8);
		vec3 tint = vec3(0.3, 0.88, 1.0);
		if (kind == 1u) {
			offset = vec2(age * (1.0 + 5.0 * r1), (r2 - 0.5) * age * 1.4);
		} else if (kind == 2u) {
			offset.y += age * 0.9 - age * age * 0.55;
			tint = vec3(0.68, 1.0, 0.32);
		} else if (kind == 3u) {
			offset.x *= 1.65;
			offset.y += sin(age * 5.0 + r2 * 6.2831853) * age * 0.22;
			tint = vec3(0.25, 0.65, 1.0);
		} else {
			direction = vec2(cos(angle + age * 1.8), sin(angle + age * 1.8));
			offset = direction * age * (0.8 + r1 * 2.0);
			tint = vec3(0.83, 0.57, 1.0);
		}
		float size = (0.045 + r2 * 0.055) * (1.0 - progress * progress);
		vec2 corner = vec2((k == 1 || k == 2 || k == 4) ? 1.0 : -1.0, (k == 2 || k == 4 || k == 5) ? 1.0 : -1.0);
		vec3 center = ca.xyz + vec3(offset, 0.0);
		vec4 cc = forge_vp * vec4(center, 1.0);
		vec4 clip = forge_vp * vec4(center + vec3(corner * size, 0.0), 1.0);
		if (cc.w > 1e-6 && cc.z >= 0.0 && cc.z <= cc.w && clip.w > 1e-6) {
			vec2 ndc = clip.xy / clip.w;
			// rurix 出帧(首行在上)里 NDC +y 在画面上方;canvas 像素坐标 y 向下。
			VERTEX = vec2((ndc.x * 0.5 + 0.5) * forge_size.x, (0.5 - ndc.y * 0.5) * forge_size.y);
			forge_uvw = vec3(corner / clip.w, 1.0 / clip.w);
			forge_col = vec4(tint, 1.0 - progress);
		}
	}
}
void fragment() {
	vec2 uv = forge_uvw.xy / forge_uvw.z;
	float radius = dot(uv, uv);
	if (radius > 1.0 || forge_col.a < 0.01) {
		discard;
	}
	float core = pow(max(0.0, 1.0 - radius), 2.0);
	COLOR = vec4(mix(forge_col.rgb * 0.72, vec3(1.0), core * 0.8), core * forge_col.a);
}
"#;

/// 叠加层:canvas item(先放)→ canvas → 材质 → 着色器。drop 前先从视口上摘下 canvas。
pub struct Particles {
    _item: Owned,
    canvas: Owned,
    material: Owned,
    _shader: Owned,
    viewport: Rid,
}

impl Particles {
    pub fn new(rs: &mut Gd<RenderingServer>, viewport: Rid) -> Particles {
        let shader = Owned::new(rs.shader_create());
        rs.shader_set_code(shader.rid(), SHADER);
        let material = Owned::new(rs.material_create());
        rs.material_set_shader(material.rid(), shader.rid());
        let canvas = Owned::new(rs.canvas_create());
        let item = Owned::new(rs.canvas_item_create());
        rs.canvas_item_set_parent(item.rid(), canvas.rid());
        rs.canvas_item_set_material(item.rid(), material.rid());
        // 顶点 = (全局粒子序号, 角序号 0..6);位置全在着色器里算。rurix 的 6 个角:(−1,−1) (1,−1) (1,1) (−1,−1) (1,1) (−1,1)。
        let n = EMITTERS * PER_EMITTER * 6;
        let points: Vec<Vector2> = (0..n).map(|v| Vector2::new((v / 6) as f32, (v % 6) as f32)).collect();
        let indices: Vec<i32> = (0..n as i32).collect();
        rs.canvas_item_add_triangle_array(
            item.rid(),
            &PackedInt32Array::from(indices.as_slice()),
            &PackedVector2Array::from(points.as_slice()),
            &PackedColorArray::from(&[Color::WHITE][..]),
        );
        rs.canvas_item_set_custom_rect_ex(item.rid(), true).rect(Rect2::new(Vector2::splat(-1.0e6), Vector2::splat(2.0e6))).done();
        rs.viewport_attach_canvas(viewport, canvas.rid());
        // 在 Mobile 的 HDR 转换层(layer 0)之上。
        rs.viewport_set_canvas_stacking(viewport, canvas.rid(), 1, 0);
        Particles { _item: item, canvas, material, _shader: shader, viewport }
    }

    /// 本帧的发射器事件:槽位 e 的两个 vec4 = (中心, 年龄)、(寿命, 样式, 种子, 有效);没出现的槽位全 0(= rurix 的闲置槽)。
    pub fn update(&mut self, rs: &mut Gd<RenderingServer>, items: &[ParticleItem]) {
        let mut em = vec![Vector4::ZERO; EMITTERS * 2];
        for it in items {
            let s = it.slot as usize;
            if s < EMITTERS {
                em[2 * s] = Vector4::new(it.center[0], it.center[1], it.center[2], it.age);
                em[2 * s + 1] = Vector4::new(it.lifetime, it.kind, it.seed, 1.0);
            }
        }
        rs.material_set_param(self.material.rid(), "forge_em", &PackedVector4Array::from(em.as_slice()).to_variant());
    }

    /// 相机与视口尺寸(与 rurix 同一个 frame_vp = RenderList.view.view_proj)。
    pub fn set_view(&mut self, rs: &mut Gd<RenderingServer>, v: &ViewSetup, w: u32, h: u32) {
        let m = v.view_proj;
        let col = |c: usize| Vector4::new(m[0][c], m[1][c], m[2][c], m[3][c]);
        let p = Projection::new([col(0), col(1), col(2), col(3)]);
        rs.material_set_param(self.material.rid(), "forge_vp", &p.to_variant());
        rs.material_set_param(self.material.rid(), "forge_size", &Vector2::new(w as f32, h as f32).to_variant());
    }
}

impl Drop for Particles {
    fn drop(&mut self) {
        RenderingServer::singleton().viewport_remove_canvas(self.viewport, self.canvas.rid());
    }
}
