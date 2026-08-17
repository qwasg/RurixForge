//! `.rxgraph` 图文档 serde 结构(10 §4.1):version/id/name/exposedProps/nodes/edges。
//! 执行边(edges 数组)与数据边分离;`inputs` 内联数据边,值来源三态 const|node+pin|ref。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 暴露属性种类(D-F4-E 冻结子集)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PropKind {
    F32,
    I32,
    Bool,
    String,
    Vec3,
}

/// 暴露属性(Inspector 可编辑;图内经 `{"ref": name}` 引用)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExposedProp {
    pub name: String,
    pub kind: PropKind,
    pub default: Value,
}

/// 值来源三态(10 §4.1):`{"const": v}` 常量 / `{"node": id, "pin": name}` 数据边 /
/// `{"ref": name}` 暴露属性。untagged 依键判别。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ValueSource {
    Const {
        #[serde(rename = "const")]
        konst: Value,
    },
    NodePin {
        node: String,
        pin: String,
    },
    Ref {
        #[serde(rename = "ref")]
        refr: String,
    },
}

/// 图节点(id + 注册表类型 + 画布坐标 + 内联数据输入)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    #[serde(rename = "type")]
    pub ntype: String,
    pub pos: [f64; 2],
    /// 内联数据输入:BTreeMap(key 字典序)保确定性序列化(serde_json::Map 方法仅
    /// 实现于 Map<String, Value>,不可复用)。
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub inputs: BTreeMap<String, ValueSource>,
}

/// 执行边(UE 白色执行引脚):from = [节点 id, 执行出口 pin],to = [节点 id, "exec"]。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    pub from: [String; 2],
    pub to: [String; 2],
}

/// `.rxgraph` 文档(10 §4.1 图结构)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphDoc {
    pub version: u32,
    pub id: String,
    pub name: String,
    #[serde(rename = "exposedProps", default)]
    pub exposed_props: Vec<ExposedProp>,
    #[serde(default)]
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub edges: Vec<Edge>,
}

impl GraphDoc {
    /// 从 JSON 字串反序列化(容忍任意排版)。
    pub fn from_json(s: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(s)
    }

    /// 确定性 JSON 序列化:serde_json 缺省 Map 为 BTreeMap(key 字典序),
    /// pretty 即两空格缩进;结构体字段序 = 声明序(与 10 §4.1 示例一致)。跨进程逐字节确定。
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn value_source_three_states_roundtrip() {
        let c: ValueSource = serde_json::from_value(json!({ "const": 1.2 })).unwrap();
        assert_eq!(c, ValueSource::Const { konst: json!(1.2) });
        let n: ValueSource = serde_json::from_value(json!({ "node": "n3", "pin": "out" })).unwrap();
        assert_eq!(
            n,
            ValueSource::NodePin { node: "n3".into(), pin: "out".into() }
        );
        let r: ValueSource = serde_json::from_value(json!({ "ref": "openSpeed" })).unwrap();
        assert_eq!(r, ValueSource::Ref { refr: "openSpeed".into() });
        // 回序列化键名保持 const/ref。
        assert_eq!(serde_json::to_value(&c).unwrap(), json!({ "const": 1.2 }));
        assert_eq!(serde_json::to_value(&r).unwrap(), json!({ "ref": "openSpeed" }));
    }

    #[test]
    fn graph_doc_roundtrip_byte_identical() {
        let text = r#"{
  "version": 1,
  "id": "g_door",
  "name": "DoorOpener",
  "exposedProps": [
    {
      "name": "openSpeed",
      "kind": "F32",
      "default": 90.0
    }
  ],
  "nodes": [
    {
      "id": "n1",
      "type": "event.on_trigger_enter",
      "pos": [40.0, 80.0]
    }
  ],
  "edges": []
}"#;
        let doc = GraphDoc::from_json(text).unwrap();
        assert_eq!(doc.version, 1);
        assert_eq!(doc.exposed_props[0].kind, PropKind::F32);
        assert_eq!(doc.nodes[0].ntype, "event.on_trigger_enter");
        let out = doc.to_json().unwrap();
        let doc2 = GraphDoc::from_json(&out).unwrap();
        assert_eq!(doc, doc2);
        assert_eq!(out, doc2.to_json().unwrap(), "序列化须逐字节同态");
        assert!(out.contains("\n  \"version\": 1,"), "两空格缩进: {out:.60}");
        assert!(out.contains("\n  \"exposedProps\":"), "camelCase 键名");
    }
}
