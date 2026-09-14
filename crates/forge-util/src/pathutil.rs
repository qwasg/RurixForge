//! 路径 containment:拒绝 `..`、UNC、盘符逃逸,canonicalize 后必须落在授权根内。
//! junction/symlink 经 canonicalize 跟随,逃出根即拒。

use std::path::{Component, Path, PathBuf};

/// 用户路径是否明显越界(空、UNC、含 `..`)。
/// 注意:Windows `fs::canonicalize` 产物带 `\\?\` verbatim 前缀,不是 UNC——必须先剥掉
/// 再做 UNC 判定,否则上层把 canonicalize 结果回传时会被误判逃逸(实测 scene_load
/// 已存在文件必挂:confine_under 回吐 verbatim 路径 → 这里当 UNC 拒掉)。
pub fn looks_escaped(user: &str) -> bool {
    let t = strip_verbatim_prefix(user.trim());
    if t.is_empty() {
        return true;
    }
    if t.starts_with("\\\\") || t.starts_with("//") {
        return true;
    }
    Path::new(t.as_str())
        .components()
        .any(|c| matches!(c, Component::ParentDir))
}

/// 剥掉 Windows verbatim 前缀(`\\?\C:\...` → `C:\...`;`\\?\UNC\srv\share` → `\\srv\share`)。
/// 非 verbatim 输入原样返回。
pub fn strip_verbatim_prefix(p: &str) -> String {
    if let Some(rest) = p.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{rest}");
    }
    if let Some(rest) = p.strip_prefix(r"\\?\") {
        return rest.to_string();
    }
    p.to_string()
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
/// 返回值保证不带 `\\?\` verbatim 前缀(canonicalize 产物一律还原成常规形式,防止
/// 下游把 verbatim 当普通字符串再判定时误伤)。
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
                let canon = cand.canonicalize().unwrap_or_else(|_| cand.clone());
                PathBuf::from(strip_verbatim_prefix(&canon.to_string_lossy()))
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

    /// 回归(2026-08-29):canonicalize 产物的 `\\?\` verbatim 前缀不是 UNC,不得判逃逸;
    /// confine_under 对已存在文件的返回值也不得带 verbatim 前缀(scene_load 实测回归链)。
    #[test]
    fn verbatim_prefix_is_not_unc_escape() {
        assert!(!looks_escaped(r"\\?\D:\ws\projects\demo\Content\Scenes\a.rxscene"));
        assert_eq!(
            strip_verbatim_prefix(r"\\?\D:\ws\a.rxscene"),
            r"D:\ws\a.rxscene"
        );
        assert_eq!(
            strip_verbatim_prefix(r"\\?\UNC\srv\share\a.rxscene"),
            r"\\srv\share\a.rxscene"
        );
        assert_eq!(strip_verbatim_prefix(r"D:\ws\a.rxscene"), r"D:\ws\a.rxscene");
        let tmp = std::env::temp_dir().join("forge-util-verbatim-test");
        std::fs::create_dir_all(&tmp).unwrap();
        let f = tmp.join("exists.txt");
        std::fs::write(&f, b"x").unwrap();
        let out = confine_under(&[tmp.as_path()], "exists.txt").unwrap();
        assert!(!out.to_string_lossy().starts_with(r"\\?\"), "{out:?}");
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
