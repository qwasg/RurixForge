//! 路径 containment:拒绝 `..`、UNC、盘符逃逸,canonicalize 后必须落在授权根内。
//! junction/symlink 经 canonicalize 跟随,逃出根即拒。

use std::path::{Component, Path, PathBuf};

/// 用户路径是否明显越界(空、UNC、含 `..`)。
pub fn looks_escaped(user: &str) -> bool {
    let t = user.trim();
    if t.is_empty() {
        return true;
    }
    if t.starts_with("\\\\") || t.starts_with("//") {
        return true;
    }
    Path::new(t)
        .components()
        .any(|c| matches!(c, Component::ParentDir))
}

/// `child` 是否在 `root` 内(双方 canonicalize;不存在则沿父目录上溯到已有祖先)。
pub fn is_inside(root: &Path, child: &Path) -> bool {
    let Ok(root) = root.canonicalize() else {
        return false;
    };
    let mut cur = child.to_path_buf();
    loop {
        if cur.exists() {
            let check = cur.canonicalize().unwrap_or_else(|_| cur);
            return check.starts_with(&root);
        }
        match cur.parent() {
            Some(p) if p != cur => cur = p.to_path_buf(),
            _ => return child.starts_with(&root),
        }
    }
}

/// 把用户路径解析到授权根之一内。相对路径依次拼到各根;绝对路径必须本身落在某根内。
pub fn confine_under(roots: &[&Path], user: &str) -> Result<PathBuf, String> {
    if looks_escaped(user) {
        return Err("PATH_OUTSIDE_ROOT".into());
    }
    let p = Path::new(user.trim());
    let candidates: Vec<PathBuf> = if p.is_absolute() {
        vec![p.to_path_buf()]
    } else {
        roots.iter().map(|r| r.join(p)).collect()
    };
    for cand in candidates {
        if roots.iter().any(|r| is_inside(r, &cand)) {
            return Ok(if cand.exists() {
                cand.canonicalize().unwrap_or(cand)
            } else {
                cand
            });
        }
    }
    Err("PATH_OUTSIDE_ROOT".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_dotdot_unc_and_empty() {
        assert!(looks_escaped(""));
        assert!(looks_escaped("../secret"));
        assert!(looks_escaped("foo/../../etc/passwd"));
        assert!(looks_escaped("\\\\server\\share"));
        assert!(looks_escaped("//server/share"));
        assert!(!looks_escaped("Content/Scenes/Main.rxscene"));
    }

    #[test]
    fn confine_keeps_relative_inside_root() {
        let dir = std::env::temp_dir().join(format!(
            "forge-pathutil-{}-{}",
            std::process::id(),
            crate::timeutil::unix_millis()
        ));
        std::fs::create_dir_all(dir.join("Content")).unwrap();
        std::fs::write(dir.join("Content").join("a.txt"), "x").unwrap();
        let hit = confine_under(&[&dir], "Content/a.txt").unwrap();
        assert!(hit.ends_with("a.txt"));
        assert!(confine_under(&[&dir], "../a.txt").is_err());
        assert!(confine_under(&[&dir], "C:/Windows/notepad.exe").is_err());
        assert!(confine_under(&[&dir], "Content/Missing/new.txt").is_ok());
        std::fs::remove_dir_all(&dir).ok();
    }
}
