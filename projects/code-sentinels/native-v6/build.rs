//! Fingerprint the native rules at build time; rendering/UI assets are outside this identity.
use sha2::{Digest, Sha256};
use std::{
    env, fs,
    path::{Path, PathBuf},
};
fn collect(dir: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("native rules source directory") {
        let path = entry.expect("source entry").path();
        if path.is_dir() {
            collect(&path, files);
        } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
            files.push(path);
        }
    }
}
fn main() {
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest directory"));
    let mut files = Vec::new();
    collect(&root.join("src"), &mut files);
    for name in ["Cargo.toml", "Cargo.lock", "build.rs"] {
        let file = root.join(name);
        if file.is_file() {
            files.push(file);
        }
    }
    files.sort_by_key(|p| {
        p.strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/")
    });
    let mut digest = Sha256::new();
    digest.update(b"code-sentinels-native-rules-v1\0");
    println!("cargo:rerun-if-changed=src");
    for file in files {
        let name = file
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let bytes = fs::read(&file).expect("read native rules source");
        println!("cargo:rerun-if-changed={name}");
        digest.update((name.len() as u64).to_le_bytes());
        digest.update(name.as_bytes());
        digest.update((bytes.len() as u64).to_le_bytes());
        digest.update(bytes);
    }
    println!(
        "cargo:rustc-env=SENTINELS_V6_RULES_FINGERPRINT={:x}",
        digest.finalize()
    );
}
