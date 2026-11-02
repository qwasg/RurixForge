//! Evidence of the requirement text actually returned to the Demo builder.
use serde_json::Value;
use std::{
    collections::BTreeMap,
    ops::Range,
    path::{Path, PathBuf},
};

const ERROR: &str = "ULTRAPLAN_REQUIREMENTS_INCOMPLETE";

struct File {
    relative: String,
    path: PathBuf,
    text: String,
}
struct Chunk {
    file: File,
    seen: Vec<bool>,
    empty_read: bool,
}
struct Source {
    file: File,
    chunks: Vec<(usize, usize)>,
}

pub(crate) struct DemoRequirements {
    root: PathBuf,
    manifest: Vec<u8>,
    chunks: Vec<Chunk>,
    sources: Vec<Source>,
    invalid: Option<String>,
}

impl DemoRequirements {
    pub(crate) fn load(root: &Path) -> Self {
        Self::read_pack(root).unwrap_or_else(|error| Self {
            root: root.to_path_buf(),
            manifest: vec![],
            chunks: vec![],
            sources: vec![],
            invalid: Some(error),
        })
    }

    fn read_pack(root: &Path) -> Result<Self, String> {
        let root = root.canonicalize().map_err(|e| e.to_string())?;
        let manifest = std::fs::read(root.join("requirements.json")).map_err(|e| e.to_string())?;
        let doc: Value = serde_json::from_slice(&manifest).map_err(|e| e.to_string())?;
        let mut result = Self {
            root,
            manifest,
            chunks: vec![],
            sources: vec![],
            invalid: None,
        };
        let mut sources = BTreeMap::<String, Vec<(usize, usize)>>::new();
        for entry in doc["files"].as_array().ok_or("需求文件列表缺失")? {
            let relative = entry["path"].as_str().ok_or("需求分段路径缺失")?;
            if !relative.starts_with("_requirements/chunks/") {
                return Err("需求分段路径无效".into());
            }
            let file = read_file(&result.root, relative)?;
            if entry["sha256"].as_str()
                != Some(forge_util::hashutil::sha256_hex(file.text.as_bytes()).as_str())
                || result.chunks.iter().any(|c| c.file.path == file.path)
            {
                return Err(format!("需求分段摘要或路径无效: {relative}"));
            }
            let id = result.chunks.len();
            result.chunks.push(Chunk {
                seen: vec![false; file.text.len()],
                file,
                empty_read: false,
            });
            if let Some(source) = entry["source"].as_str() {
                let index = entry["index"].as_u64().ok_or("需求分段序号缺失")? as usize;
                sources.entry(source.into()).or_default().push((index, id));
            }
        }
        if result.chunks.is_empty() {
            return Err("需求文件列表为空".into());
        }
        for (relative, mut parts) in sources {
            if !relative.starts_with("_requirements/") {
                return Err("需求原文路径无效".into());
            }
            let file = read_file(&result.root, &relative)?;
            parts.sort_unstable();
            let mut combined = String::new();
            let mut chunks = Vec::new();
            for (expected, (index, id)) in parts.into_iter().enumerate() {
                if expected != index {
                    return Err("需求分段序号不连续".into());
                }
                chunks.push((id, combined.len()));
                combined.push_str(&result.chunks[id].file.text);
            }
            if combined != file.text {
                return Err(format!("需求原文与分段不匹配: {relative}"));
            }
            result.sources.push(Source { file, chunks });
        }
        Ok(result)
    }

    /// Invoke only after a successful native read_file. Match its line semantics,
    /// and never credit lines beyond the feedback shown to the model.
    pub(crate) fn record_read(&mut self, root: &Path, args: &Value, output: &str) {
        if self.invalid.is_some() || root.canonicalize().ok().as_ref() != Some(&self.root) {
            return;
        }
        let Some(path) = args["path"]
            .as_str()
            .and_then(|s| root.join(s.trim()).canonicalize().ok())
        else {
            return;
        };
        if let Some(id) = self.chunks.iter().position(|c| c.file.path == path) {
            let ranges = visible_lines(&self.chunks[id].file.text, args, output);
            if let Some(ranges) = ranges {
                self.mark(id, 0, &ranges);
                self.chunks[id].empty_read = self.chunks[id].file.text.is_empty();
            }
        } else if let Some(source) = self.sources.iter().find(|s| s.file.path == path) {
            if let Some(ranges) = visible_lines(&source.file.text, args, output) {
                let chunks = source.chunks.clone();
                let empty = source.file.text.is_empty();
                for (id, offset) in chunks {
                    self.mark(id, offset, &ranges);
                    if empty {
                        self.chunks[id].empty_read = true;
                    }
                }
            }
        }
    }

    fn mark(&mut self, id: usize, offset: usize, ranges: &[Range<usize>]) {
        let seen = &mut self.chunks[id].seen;
        for range in ranges {
            let start = range.start.saturating_sub(offset).min(seen.len());
            let end = range.end.saturating_sub(offset).min(seen.len());
            seen[start..end].fill(true);
        }
    }

    pub(crate) fn ensure_complete(&self, root: &Path) -> Result<(), String> {
        let fail = |message: String| Err(format!("{ERROR}: {message}"));
        if let Some(error) = &self.invalid {
            return fail(format!("需求包无效: {error}"));
        }
        if root.canonicalize().ok().as_ref() != Some(&self.root)
            || std::fs::read(root.join("requirements.json")).ok().as_ref() != Some(&self.manifest)
            || self
                .chunks
                .iter()
                .map(|c| &c.file)
                .chain(self.sources.iter().map(|s| &s.file))
                .any(|file| {
                    read_file(&self.root, &file.relative).map_or(true, |current| {
                        current.path != file.path || current.text != file.text
                    })
                })
        {
            return fail("需求包已变更，请重新启动本轮 Demo".into());
        }
        let missing: Vec<_> = self
            .chunks
            .iter()
            .filter(|c| {
                if c.seen.is_empty() {
                    !c.empty_read
                } else {
                    c.seen.iter().any(|seen| !seen)
                }
            })
            .collect();
        if missing.is_empty() {
            return Ok(());
        }
        fail(format!(
            "尚有 {}/{} 个需求分段未读完；请用 read_file 全文读取: {}{}",
            missing.len(),
            self.chunks.len(),
            missing
                .iter()
                .take(3)
                .map(|c| c.file.relative.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            if missing.len() > 3 {
                "（其余分段见 requirements.json）"
            } else {
                ""
            }
        ))
    }
}

fn read_file(root: &Path, relative: &str) -> Result<File, String> {
    if relative.contains([':', '\\'])
        || relative
            .split('/')
            .any(|s| s.is_empty() || s == "." || s == "..")
    {
        return Err("需求路径无效".into());
    }
    let path = root
        .join(relative)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if !path.starts_with(root) {
        return Err("需求路径越界".into());
    }
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    Ok(File {
        relative: relative.into(),
        path,
        text,
    })
}

fn number(args: &Value, key: &str) -> Option<usize> {
    args[key]
        .as_u64()
        .map(|n| n as usize)
        .or_else(|| args[key].as_str()?.trim().parse().ok())
}

fn visible_lines(text: &str, args: &Value, output: &str) -> Option<Vec<Range<usize>>> {
    let lines: Vec<_> = text.lines().collect();
    let (offset, limit) = (number(args, "offset"), number(args, "limit"));
    let window = offset.is_some() || limit.is_some();
    let start = if window {
        offset
            .unwrap_or(1)
            .max(1)
            .saturating_sub(1)
            .min(lines.len())
    } else {
        0
    };
    let end = if window {
        limit
            .filter(|n| *n > 0)
            .map_or(lines.len(), |n| start.saturating_add(n).min(lines.len()))
    } else {
        lines.len()
    };
    let expected = if window {
        lines[start..end].join("\n")
    } else {
        text.to_string()
    };
    if expected != output {
        return None;
    }
    let mut rendered = 0;
    let mut ranges = Vec::new();
    for index in start..end {
        let begin = lines[index].as_ptr() as usize - text.as_ptr() as usize;
        let finish = lines.get(index + 1).map_or(text.len(), |next| {
            next.as_ptr() as usize - text.as_ptr() as usize
        });
        rendered += if window {
            usize::from(index > start) + lines[index].chars().count()
        } else {
            text[begin..finish].chars().count()
        };
        if rendered > crate::llm::TOOL_FEEDBACK_MAX {
            break;
        }
        ranges.push(begin..finish);
    }
    Some(ranges)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    struct Fixture(PathBuf);
    impl Fixture {
        fn new(texts: &[&str]) -> Self {
            let root = std::env::temp_dir().join(format!(
                "demo-read-{}-{}",
                std::process::id(),
                crate::events::new_id("t")
            ));
            std::fs::create_dir_all(root.join("_requirements/chunks")).unwrap();
            let mut files = Vec::new();
            for (id, text) in texts.iter().enumerate() {
                let path = format!("_requirements/chunks/{id}.txt");
                std::fs::write(root.join(&path), text).unwrap();
                files.push(json!({"path":path,"source":"_requirements/source.txt","index":id,"sha256":forge_util::hashutil::sha256_hex(text.as_bytes())}));
            }
            std::fs::write(root.join("_requirements/source.txt"), texts.concat()).unwrap();
            std::fs::write(
                root.join("requirements.json"),
                json!({"files":files}).to_string(),
            )
            .unwrap();
            Self(root)
        }
        fn read(
            &self,
            tracker: &mut DemoRequirements,
            path: &str,
            offset: Option<usize>,
            limit: Option<usize>,
        ) {
            let mut args = json!({"path":path});
            if let Some(n) = offset {
                args["offset"] = json!(n);
            }
            if let Some(n) = limit {
                args["limit"] = json!(n);
            }
            let text = std::fs::read_to_string(self.0.join(path)).unwrap();
            let output = if offset.is_some() || limit.is_some() {
                text.lines()
                    .skip(offset.unwrap_or(1).max(1) - 1)
                    .take(limit.filter(|n| *n > 0).unwrap_or(usize::MAX))
                    .collect::<Vec<_>>()
                    .join("\n")
            } else {
                text
            };
            tracker.record_read(&self.0, &args, &output);
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            assert!(self.0.starts_with(std::env::temp_dir()));
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn cumulative_windows_handle_out_of_order_gaps_and_zero() {
        let f = Fixture::new(&["one\r\ntwo\r\nthree\r\n"]);
        let mut tracker = DemoRequirements::load(&f.0);
        f.read(&mut tracker, "_requirements/chunks/0.txt", Some(3), Some(0));
        f.read(&mut tracker, "_requirements/chunks/0.txt", Some(0), Some(1));
        assert!(tracker.ensure_complete(&f.0).unwrap_err().contains("1/1"));
        f.read(&mut tracker, "_requirements/chunks/0.txt", Some(2), Some(1));
        assert!(tracker.ensure_complete(&f.0).is_ok());
    }

    #[test]
    fn originals_credit_chunks_and_empty_chunks_require_a_read() {
        let f = Fixture::new(&["first\n", "second\n"]);
        let mut tracker = DemoRequirements::load(&f.0);
        f.read(&mut tracker, "_requirements/source.txt", None, None);
        assert!(tracker.ensure_complete(&f.0).is_ok());
        let f = Fixture::new(&[""]);
        let mut tracker = DemoRequirements::load(&f.0);
        assert!(tracker.ensure_complete(&f.0).is_err());
        f.read(&mut tracker, "_requirements/chunks/0.txt", None, None);
        assert!(tracker.ensure_complete(&f.0).is_ok());
    }

    #[test]
    fn truncated_originals_outside_paths_and_failed_output_do_not_complete() {
        let long = "长".repeat(crate::llm::TOOL_FEEDBACK_MAX + 1);
        let split = long.char_indices().nth(2000).unwrap().0;
        let f = Fixture::new(&[&long[..split], &long[split..]]);
        let mut tracker = DemoRequirements::load(&f.0);
        f.read(&mut tracker, "_requirements/source.txt", None, None);
        tracker.record_read(
            &f.0,
            &json!({"path":"_requirements/chunks/0.txt"}),
            "PATH_NOT_FOUND",
        );
        tracker.record_read(&f.0, &json!({"path":"../outside.txt"}), "unrelated");
        let error = tracker.ensure_complete(&f.0).unwrap_err();
        assert!(error.contains("2/2") && error.contains("_requirements/chunks/0.txt"));
    }

    #[test]
    fn source_windows_and_chunk_windows_share_coverage_across_split_lines() {
        let f = Fixture::new(&["first\nsec", "ond\nthird\n"]);
        let mut tracker = DemoRequirements::load(&f.0);
        f.read(&mut tracker, "_requirements/source.txt", Some(2), Some(1));
        f.read(&mut tracker, "_requirements/chunks/1.txt", Some(2), Some(1));
        assert!(tracker.ensure_complete(&f.0).unwrap_err().contains("1/2"));
        f.read(&mut tracker, "_requirements/source.txt", Some(1), Some(1));
        assert!(tracker.ensure_complete(&f.0).is_ok());
    }

    #[test]
    fn pack_tampering_and_invalid_manifest_fail_closed() {
        let f = Fixture::new(&["first"]);
        let mut tracker = DemoRequirements::load(&f.0);
        f.read(&mut tracker, "_requirements/chunks/0.txt", None, Some(0));
        assert!(tracker.ensure_complete(&f.0).is_ok());
        std::fs::write(f.0.join("_requirements/chunks/0.txt"), "changed").unwrap();
        assert!(tracker
            .ensure_complete(&f.0)
            .unwrap_err()
            .contains("已变更"));
        assert!(DemoRequirements::load(&f.0).ensure_complete(&f.0).is_err());
        std::fs::write(f.0.join("requirements.json"), "{}").unwrap();
        assert!(DemoRequirements::load(&f.0)
            .ensure_complete(&f.0)
            .unwrap_err()
            .contains("需求包无效"));
    }

    /// Opt-in replay of a retained attempt and its host events. Event previews
    /// are checked against immutable package text before reconstructing reads.
    #[test]
    #[ignore = "requires FORGE_DEMO_REPLAY_ROOT and FORGE_DEMO_REPLAY_EVENTS"]
    fn replay_recorded_original_and_chunk_reads() {
        let root = PathBuf::from(
            std::env::var_os("FORGE_DEMO_REPLAY_ROOT").expect("FORGE_DEMO_REPLAY_ROOT"),
        );
        let events = PathBuf::from(
            std::env::var_os("FORGE_DEMO_REPLAY_EVENTS").expect("FORGE_DEMO_REPLAY_EVENTS"),
        );
        let selected = std::env::var("FORGE_DEMO_REPLAY_AGENT").ok();
        let mut tracker = DemoRequirements::load(&root);
        assert!(
            tracker.invalid.is_none(),
            "retained requirement package must validate"
        );
        let mut pending = BTreeMap::new();
        let (mut originals, mut chunks) = (0, 0);
        for line in std::fs::read_to_string(&events).unwrap().lines() {
            let event: Value = serde_json::from_str(line).unwrap();
            let payload = &event["payload"];
            let actor = payload["agentId"]
                .as_str()
                .or(event["source"]["actor"].as_str())
                .unwrap_or("");
            if selected.as_deref().is_some_and(|id| id != actor) || payload["name"] != "read_file" {
                continue;
            }
            let Some(id) = payload["toolCallId"]
                .as_str()
                .or(payload["callId"].as_str())
            else {
                continue;
            };
            let key = (actor.to_string(), id.to_string());
            if event["type"] == "agent.tool.invoked" {
                let args = payload["args"]
                    .as_str()
                    .map(|text| serde_json::from_str(text).unwrap())
                    .unwrap_or_else(|| payload["args"].clone());
                pending.insert(key, args);
                continue;
            }
            if event["type"] != "agent.tool.completed" || payload["ok"] == false {
                continue;
            }
            let Some(args) = pending.remove(&key) else {
                continue;
            };
            let Some(path) = args["path"]
                .as_str()
                .and_then(|p| root.join(p.trim()).canonicalize().ok())
            else {
                continue;
            };
            let chunk = tracker
                .chunks
                .iter()
                .find(|c| c.file.path == path)
                .map(|c| &c.file);
            let source = tracker
                .sources
                .iter()
                .find(|s| s.file.path == path)
                .map(|s| &s.file);
            let Some(file) = chunk.or(source) else {
                continue;
            };
            let (offset, limit) = (number(&args, "offset"), number(&args, "limit"));
            let expected = if offset.is_some() || limit.is_some() {
                file.text
                    .lines()
                    .skip(offset.unwrap_or(1).max(1) - 1)
                    .take(limit.filter(|n| *n > 0).unwrap_or(usize::MAX))
                    .collect::<Vec<_>>()
                    .join("\n")
            } else {
                file.text.clone()
            };
            let output = replay_output(&payload["output"], events.parent().unwrap());
            let preview = |text: &str| {
                let count = text.chars().count();
                if count <= 2000 {
                    text.to_string()
                } else {
                    format!(
                        "{}…\n（+{} 字已省略）",
                        text.chars().take(2000).collect::<String>(),
                        count - 2000
                    )
                }
            };
            assert!(
                output == expected || output == preview(&expected),
                "recorded read output differs from retained package"
            );
            if chunk.is_some() {
                chunks += 1;
            } else {
                originals += 1;
            }
            tracker.record_read(&root, &args, &expected);
        }
        assert!(
            originals > 0 && chunks > 0,
            "replay must exercise original and chunk reads"
        );
        tracker.ensure_complete(&root).unwrap();
        eprintln!("validated replay: {originals} original reads and {chunks} chunk reads");
    }

    fn replay_output(value: &Value, events_dir: &Path) -> String {
        if let Some(text) = value.as_str().or(value["text"].as_str()) {
            return text.into();
        }
        let path = value["path"]
            .as_str()
            .or(value["artifactPath"].as_str())
            .or(value["artifact"]["path"].as_str())
            .expect("output artifact requires text or a readable path");
        std::fs::read_to_string(events_dir.join(path)).expect("read output artifact")
    }
}
