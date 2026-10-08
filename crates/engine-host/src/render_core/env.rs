//! Stage 5 的场景级渲染设置与体积类实体(01 §6.3、02 §9.5 Stage 5),后端中立,只给 Pipelined 路径(Godot)用;
//! rurix 不读这些组件。组件与字段见 forge-scene 注册表:场景级 Environment / CameraAttributes / RenderSettings
//! (各取场景实体序第一个启用的),实体级 ReflectionProbe / Decal / FogVolume,挂在 Light 上的 LightParams。
//! props 先过 `forge_scene::normalize_props`(磁盘上的场景可能缺字段),类型不对的字段再换回注册表缺省,
//! 所以访问器拿到的永远是全量、合法的字段。

use std::path::Path;

use forge_scene::{Component, Scene};
use serde_json::{Map, Value};

use super::list::{ExtractStats, Fp, ItemKey, TexData};
use super::math::{trs_model, M4};

/// 实体级组件(每个启用的一份;场景级的 Environment / CameraAttributes / RenderSettings 见 `scene_env`)。
pub(crate) const VOLUME_COMPONENTS: [&str; 3] = ["ReflectionProbe", "Decal", "FogVolume"];

/// 一个组件归一后的 props(字段全量、类型合法)。
#[derive(Debug, Clone, PartialEq)]
pub struct Props {
    ctype: &'static str,
    map: Map<String, Value>,
}

impl Props {
    pub(crate) fn of(ctype: &'static str, c: &Component) -> Props {
        let raw = if c.props.is_object() { c.props.clone() } else { Value::Object(Map::new()) };
        let mut map = match forge_scene::normalize_props(ctype, &raw) {
            Ok(Value::Object(m)) => m,
            _ => raw.as_object().cloned().unwrap_or_default(),
        };
        if let Some(spec) = forge_scene::find_spec(ctype) {
            for f in spec.fields {
                let ok = map.get(f.name).is_some_and(|v| {
                    let mut one = Map::new();
                    one.insert(f.name.to_string(), v.clone());
                    forge_scene::validate_props(ctype, &Value::Object(one)).is_ok()
                });
                if !ok {
                    if let Some(d) = f.default.and_then(|d| serde_json::from_str::<Value>(d).ok()) {
                        map.insert(f.name.to_string(), d);
                    }
                }
            }
        }
        Props { ctype, map }
    }

    pub fn ctype(&self) -> &'static str {
        self.ctype
    }

    pub fn num(&self, k: &str) -> f32 {
        self.map.get(k).and_then(Value::as_f64).unwrap_or(0.0) as f32
    }

    pub fn flag(&self, k: &str) -> bool {
        self.map.get(k).and_then(Value::as_bool).unwrap_or(false)
    }

    pub fn text(&self, k: &str) -> &str {
        self.map.get(k).and_then(Value::as_str).unwrap_or("")
    }

    fn floats<const N: usize>(&self, k: &str, fill: [f32; N]) -> [f32; N] {
        let mut out = fill;
        if let Some(a) = self.map.get(k).and_then(Value::as_array) {
            for (o, v) in out.iter_mut().zip(a) {
                *o = v.as_f64().unwrap_or(0.0) as f32;
            }
        }
        out
    }

    /// [f32;4] 字段(颜色是 sRGB 编码值,同 Godot)。
    pub fn rgba(&self, k: &str) -> [f32; 4] {
        self.floats(k, [0.0, 0.0, 0.0, 1.0])
    }

    pub fn vec3(&self, k: &str) -> [f32; 3] {
        self.floats(k, [0.0; 3])
    }

    fn fingerprint(&self) -> String {
        serde_json::to_string(&self.map).unwrap_or_default()
    }
}

/// 场景级设置;None = 场景里没有该组件(Godot 按 Stage 4 的缺省环境处理)。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SceneEnv {
    pub environment: Option<Props>,
    pub camera_attributes: Option<Props>,
    pub render_settings: Option<Props>,
    /// Environment 引用的贴图(skyTexture、adjustmentColorCorrection),按字段名;解析失败的不出现。
    pub textures: Vec<(&'static str, TexData)>,
}

/// 实体级体积类组件的一个实例。
#[derive(Debug, Clone, PartialEq)]
pub struct VolumeItem {
    pub key: ItemKey,
    /// "ReflectionProbe" | "Decal" | "FogVolume"
    pub kind: &'static str,
    /// modelrt::entity_world(走 Parent 链;环 → 退回本地 transform)。
    pub world: M4,
    pub props: Props,
    /// Decal 的贴图(textureAlbedo / textureNormal / textureOrm / textureEmission),按字段名。
    pub textures: Vec<(&'static str, TexData)>,
    /// 内容指纹(kind + props + 贴图身份),不含 world;RenderDelta 之后 Godot 侧据此区分"重建"与"只移动"。
    pub content: u64,
}

fn texture(root: &Path, guid: &str) -> Option<TexData> {
    if guid.is_empty() {
        return None;
    }
    let t = super::assets::load_tex_static_cached(root, guid)?;
    Some(TexData { guid: guid.to_string(), w: t.w, h: t.h, rgba: t.rgba })
}

fn textures(p: &Props, root: &Path, fields: &[&'static str]) -> Vec<(&'static str, TexData)> {
    fields.iter().filter_map(|f| texture(root, p.text(f)).map(|t| (*f, t))).collect()
}

fn first(scene: &Scene, ctype: &'static str) -> Option<Props> {
    scene.entities.iter().find_map(|e| e.component(ctype).filter(|c| c.enabled)).map(|c| Props::of(ctype, c))
}

/// 场景级三件套 + Environment 的贴图。
pub(super) fn scene_env(scene: &Scene, root: &Path) -> SceneEnv {
    let environment = first(scene, "Environment");
    let textures = environment.as_ref().map(|p| textures(p, root, &["skyTexture", "adjustmentColorCorrection"])).unwrap_or_default();
    SceneEnv { environment, camera_attributes: first(scene, "CameraAttributes"), render_settings: first(scene, "RenderSettings"), textures }
}

/// 实体级体积类组件,按 key 升序;同一实体上的多个(例如探针 + 贴花)用 sub 区分。
pub(super) fn volumes(scene: &Scene, root: &Path, stats: &mut ExtractStats) -> Vec<VolumeItem> {
    let mut out = Vec::new();
    for e in &scene.entities {
        for kind in VOLUME_COMPONENTS {
            let Some(c) = e.component(kind).filter(|c| c.enabled) else {
                continue;
            };
            let props = Props::of(kind, c);
            let textures = if kind == "Decal" {
                textures(&props, root, &["textureAlbedo", "textureNormal", "textureOrm", "textureEmission"])
            } else {
                Vec::new()
            };
            let mut fp = Fp::new(kind).s(&props.fingerprint());
            for (f, t) in &textures {
                fp = fp.s(f).s(&t.guid).u(t.w as u64).u(t.h as u64);
            }
            let world = crate::modelrt::entity_world(scene, e).unwrap_or_else(|_| trs_model(&e.transform));
            out.push(VolumeItem { key: ItemKey { entity: e.id, sub: 0 }, kind, world, props, textures, content: fp.done() });
        }
    }
    out.sort_by_key(|v| v.key.entity);
    for i in 1..out.len() {
        if out[i].key.entity == out[i - 1].key.entity {
            out[i].key.sub = out[i - 1].key.sub + 1;
        }
    }
    stats.volumes = out.len();
    out
}


#[cfg(test)]
mod tests {
    use super::*;
    use forge_scene::{Entity, Transform};
    use serde_json::json;

    const ALL: [&str; 7] = ["Environment", "CameraAttributes", "RenderSettings", "ReflectionProbe", "Decal", "FogVolume", "LightParams"];

    /// schema 定稿(02 §9.5 Stage 5):7 个组件都已注册、字段全部可选,空 props 归一后逐字段合法;已有组件没有加字段。
    #[test]
    fn stage5_components_are_registered_with_all_optional_valid_defaults() {
        for ctype in ALL {
            let spec = forge_scene::find_spec(ctype).unwrap_or_else(|| panic!("缺组件 {ctype}"));
            assert!(spec.fields.iter().all(|f| f.default.is_some()), "{ctype} 的字段必须全部可选");
            let n = forge_scene::normalize_props(ctype, &json!({})).unwrap();
            assert_eq!(n.as_object().unwrap().len(), spec.fields.len(), "{ctype}");
            forge_scene::validate_props(ctype, &n).unwrap_or_else(|e| panic!("{ctype} 缺省值不合法:{e}"));
        }
        let light: Vec<&str> = forge_scene::find_spec("Light").unwrap().fields.iter().map(|f| f.name).collect();
        assert_eq!(light, ["kind", "color", "intensity", "castShadow"], "Light 不加字段(rurix 的 entity.get / scene.save 不变)");
        assert_eq!(forge_scene::find_spec("Camera").unwrap().fields.len(), 5);
    }

    #[test]
    fn props_fill_defaults_and_repair_wrong_types() {
        let c = Component::new("Environment", json!({ "tonemap": "agx", "exposure": "bright", "glowLevel2": 0.5, "extra": 1 }));
        let p = Props::of("Environment", &c);
        assert_eq!(p.text("tonemap"), "agx");
        assert_eq!(p.num("exposure"), 1.0, "类型不对 → 注册表缺省");
        assert_eq!(p.num("glowLevel2"), 0.5);
        assert_eq!(p.num("glowIntensity"), 0.3);
        assert_eq!(p.rgba("fogLightColor"), [0.518, 0.553, 0.608, 1.0]);
        assert!(p.flag("volumetricFogTemporalReprojection"));
        assert_eq!(p.text("background"), "clearColor");
        let bad = Component::new("Environment", json!("not an object"));
        assert_eq!(Props::of("Environment", &bad), Props::of("Environment", &Component::new("Environment", json!({}))));
    }

    fn ent(id: u64, comps: Vec<Component>, t: [f32; 3]) -> Entity {
        Entity { entity_guid: None, id, name: format!("e{id}"), transform: Transform { translation: t, ..Transform::default() }, components: comps }
    }

    #[test]
    fn scene_level_takes_first_enabled_and_volumes_follow_parent_chain() {
        let root = Path::new(".");
        let mut s = Scene::new("env");
        let mut off = Component::new("Environment", json!({ "tonemap": "aces" }));
        off.enabled = false;
        s.entities.push(ent(1, vec![off], [0.0; 3]));
        s.entities.push(ent(2, vec![Component::new("Environment", json!({ "tonemap": "filmic" }))], [0.0; 3]));
        s.entities.push(ent(3, vec![Component::new("Environment", json!({ "tonemap": "agx" }))], [0.0; 3]));
        s.entities.push(ent(9, vec![Component::new("Decal", json!({ "size": [1.0, 2.0, 3.0] })),
                                    Component::new("ReflectionProbe", json!({})),
                                    Component::new("Parent", json!({ "entity": 4 }))], [1.0, 0.0, 0.0]));
        s.entities.push(ent(4, vec![], [0.0, 5.0, 0.0]));
        let env = scene_env(&s, root);
        assert_eq!(env.environment.as_ref().map(|p| p.text("tonemap")), Some("filmic"), "跳过禁用的,取第一个启用的");
        assert!(env.camera_attributes.is_none() && env.render_settings.is_none() && env.textures.is_empty());
        let mut stats = ExtractStats::default();
        let v = volumes(&s, root, &mut stats);
        assert_eq!(v.iter().map(|x| (x.key.entity, x.key.sub, x.kind)).collect::<Vec<_>>(), [(9, 0, "ReflectionProbe"), (9, 1, "Decal")]);
        assert_eq!(stats.volumes, 2);
        assert_eq!((v[1].world[0][3], v[1].world[1][3]), (1.0, 5.0), "走 Parent 链");
        assert_eq!(v[1].props.vec3("size"), [1.0, 2.0, 3.0]);
        let mut moved = s.clone();
        moved.entities[3].transform.translation = [7.0, 0.0, 0.0];
        let w = volumes(&moved, root, &mut stats);
        assert_eq!(w[1].content, v[1].content, "content 不含 world");
        assert_ne!(w[1].world, v[1].world);
        assert_eq!(scene_env(&Scene::new("empty"), root), SceneEnv::default());
    }
}
