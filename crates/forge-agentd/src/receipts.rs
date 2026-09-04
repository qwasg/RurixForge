//! 后台子代理回执收件箱(D-036;multitask 异步委派的「送达」环节)。
//!
//! 为什么需要它:本仓的 turn 循环每轮只发 `[system, preamble, user]`,**不带对话历史**
//! (llm.rs run_tool_loop)。异步派出去的子代理即便跑完、事件也进了对话流,主 agent
//! 依然什么都看不见——回执必须显式落盘,再喂进主 agent 的上行消息,才叫「收到回执」。
//!
//! 三条送达路径(D-038),同一份收件箱、同一套「取件即消费」:
//! - 主 agent 正在跑 → 它的循环每迭代开头取件,以 user 消息插入(ToolLoopCfg.inbox);
//! - 主 agent 空闲 → 子代理终态触发唤醒轮,开轮时取件作 preamble 段;
//! - 都没赶上(进程重启清扫出的 failed 条目等)→ 下一轮用户发言开轮时取件。
//!
//! 纪律:
//! - 派发即写 `running` 条目(进程被杀后由启动清扫置 failed,前端卡片不会永远转圈);
//! - 注入即消费(`consumed=true`),同一条回执不重复喂;
//! - 预算截断只截「本轮注入哪几条」,没进注入的条目**不标消费**,留到下一次取件——
//!   不许出现「标了消费却没喂进去」的静默丢失(I-5)。

use std::path::{Path as FsPath, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::events::{new_id, now_rfc3339};

/// 单条回执状态(running = 后台仍在跑)。
pub const RECEIPT_TERMINAL: [&str; 3] = ["completed", "failed", "cancelled"];

/// 单条摘要注入上限(字符;超出如实截断标注)。
const PER_ITEM_CHARS: usize = 1200;
/// 整段注入上限(字符;放不下的条目留到下一轮,不丢)。
const SECTION_BUDGET_CHARS: usize = 6000;

/// 后台子代理回执(wire camelCase;runId = 后台 run,也是前端子代理卡片 id)。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Receipt {
    pub id: String,
    pub session_id: String,
    /// 后台 run id:取消走 `/api/forge/runs/{runId}/cancel`,前端卡片按它归组。
    pub run_id: String,
    /// 派发它的父 run(留痕:这条回执是哪一轮派出去的)。
    #[serde(default)]
    pub dispatched_by: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subagent_type: Option<String>,
    pub description: String,
    /// running | completed | failed | cancelled。
    pub status: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
    /// 已注入过主 agent 上下文。
    #[serde(default)]
    pub consumed: bool,
}

impl Receipt {
    pub fn is_terminal(&self) -> bool {
        RECEIPT_TERMINAL.contains(&self.status.as_str())
    }

    /// 注入段/事件面的中文状态词。
    fn status_label(&self) -> &'static str {
        match self.status.as_str() {
            "completed" => "已完成",
            "failed" => "失败",
            "cancelled" => "已取消",
            _ => "进行中",
        }
    }
}

/// 回执存贮:内存 Vec(创建序)+ receipts.json 整文件读-改-写(同 TodoStore 纪律)。
pub struct ReceiptStore {
    path: PathBuf,
    inner: Mutex<Vec<Receipt>>,
}

impl ReceiptStore {
    pub fn load(path: PathBuf) -> Self {
        let items = read_file(&path);
        ReceiptStore {
            path,
            inner: Mutex::new(items),
        }
    }

    fn persist_locked(&self, inner: &[Receipt]) {
        let doc = json!({ "receipts": inner });
        let text = serde_json::to_string_pretty(&doc).expect("receipts 序列化失败");
        if let Err(e) = write_atomic(&self.path, &text) {
            eprintln!("receipts.json 写盘失败({}): {e}", self.path.display());
        }
    }

    /// 派发登记(status=running)。
    pub fn begin(
        &self,
        session_id: &str,
        run_id: &str,
        dispatched_by: &str,
        subagent_type: Option<&str>,
        description: &str,
    ) -> Receipt {
        let r = Receipt {
            id: new_id("rcpt"),
            session_id: session_id.to_string(),
            run_id: run_id.to_string(),
            dispatched_by: dispatched_by.to_string(),
            subagent_type: subagent_type.map(str::to_string),
            description: description.to_string(),
            status: "running".to_string(),
            summary: String::new(),
            created_at: now_rfc3339(),
            finished_at: None,
            consumed: false,
        };
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.push(r.clone());
        self.persist_locked(&inner);
        r
    }

    /// 终态回填(按 runId);run 不存在 → None。
    pub fn finish(&self, run_id: &str, status: &str, summary: &str) -> Option<Receipt> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let r = inner.iter_mut().find(|r| r.run_id == run_id)?;
        r.status = status.to_string();
        r.summary = summary.to_string();
        r.finished_at = Some(now_rfc3339());
        let out = r.clone();
        self.persist_locked(&inner);
        Some(out)
    }

    /// 该会话待注入的回执(终态且未消费,创建序)。
    pub fn unconsumed(&self, session_id: &str) -> Vec<Receipt> {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|r| r.session_id == session_id && r.is_terminal() && !r.consumed)
            .cloned()
            .collect()
    }

    /// 标记已注入(只标真进了本轮 preamble 的那几条)。
    pub fn mark_consumed(&self, ids: &[String]) {
        if ids.is_empty() {
            return;
        }
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        for r in inner.iter_mut() {
            if ids.iter().any(|id| id == &r.id) {
                r.consumed = true;
            }
        }
        self.persist_locked(&inner);
    }

    /// 全量列举(测试断言用;生产取件一律走 unconsumed —— 对外可见面是事件流,
    /// 收件箱本身不开 REST,见 11 E-11-002)。
    #[cfg(test)]
    pub fn list_by_session(&self, session_id: &str) -> Vec<Receipt> {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|r| r.session_id == session_id)
            .cloned()
            .collect()
    }

    /// 崩溃恢复清扫:后台 run 只存内存,进程重启后残留的 running 条目一律是幽灵,
    /// 置 failed 并交回调用方补发 subagent.failed(否则前端卡片永远转圈)。
    pub fn sweep_running(&self, reason: &str) -> Vec<Receipt> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let mut swept = Vec::new();
        for r in inner.iter_mut() {
            if r.status == "running" {
                r.status = "failed".to_string();
                r.summary = reason.to_string();
                r.finished_at = Some(now_rfc3339());
                swept.push(r.clone());
            }
        }
        if !swept.is_empty() {
            self.persist_locked(&inner);
        }
        swept
    }
}

/// 注入段装配(纯函数,便于单测):返回 (段文本, 本轮实际注入的回执 id)。
/// 空输入 → (空串, 空)。超预算的条目不进本段也不返回 id——留到下一轮再喂。
pub fn injection_section(items: &[Receipt]) -> (String, Vec<String>) {
    if items.is_empty() {
        return (String::new(), Vec::new());
    }
    let head = "【后台子代理回执】以下是你此前派发、现已结束的后台子代理汇报(只给结果摘要,\
子代理的中间过程不进本轮上下文)。据此判断还要不要补派或收尾,别重复已经做完的事:\n";
    let mut out = String::from(head);
    let mut used: Vec<String> = Vec::new();
    let mut skipped = 0usize;
    for r in items {
        let mut summary = r.summary.trim().to_string();
        if summary.is_empty() {
            summary = "(子代理未给出汇报正文)".to_string();
        }
        if summary.chars().count() > PER_ITEM_CHARS {
            summary = summary.chars().take(PER_ITEM_CHARS).collect::<String>();
            summary.push_str("…(摘要过长,已截断)");
        }
        let role = match r.subagent_type.as_deref() {
            Some(t) if !t.is_empty() => format!("(工种 {t})"),
            _ => String::new(),
        };
        let line = format!(
            "{}. [{}] {}{}:{}\n",
            used.len() + 1,
            r.status_label(),
            r.description,
            role,
            summary
        );
        if out.chars().count() + line.chars().count() > SECTION_BUDGET_CHARS {
            skipped += 1;
            continue;
        }
        out.push_str(&line);
        used.push(r.id.clone());
    }
    if used.is_empty() {
        return (String::new(), Vec::new());
    }
    if skipped > 0 {
        out.push_str(&format!(
            "(另有 {skipped} 条回执因本轮上下文预算未列出,将在下一轮送达)\n"
        ));
    }
    (out, used)
}

fn read_file(path: &FsPath) -> Vec<Receipt> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let v: Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("receipts.json 解析失败({}): {e},按空处理", path.display());
            return Vec::new();
        }
    };
    v.get("receipts")
        .and_then(Value::as_array)
        .cloned()
        .or_else(|| v.as_array().cloned())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|item| {
            serde_json::from_value::<Receipt>(item)
                .map_err(|e| eprintln!("receipts.json 条目解析失败: {e}"))
                .ok()
        })
        .collect()
}

/// 原子写:tmp 全量写 + rename(与 sessions.rs / agent.rs 同纪律)。
fn write_atomic(path: &FsPath, text: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store(tag: &str) -> (ReceiptStore, PathBuf) {
        let dir = std::env::temp_dir().join(format!("forge-receipts-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("receipts.json");
        (ReceiptStore::load(path), dir)
    }

    #[test]
    fn begin_finish_and_unconsumed_roundtrip() {
        let (store, dir) = temp_store("roundtrip");
        let r = store.begin("s1", "run_1", "run_parent", Some("scene-builder"), "摆放僵尸");
        assert_eq!(r.status, "running");
        // running 不进注入面(还没结束,没什么可汇报)。
        assert!(store.unconsumed("s1").is_empty());
        assert_eq!(store.list_by_session("s1").len(), 1);

        store.finish("run_1", "completed", "已摆 5 个僵尸,资产 Content/Zombies/*.rxsprite");
        let pending = store.unconsumed("s1");
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].status, "completed");

        // 注入即消费,不重复喂。
        store.mark_consumed(&[pending[0].id.clone()]);
        assert!(store.unconsumed("s1").is_empty());

        // 落盘可读回(同路径重开)。
        let reopened = ReceiptStore::load(dir.join("receipts.json"));
        let all = reopened.list_by_session("s1");
        assert_eq!(all.len(), 1);
        assert!(all[0].consumed);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unconsumed_is_session_scoped() {
        let (store, dir) = temp_store("scoped");
        store.begin("s1", "run_1", "p", None, "甲");
        store.begin("s2", "run_2", "p", None, "乙");
        store.finish("run_1", "completed", "甲完");
        store.finish("run_2", "completed", "乙完");
        assert_eq!(store.unconsumed("s1").len(), 1);
        assert_eq!(store.unconsumed("s2").len(), 1);
        assert_eq!(store.unconsumed("s1")[0].description, "甲");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn sweep_running_marks_failed_for_crash_recovery() {
        let (store, dir) = temp_store("sweep");
        store.begin("s1", "run_1", "p", None, "跑到一半");
        store.begin("s1", "run_2", "p", None, "已完成的");
        store.finish("run_2", "completed", "好了");
        let swept = store.sweep_running("agentd 进程重启,后台子代理中断");
        assert_eq!(swept.len(), 1);
        assert_eq!(swept[0].run_id, "run_1");
        assert_eq!(swept[0].status, "failed");
        // 幂等:再扫一次没有 running 了。
        assert!(store.sweep_running("x").is_empty());
        // 清扫后的失败条目照样要送达主 agent(失败也是信息,不遮蔽)。
        assert_eq!(store.unconsumed("s1").len(), 2);
        std::fs::remove_dir_all(&dir).ok();
    }

    fn fake(id: &str, status: &str, desc: &str, summary: &str) -> Receipt {
        Receipt {
            id: id.to_string(),
            session_id: "s".into(),
            run_id: format!("run_{id}"),
            dispatched_by: "p".into(),
            subagent_type: Some("qa-tester".into()),
            description: desc.into(),
            status: status.into(),
            summary: summary.into(),
            created_at: String::new(),
            finished_at: None,
            consumed: false,
        }
    }

    #[test]
    fn injection_section_lists_status_role_and_summary() {
        let (text, used) = injection_section(&[
            fake("a", "completed", "摆场景", "5 个实体已建"),
            fake("b", "failed", "跑测试", "断言 3 失败"),
        ]);
        assert_eq!(used, vec!["a".to_string(), "b".to_string()]);
        assert!(text.contains("后台子代理回执"));
        assert!(text.contains("1. [已完成] 摆场景(工种 qa-tester):5 个实体已建"));
        assert!(text.contains("2. [失败] 跑测试(工种 qa-tester):断言 3 失败"));
        // 空输入不产生空段。
        assert_eq!(injection_section(&[]).0, "");
    }

    #[test]
    fn injection_section_truncates_long_summary_and_defers_overflow() {
        let long = "字".repeat(PER_ITEM_CHARS + 500);
        let (text, used) = injection_section(&[fake("a", "completed", "长报告", &long)]);
        assert_eq!(used.len(), 1);
        assert!(text.contains("已截断"));
        assert!(text.chars().count() < PER_ITEM_CHARS + 300);

        // 多条撑爆整段预算:放不下的不返回 id(留到下一轮),并如实标注条数。
        let items: Vec<Receipt> = (0..10)
            .map(|i| fake(&format!("r{i}"), "completed", "批量", &"字".repeat(900)))
            .collect();
        let (text2, used2) = injection_section(&items);
        assert!(used2.len() < items.len(), "应有条目被预算挡下: {}", used2.len());
        assert!(text2.contains("因本轮上下文预算未列出"));
        assert!(text2.chars().count() <= SECTION_BUDGET_CHARS + 80);
    }

    #[test]
    fn injection_section_fills_placeholder_for_empty_summary() {
        let (text, _) = injection_section(&[fake("a", "cancelled", "半路取消", "  ")]);
        assert!(text.contains("[已取消]"));
        assert!(text.contains("子代理未给出汇报正文"));
    }
}
