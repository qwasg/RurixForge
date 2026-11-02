//! Read-only toolchain discovery shared by authoring, runtime and packaging.
//! An explicit override is authoritative, including when it is invalid.

use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug)]
pub enum ToolchainTool {
    Rx,
    Rurixc,
}

impl ToolchainTool {
    pub fn env_key(self) -> &'static str {
        match self {
            Self::Rx => "FORGE_RX_CLI",
            Self::Rurixc => "FORGE_RURIXC",
        }
    }

    pub fn file_name(self) -> &'static str {
        match (self, cfg!(windows)) {
            (Self::Rx, true) => "rx.exe",
            (Self::Rx, false) => "rx",
            (Self::Rurixc, true) => "rurixc.exe",
            (Self::Rurixc, false) => "rurixc",
        }
    }
}

#[derive(Clone, Debug)]
pub struct ToolchainResolution {
    pub path: Option<PathBuf>,
    pub source: Option<&'static str>,
    pub attempted: Vec<PathBuf>,
    pub error: Option<String>,
}

impl ToolchainResolution {
    pub fn into_result(self) -> Result<PathBuf, String> {
        self.path
            .ok_or_else(|| self.error.unwrap_or_else(|| "RX_CLI_NOT_FOUND".into()))
    }
}

pub fn resolve(tool: ToolchainTool) -> ToolchainResolution {
    resolve_for_engine(tool, None)
}

/// `engine` selects the packaged engine's sibling directory when packing a game.
/// Runtime and MCP processes otherwise share FORGE_ENGINE_HOST_BIN/current_exe.
pub fn resolve_for_engine(tool: ToolchainTool, engine: Option<&Path>) -> ToolchainResolution {
    let override_path = std::env::var_os(tool.env_key()).map(PathBuf::from);
    let engine = engine
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("FORGE_ENGINE_HOST_BIN")
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
        })
        .or_else(|| std::env::current_exe().ok());
    resolve_from(
        tool,
        override_path.as_deref(),
        engine.as_deref(),
        &crate::roots::forge_root(),
    )
}

/// Explicit inputs make precedence testable without mutating process environment.
pub fn resolve_from(
    tool: ToolchainTool,
    override_path: Option<&Path>,
    engine: Option<&Path>,
    workspace: &Path,
) -> ToolchainResolution {
    let mut candidates: Vec<(PathBuf, &'static str)> = Vec::new();
    if let Some(path) = override_path {
        candidates.push((path.to_path_buf(), "environment"));
    } else {
        if let Some(parent) = engine.and_then(Path::parent) {
            candidates.push((parent.join(tool.file_name()), "engine-sibling"));
        }
        for directory in ["runtime/bin", "runtime", "bin"] {
            candidates.push((workspace.join(directory).join(tool.file_name()), "runtime"));
        }
        for profile in ["release", "debug"] {
            candidates.push((
                workspace
                    .join("target")
                    .join(profile)
                    .join(tool.file_name()),
                "workspace-target",
            ));
        }
    }
    let mut attempted = Vec::new();
    for (path, source) in candidates {
        if attempted.contains(&path) {
            continue;
        }
        attempted.push(path.clone());
        // Inspect only: never create files, run a compiler or make a cache here.
        if path.is_file() && std::fs::File::open(&path).is_ok() {
            return ToolchainResolution {
                path: Some(path),
                source: Some(source),
                attempted,
                error: None,
            };
        }
    }
    let searched = attempted
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let reason = if override_path.is_some() {
        "explicit override is not a readable file"
    } else {
        "no readable toolchain binary"
    };
    ToolchainResolution {
        path: None,
        source: override_path.map(|_| "environment"),
        attempted,
        error: Some(format!(
            "RX_CLI_NOT_FOUND: {}: {reason}; checked [{searched}]",
            tool.env_key()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn precedence_and_invalid_override_are_shared_and_read_only() {
        let root = std::env::temp_dir().join(format!(
            "forge-toolchain-{}-{}",
            std::process::id(),
            crate::timeutil::unix_millis()
        ));
        let engine = root.join("engine/engine-host.exe");
        let sibling = root.join("engine").join(ToolchainTool::Rurixc.file_name());
        let runtime = root
            .join("runtime/bin")
            .join(ToolchainTool::Rurixc.file_name());
        let target = root
            .join("target/debug")
            .join(ToolchainTool::Rurixc.file_name());
        for path in [&sibling, &runtime, &target] {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"fixture binary").unwrap();
        }
        let probe = |explicit: Option<&Path>| {
            resolve_from(ToolchainTool::Rurixc, explicit, Some(&engine), &root)
        };
        assert_eq!(probe(None).path, Some(sibling.clone()));
        assert_eq!(probe(Some(&target)).path, Some(target.clone()));
        let missing = root.join("missing/rurixc.exe");
        let failure = probe(Some(&missing));
        assert!(failure.path.is_none());
        assert_eq!(failure.attempted, vec![missing.clone()]);
        assert!(failure.error.unwrap().contains("explicit override"));
        assert!(!missing.parent().unwrap().exists());
        assert!(probe(Some(Path::new(""))).path.is_none());
        assert!(probe(Some(&root)).path.is_none());
        std::fs::remove_file(&sibling).unwrap();
        assert_eq!(probe(None).path, Some(runtime.clone()));
        std::fs::remove_file(&runtime).unwrap();
        assert_eq!(probe(None).path, Some(target.clone()));
        std::fs::remove_file(&target).unwrap();
        let failure = probe(None);
        assert!(failure.path.is_none());
        assert!(failure.error.unwrap().contains("RX_CLI_NOT_FOUND"));
        std::fs::remove_dir_all(root).unwrap();
    }
}
