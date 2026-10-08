//! Proposal(确认单,12 §3 / 11 §4):两阶段治理——dry-run 收集影响面 → pending →
//! 批准/拒绝;destructive 工具(asset_delete force=true)无 approved Proposal 一律
//! GOV_PROPOSAL_REQUIRED(I-6,full-auto 也不豁免)。
//!
//! 进程级内存态(会话挂起等待语义;持久化/replay 留 F3 事件日志波)。

use std::sync::Mutex;

use serde_json::{json, Value};

/// 单条 Proposal(11 §4 DTO)。
#[derive(Debug, Clone)]
pub struct Proposal {
    pub id: String,
    pub kind: String,
    pub summary: String,
    pub impact: Value,
    /// pending / approved / rejected(终态不可逆)。
    pub status: String,
    pub created_by: Value,
}

impl Proposal {
    pub fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "kind": self.kind,
            "summary": self.summary,
            "impact": self.impact,
            "status": self.status,
            "createdBy": self.created_by,
        })
    }
}

/// Proposal 存贮(内存;id 单调)。
#[derive(Default)]
pub struct ProposalStore {
    inner: Mutex<Vec<Proposal>>,
    next: Mutex<u64>,
}

impl ProposalStore {
    /// 创建 pending Proposal,返回 id。
    pub fn create(&self, kind: &str, summary: String, impact: Value, created_by: Value) -> String {
        let mut n = self.next.lock().unwrap_or_else(|e| e.into_inner());
        *n += 1;
        let id = format!("prop_{n}");
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.push(Proposal {
            id: id.clone(),
            kind: kind.to_string(),
            summary,
            impact,
            status: "pending".into(),
            created_by,
        });
        id
    }

    pub fn list(&self) -> Vec<Proposal> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn get(&self, id: &str) -> Option<Proposal> {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .find(|p| p.id == id)
            .cloned()
    }

    /// 状态迁移:pending → approved|rejected;已终态 → Err(当前状态)。
    pub fn transition(&self, id: &str, action: &str) -> Result<Proposal, String> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let p = inner
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or_else(|| format!("NOT_FOUND:{id}"))?;
        if p.status != "pending" {
            return Err(format!("CLOSED:{}", p.status));
        }
        p.status = match action {
            "approve" => "approved".into(),
            "reject" => "rejected".into(),
            other => return Err(format!("INVALID_ACTION:{other}")),
        };
        Ok(p.clone())
    }

    /// 有无 approved Proposal 覆盖给定资产清单(kind 匹配且 impact.assets ⊇ paths)。
    pub fn has_approved_covering(&self, kind: &str, paths: &[String]) -> bool {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.iter().any(|p| {
            p.kind == kind
                && p.status == "approved"
                && p.impact
                    .get("assets")
                    .and_then(Value::as_array)
                    .map(|assets| {
                        paths
                            .iter()
                            .all(|want| assets.iter().any(|a| a.as_str() == Some(want)))
                    })
                    .unwrap_or(false)
        })
    }
}
