//! forge-scene — Forge 场景模型:Entity/Component/Scene + 组件注册表 +
//! 确定性 .rxscene 序列化(key 排序、两空格缩进、末尾单换行,load→save 逐字节同态)。

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::fmt;
use std::path::Path;

/// 实体变换(平移 + xyzw 四元数旋转 + 缩放)。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Transform {
    pub translation: [f32; 3],
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
}

impl Default for Transform {
    fn default() -> Self {
        Transform {
            translation: [0.0; 3],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0; 3],
        }
    }
}

/// 组件实例:{type, enabled, props}(props 为自由 JSON 对象,受注册表校验)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Component {
    #[serde(rename = "type")]
    pub ctype: String,
    pub enabled: bool,
    #[serde(default = "empty_object")]
    pub props: Value,
}

fn empty_object() -> Value {
    Value::Object(Map::new())
}

impl Component {
    /// 建启用态组件(props 调用方保证已过注册表校验)。
    pub fn new(ctype: impl Into<String>, props: Value) -> Self {
        Component {
            ctype: ctype.into(),
            enabled: true,
            props,
        }
    }
}

/// 实体(id + 名称 + 变换 + 组件列表)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entity {
    pub id: u64,
    pub name: String,
    pub transform: Transform,
    #[serde(default)]
    pub components: Vec<Component>,
}

impl Entity {
    /// 按类型名查组件。
    pub fn component(&self, ctype: &str) -> Option<&Component> {
        self.components.iter().find(|c| c.ctype == ctype)
    }

    /// 按类型名查可变组件。
    pub fn component_mut(&mut self, ctype: &str) -> Option<&mut Component> {
        self.components.iter_mut().find(|c| c.ctype == ctype)
    }
}

fn default_next_id() -> u64 {
    1
}

/// 场景(名称 + 实体列表 + 单调 id 计数器,随场景持久化,重载后新建不撞号)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scene {
    pub name: String,
    #[serde(default)]
    pub entities: Vec<Entity>,
    #[serde(default = "default_next_id")]
    pub next_id: u64,
}

/// 场景摘要(JSON 键与宿主协议对齐:name / entityCount)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneSummary {
    pub name: String,
    pub entity_count: usize,
}

/// 场景存取错误(IO 或 JSON)。
#[derive(Debug)]
pub enum SceneError {
    Io(std::io::Error),
    Json(serde_json::Error),
}

impl fmt::Display for SceneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SceneError::Io(e) => write!(f, "IO 错误:{e}"),
            SceneError::Json(e) => write!(f, "JSON 错误:{e}"),
        }
    }
}

impl std::error::Error for SceneError {}

impl From<std::io::Error> for SceneError {
    fn from(e: std::io::Error) -> Self {
        SceneError::Io(e)
    }
}

impl From<serde_json::Error> for SceneError {
    fn from(e: serde_json::Error) -> Self {
        SceneError::Json(e)
    }
}

// ---------- 组件注册表 ----------

/// 组件字段简表(供 Inspector/agent 发现)。
pub struct FieldSpec {
    pub name: &'static str,
    /// 类型标记:string / number / [f32;3] / enum:a|b|c
    pub ty: &'static str,
}

/// 组件类型注册项。
pub struct ComponentSpec {
    pub name: &'static str,
    pub fields: &'static [FieldSpec],
}

/// 首发组件集:MeshRenderer / RigidBody / Light / Camera。
pub const REGISTRY: &[ComponentSpec] = &[
    ComponentSpec {
        name: "MeshRenderer",
        fields: &[
            FieldSpec { name: "mesh", ty: "string" },
            FieldSpec { name: "material", ty: "string" },
        ],
    },
    ComponentSpec {
        name: "RigidBody",
        fields: &[
            FieldSpec { name: "kind", ty: "enum:static|dynamic|kinematic" },
            FieldSpec { name: "mass", ty: "number" },
        ],
    },
    ComponentSpec {
        name: "Light",
        fields: &[
            FieldSpec { name: "kind", ty: "string" },
            FieldSpec { name: "color", ty: "[f32;3]" },
            FieldSpec { name: "intensity", ty: "number" },
        ],
    },
    ComponentSpec {
        name: "Camera",
        fields: &[
            FieldSpec { name: "fov", ty: "number" },
            FieldSpec { name: "near", ty: "number" },
            FieldSpec { name: "far", ty: "number" },
        ],
    },
];

/// 按名查注册项。
pub fn find_spec(ctype: &str) -> Option<&'static ComponentSpec> {
    REGISTRY.iter().find(|s| s.name == ctype)
}

/// listTypes():类型名 + 字段 schema 简表。
pub fn list_types_json() -> Value {
    Value::Array(
        REGISTRY
            .iter()
            .map(|s| {
                json!({
                    "name": s.name,
                    "fields": s.fields.iter()
                        .map(|f| json!({ "name": f.name, "type": f.ty }))
                        .collect::<Vec<_>>(),
                })
            })
            .collect(),
    )
}

/// 校验 props:类型须已注册、props 为对象、字段齐全且类型匹配。
pub fn validate_props(ctype: &str, props: &Value) -> Result<(), String> {
    let spec = find_spec(ctype).ok_or_else(|| format!("未知组件类型:{ctype}"))?;
    let obj = props
        .as_object()
        .ok_or_else(|| format!("组件 {ctype} 的 props 须为对象"))?;
    for f in spec.fields {
        let v = obj
            .get(f.name)
            .ok_or_else(|| format!("组件 {ctype} 缺字段 {}", f.name))?;
        let ok = match f.ty {
            "string" => v.is_string(),
            "number" => v.is_number(),
            "[f32;3]" => v
                .as_array()
                .is_some_and(|a| a.len() == 3 && a.iter().all(Value::is_number)),
            _ if f.ty.starts_with("enum:") => v
                .as_str()
                .is_some_and(|s| f.ty[5..].split('|').any(|e| e == s)),
            _ => true,
        };
        if !ok {
            return Err(format!("组件 {ctype} 字段 {} 类型须为 {}", f.name, f.ty));
        }
    }
    Ok(())
}

/// 校验整个组件实例。
pub fn validate_component(c: &Component) -> Result<(), String> {
    validate_props(&c.ctype, &c.props)
}

// ---------- 确定性序列化 ----------

fn push_indent(out: &mut String, level: usize) {
    for _ in 0..level {
        out.push_str("  ");
    }
}

/// 规范 JSON 写入:object key 字典序、两空格缩进;空对象/数组紧凑。
fn write_canonical(v: &Value, out: &mut String, level: usize) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => out.push_str(&n.to_string()),
        Value::String(s) => out.push_str(&serde_json::to_string(s).unwrap_or_default()),
        Value::Array(a) => {
            if a.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push_str("[\n");
            for (i, item) in a.iter().enumerate() {
                push_indent(out, level + 1);
                write_canonical(item, out, level + 1);
                if i + 1 < a.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            push_indent(out, level);
            out.push(']');
        }
        Value::Object(m) => {
            if m.is_empty() {
                out.push_str("{}");
                return;
            }
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            out.push_str("{\n");
            for (i, k) in keys.iter().enumerate() {
                push_indent(out, level + 1);
                out.push_str(&serde_json::to_string(k).unwrap_or_default());
                out.push_str(": ");
                write_canonical(&m[*k], out, level + 1);
                if i + 1 < keys.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            push_indent(out, level);
            out.push('}');
        }
    }
}

impl Scene {
    /// 建空场景(id 计数器从 1 起)。
    pub fn new(name: impl Into<String>) -> Self {
        Scene {
            name: name.into(),
            entities: Vec::new(),
            next_id: 1,
        }
    }

    /// 分配单调 id(随场景持久化)。
    pub fn alloc_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// 按 id 查实体。
    pub fn entity(&self, id: u64) -> Option<&Entity> {
        self.entities.iter().find(|e| e.id == id)
    }

    /// 按 id 查可变实体。
    pub fn entity_mut(&mut self, id: u64) -> Option<&mut Entity> {
        self.entities.iter_mut().find(|e| e.id == id)
    }

    /// 规范 JSON 字节(无末尾换行;key 排序、固定缩进,跨进程逐字节确定)。
    pub fn to_json(&self) -> Result<String, SceneError> {
        let v = serde_json::to_value(self)?;
        let mut out = String::new();
        write_canonical(&v, &mut out, 0);
        Ok(out)
    }

    /// 从 JSON 字串反序列化(容忍任意排版/换行)。
    pub fn from_json(s: &str) -> Result<Self, SceneError> {
        Ok(serde_json::from_str(s)?)
    }

    /// 保存 .rxscene(规范字节 + 末尾单换行)。
    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), SceneError> {
        let mut text = self.to_json()?;
        text.push('\n');
        std::fs::write(path, text)?;
        Ok(())
    }

    /// 读取 .rxscene(UTF-8 JSON)。
    pub fn load(path: impl AsRef<Path>) -> Result<Self, SceneError> {
        let text = std::fs::read_to_string(path)?;
        Self::from_json(&text)
    }

    /// 摘要:(name, entityCount)。
    pub fn summary(&self) -> SceneSummary {
        SceneSummary {
            name: self.name.clone(),
            entity_count: self.entities.len(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn demo_scene() -> Scene {
        let mut scene = Scene::new("关卡一");
        let id1 = scene.alloc_id();
        let id2 = scene.alloc_id();
        scene.entities = vec![
            Entity {
                id: id1,
                name: "主角".into(),
                transform: Transform::default(),
                components: vec![
                    Component::new("MeshRenderer", json!({"mesh": "hero.fbx", "material": "hero.mat"})),
                    Component {
                        ctype: "RigidBody".into(),
                        enabled: false,
                        props: json!({"kind": "dynamic", "mass": 80.5}),
                    },
                ],
            },
            Entity {
                id: id2,
                name: "地板".into(),
                transform: Transform {
                    translation: [0.0, -1.0, 0.0],
                    rotation: [0.0, 0.0, 0.0, 1.0],
                    scale: [10.0, 0.5, 10.0],
                },
                components: vec![Component::new(
                    "Light",
                    json!({"kind": "directional", "color": [1.0, 0.9, 0.8], "intensity": 2.5}),
                )],
            },
        ];
        scene
    }

    #[test]
    fn serde_roundtrip_byte_identical() {
        let scene = demo_scene();
        let json1 = scene.to_json().unwrap();
        let scene2 = Scene::from_json(&json1).unwrap();
        let json2 = scene2.to_json().unwrap();
        assert_eq!(scene, scene2, "反序列化结构须等价");
        assert_eq!(json1, json2, "序列化→反序列化→再序列化须逐字节同态");
    }

    #[test]
    fn canonical_form_sorted_keys_and_indent() {
        let scene = demo_scene();
        let text = scene.to_json().unwrap();
        // 顶层 key 字典序:entities < name < next_id。
        assert!(text.starts_with("{\n  \"entities\":"), "顶层首键须为 entities:{text:.40}");
        assert!(text.contains("\n  \"name\": \"关卡一\","), "name 须两空格缩进");
        assert!(text.contains("\n  \"next_id\": 3\n}"), "next_id 须持久化");
        // props 内 key 亦排序(material < mesh)。
        let mi = text.find("\"material\"").unwrap();
        let me = text.find("\"mesh\"").unwrap();
        assert!(mi < me, "props key 须字典序");
        assert!(!text.ends_with('\n'), "to_json 不带末尾换行");
    }

    #[test]
    fn rxscene_save_load_byte_homomorphic() {
        let scene = demo_scene();
        let path = std::env::temp_dir().join(format!(
            "forge_scene_test_{}_save_load.rxscene",
            std::process::id()
        ));
        scene.save(&path).unwrap();
        let on_disk = std::fs::read_to_string(&path).unwrap();
        // 落盘 = 规范字节 + 末尾单换行。
        assert_eq!(on_disk, format!("{}\n", scene.to_json().unwrap()));
        assert!(!on_disk.ends_with("\n\n"), "末尾须单换行");
        // load→save 逐字节同态。
        let loaded = Scene::load(&path).unwrap();
        assert_eq!(loaded, scene);
        let path2 = std::env::temp_dir().join(format!(
            "forge_scene_test_{}_resave.rxscene",
            std::process::id()
        ));
        loaded.save(&path2).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), std::fs::read(&path2).unwrap());
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&path2);
    }

    #[test]
    fn next_id_survives_reload_no_collision() {
        let scene = demo_scene(); // 已分配 id 1、2
        let path = std::env::temp_dir().join(format!(
            "forge_scene_test_{}_nextid.rxscene",
            std::process::id()
        ));
        scene.save(&path).unwrap();
        let mut loaded = Scene::load(&path).unwrap();
        let new_id = loaded.alloc_id();
        assert_eq!(new_id, 3, "重载后 id 须接续计数,不撞号");
        assert!(loaded.entity(1).is_some() && loaded.entity(2).is_some());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn registry_lists_four_types_with_fields() {
        let v = list_types_json();
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 4);
        let names: Vec<&str> = arr.iter().filter_map(|t| t["name"].as_str()).collect();
        for want in ["MeshRenderer", "RigidBody", "Light", "Camera"] {
            assert!(names.contains(&want), "注册表缺 {want}");
        }
        let rb = arr.iter().find(|t| t["name"] == "RigidBody").unwrap();
        assert_eq!(rb["fields"][0]["name"], "kind");
        assert_eq!(rb["fields"][0]["type"], "enum:static|dynamic|kinematic");
    }

    #[test]
    fn validate_props_accepts_and_rejects() {
        assert!(validate_props("MeshRenderer", &json!({"mesh": "a", "material": "b"})).is_ok());
        assert!(validate_props("RigidBody", &json!({"kind": "static", "mass": 1.0})).is_ok());
        // 未知类型 / 缺字段 / 枚举越界 / 数组长度错 → Err。
        assert!(validate_props("NoSuch", &json!({})).is_err());
        assert!(validate_props("Camera", &json!({"fov": 60.0, "near": 0.1})).is_err());
        assert!(validate_props("RigidBody", &json!({"kind": "bad", "mass": 1.0})).is_err());
        assert!(validate_props("Light", &json!({"kind": "point", "color": [1.0, 0.0], "intensity": 1.0})).is_err());
        assert!(validate_props("Camera", &json!("not object")).is_err());
    }

    #[test]
    fn summary_reports_name_and_count() {
        let scene = demo_scene();
        let s = scene.summary();
        assert_eq!(s.name, "关卡一");
        assert_eq!(s.entity_count, 2);
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["entityCount"], 2);
        assert_eq!(v["name"], "关卡一");
    }
}
