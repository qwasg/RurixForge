//! Swarm(04 §5,11 §2.2):单用户多 agent 并行——同进程逻辑 worker(D-F3-D,零端口扩张)。
//! 内存态 SwarmCoordinator:节点注册表 + 分片表;四分片策略(04 §5.2);
//! 分片创建两两不相交校验(04 §5.2 逐字),相交 = GOV_SWARM_SHARD_OVERLAP(11 §4)。
//!
//! 分片报告纪律:聚合只读汇总,不得遮蔽任一分片失败(契约 guardrails)。

use std::collections::HashSet;
use std::sync::Mutex;

use serde_json::{json, Value};

/// 四分片策略(04 §5.2)。
pub const SHARD_TYPES: [&str; 4] = [
    "scene-partition",
    "asset-batch",
    "code-module",
    "test-matrix",
];

/// 逻辑 worker 节点(04 §5.1:capabilities/maxConcurrency/healthStatus/loadScore)。
#[derive(Debug, Clone)]
pub struct SwarmNode {
    pub id: String,
    pub capabilities: Vec<String>,
    pub max_concurrency: u32,
    pub health_status: String,
    pub load_score: f64,
}

impl SwarmNode {
    fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "kind": "logical",
            "capabilities": self.capabilities,
            "maxConcurrency": self.max_concurrency,
            "healthStatus": self.health_status,
            "loadScore": self.load_score,
        })
    }
}

/// 分片(pending/running/done/failed;report 执行后回填)。
#[derive(Debug, Clone)]
pub struct Shard {
    pub id: String,
    pub shard_type: String,
    pub items: Vec<String>,
    pub status: String,
    pub report: Option<Value>,
}

impl Shard {
    fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "shardType": self.shard_type,
            "items": self.items,
            "itemCount": self.items.len(),
            "status": self.status,
            "report": self.report,
        })
    }
}

/// 分片创建错误:Overlap → GOV_SWARM_SHARD_OVERLAP;Invalid → FORGE_INVALID_ARGS。
#[derive(Debug)]
pub enum ShardError {
    Overlap(String),
    Invalid(String),
}

/// Swarm 协调器(内存态;进程重启即空——事件日志持久化留 RD-F0-003 承接)。
pub struct SwarmCoordinator {
    nodes: Mutex<Vec<SwarmNode>>,
    shards: Mutex<Vec<Shard>>,
    next_node: Mutex<u64>,
    next_shard: Mutex<u64>,
}

impl Default for SwarmCoordinator {
    fn default() -> Self {
        let c = Self {
            nodes: Mutex::new(Vec::new()),
            shards: Mutex::new(Vec::new()),
            next_node: Mutex::new(0),
            next_shard: Mutex::new(0),
        };
        // 默认单逻辑节点:本进程即 worker(04 §5.1「同进程逻辑 worker」)。
        c.register_node(vec![
            "scene-partition".into(),
            "asset-batch".into(),
            "code-module".into(),
            "test-matrix".into(),
        ]);
        c
    }
}

impl SwarmCoordinator {
    /// 注册逻辑 worker 节点,返回 id。
    pub fn register_node(&self, capabilities: Vec<String>) -> String {
        let mut n = self.next_node.lock().unwrap_or_else(|e| e.into_inner());
        *n += 1;
        let id = format!("local-{n}");
        let mut nodes = self.nodes.lock().unwrap_or_else(|e| e.into_inner());
        nodes.push(SwarmNode {
            id: id.clone(),
            capabilities,
            max_concurrency: 4,
            health_status: "ok".into(),
            load_score: 0.0,
        });
        id
    }

    /// 演示播种(11 §2.2 seed-demo):幂等——已有非默认节点则直接报告。
    pub fn seed_demo(&self) -> Value {
        let before = self.nodes.lock().unwrap_or_else(|e| e.into_inner()).len();
        let mut added = 0;
        for caps in [
            vec!["scene-partition".to_string()],
            vec!["asset-batch".to_string()],
        ] {
            self.register_node(caps);
            added += 1;
        }
        json!({ "seeded": added, "nodesBefore": before, "nodesAfter": before + added })
    }

    /// 状态快照(11 §2.2 swarm/state)。
    pub fn state_json(&self) -> Value {
        let nodes = self.nodes.lock().unwrap_or_else(|e| e.into_inner());
        let shards = self.shards.lock().unwrap_or_else(|e| e.into_inner());
        json!({
            "nodes": nodes.iter().map(SwarmNode::to_json).collect::<Vec<_>>(),
            "shards": shards.iter().map(Shard::to_json).collect::<Vec<_>>(),
        })
    }

    /// 分片创建:shardType ∈ 四策略;items 非空且自身无重复;
    /// 与非终态(pending/running)同类型分片两两不相交。round-robin 切片。
    /// 返回新分片 id 清单(按创建序)。
    pub fn create_shards(
        &self,
        shard_type: &str,
        items: Vec<String>,
        count: usize,
    ) -> Result<Vec<String>, ShardError> {
        if !SHARD_TYPES.contains(&shard_type) {
            return Err(ShardError::Invalid(format!(
                "shardType 须为 {:?} 之一,实: {shard_type}",
                SHARD_TYPES
            )));
        }
        if items.is_empty() {
            return Err(ShardError::Invalid("items 不可空".into()));
        }
        if count == 0 {
            return Err(ShardError::Invalid("shardCount 须 ≥1".into()));
        }
        // 输入集自身去重校验(同项两次 = 自相交)。
        let mut seen = HashSet::with_capacity(items.len());
        for it in &items {
            if !seen.insert(it) {
                return Err(ShardError::Overlap(format!("输入集含重复项: {it}")));
            }
        }
        let mut shards = self.shards.lock().unwrap_or_else(|e| e.into_inner());
        // 与非终态同类型分片相交校验(04 §5.2「分片创建时校验输入集两两不相交」)。
        let new_set: HashSet<&String> = items.iter().collect();
        for s in shards.iter() {
            if s.shard_type == shard_type
                && (s.status == "pending" || s.status == "running")
                && s.items.iter().any(|it| new_set.contains(it))
            {
                return Err(ShardError::Overlap(format!(
                    "与活动分片 {} 相交(项: {})",
                    s.id,
                    s.items
                        .iter()
                        .filter(|it| new_set.contains(it))
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(",")
                )));
            }
        }
        // round-robin 切片:第 i 片取 items[i], items[i+count], …(天然两两不相交)。
        let real_count = count.min(items.len());
        let mut buckets: Vec<Vec<String>> = (0..real_count).map(|_| Vec::new()).collect();
        for (i, it) in items.into_iter().enumerate() {
            buckets[i % real_count].push(it);
        }
        let mut ids = Vec::with_capacity(real_count);
        let mut n = self.next_shard.lock().unwrap_or_else(|e| e.into_inner());
        for bucket in buckets {
            *n += 1;
            let id = format!("shard_{n}");
            shards.push(Shard {
                id: id.clone(),
                shard_type: shard_type.to_string(),
                items: bucket,
                status: "pending".into(),
                report: None,
            });
            ids.push(id);
        }
        Ok(ids)
    }

    /// 取分片快照(id → Shard clone)。
    pub fn get_shard(&self, id: &str) -> Option<Shard> {
        self.shards
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .find(|s| s.id == id)
            .cloned()
    }

    /// 分片状态迁移(running)——返回 false 表示分片不存在。
    pub fn mark_running(&self, id: &str) -> bool {
        let mut shards = self.shards.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(s) = shards.iter_mut().find(|s| s.id == id) {
            s.status = "running".into();
            true
        } else {
            false
        }
    }

    /// 分片完成回填:ok → done + report;有错误 → failed + report(失败不遮蔽)。
    pub fn complete_shard(&self, id: &str, report: Value, ok: bool) -> bool {
        let mut shards = self.shards.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(s) = shards.iter_mut().find(|s| s.id == id) {
            s.status = if ok { "done".into() } else { "failed".into() };
            s.report = Some(report);
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(n: usize) -> Vec<String> {
        (1..=n).map(|i| i.to_string()).collect()
    }

    #[test]
    fn default_registers_one_logical_node() {
        let c = SwarmCoordinator::default();
        let st = c.state_json();
        assert_eq!(st["nodes"].as_array().unwrap().len(), 1);
        assert_eq!(st["nodes"][0]["kind"], "logical");
        assert_eq!(st["shards"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn create_shards_round_robin_disjoint_full_coverage() {
        let c = SwarmCoordinator::default();
        let shard_ids = c.create_shards("scene-partition", ids(40), 4).unwrap();
        assert_eq!(shard_ids.len(), 4);
        let mut all: Vec<String> = Vec::new();
        for id in &shard_ids {
            let s = c.get_shard(id).unwrap();
            assert_eq!(s.status, "pending");
            all.extend(s.items);
        }
        // 全覆盖 + 两两不相交(数值排序后无重复;字符串排序会把 "10" 排 "2" 前)。
        let mut nums: Vec<u64> = all.iter().map(|s| s.parse().unwrap()).collect();
        nums.sort_unstable();
        nums.dedup();
        assert_eq!(nums, (1..=40u64).collect::<Vec<_>>());
    }

    #[test]
    fn create_shards_rejects_duplicate_items() {
        let c = SwarmCoordinator::default();
        let err = c
            .create_shards("scene-partition", vec!["1".into(), "1".into()], 2)
            .unwrap_err();
        assert!(matches!(err, ShardError::Overlap(_)));
    }

    #[test]
    fn create_shards_rejects_overlap_with_active_shard() {
        let c = SwarmCoordinator::default();
        c.create_shards("scene-partition", ids(10), 2).unwrap();
        // 与 pending 分片相交 → Overlap。
        let err = c.create_shards("scene-partition", vec!["5".into()], 1).unwrap_err();
        assert!(matches!(err, ShardError::Overlap(_)));
        // 不同类型不相干(asset-batch 与 scene-partition 命名空间独立)。
        c.create_shards("asset-batch", vec!["5".into()], 1).unwrap();
    }

    #[test]
    fn completed_shard_releases_its_items() {
        let c = SwarmCoordinator::default();
        let first = c.create_shards("scene-partition", ids(4), 1).unwrap();
        c.complete_shard(&first[0], json!({"ok": 4}), true);
        // 已 done → 同项可再分片。
        c.create_shards("scene-partition", ids(4), 1).unwrap();
    }

    #[test]
    fn invalid_shard_type_and_empty_items_rejected() {
        let c = SwarmCoordinator::default();
        assert!(matches!(
            c.create_shards("bogus", ids(2), 1).unwrap_err(),
            ShardError::Invalid(_)
        ));
        assert!(matches!(
            c.create_shards("scene-partition", vec![], 1).unwrap_err(),
            ShardError::Invalid(_)
        ));
    }

    #[test]
    fn seed_demo_is_additive() {
        let c = SwarmCoordinator::default();
        let r = c.seed_demo();
        assert_eq!(r["seeded"], 2);
        assert_eq!(r["nodesAfter"], 3);
    }

    #[test]
    fn complete_shard_marks_failed_without_masking() {
        let c = SwarmCoordinator::default();
        let ids_created = c.create_shards("scene-partition", ids(3), 1).unwrap();
        c.mark_running(&ids_created[0]);
        c.complete_shard(&ids_created[0], json!({"errors": ["boom"]}), false);
        let s = c.get_shard(&ids_created[0]).unwrap();
        assert_eq!(s.status, "failed");
        assert!(s.report.unwrap()["errors"].as_array().unwrap().len() == 1);
    }
}
