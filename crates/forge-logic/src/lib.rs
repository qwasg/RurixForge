//! forge-logic — Forge 交互逻辑(10 · 交互逻辑设计):
//! `.rxgraph` 图文档 serde(graph)+ 节点类型注册表(registry,10 §4.2 首发冻结子集)
//! + 全图校验器(validate,10 §5 保存即全图校验:悬空输入/类型不匹配/双环检测)。F4 wave.2。
//! F4 wave.3:interp 图解释执行运行时(10 §3.2 规范序:输入 → 接触(物理 contact + 逻辑
//! trigger)→ timer → update → message;跨实体按实体 id 升序,同实体多图按挂载序)。

pub mod graph;
pub mod interp;
pub mod registry;
pub mod validate;
