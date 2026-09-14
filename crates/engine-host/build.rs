//! Draft release metadata hook. Not applied to the working repository.
use std::{env, fs, path::PathBuf, process::Command};

fn clean_string(name: &str, value: String) -> String {
    let upper = value.trim().to_ascii_uppercase();
    assert!(
        !value.trim().is_empty()
            && !upper.contains("TO_BE_FILLED")
            && !upper.contains("PENDING")
            && !upper.contains("[USER")
            && !upper.contains("[MAINTAINER")
            && !matches!(upper.as_str(), "TBD" | "TODO")
            && !value
                .chars()
                .any(|c| c.is_control() || c == '"' || c == '\\'),
        "{name} must contain reviewed real metadata, without control/quote characters"
    );
    value
}

fn main() {
    for name in [
        "FORGE_RELEASE_METADATA",
        "FORGE_RELEASE_COMPANY_NAME",
        "RC_EXE",
    ] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    println!("cargo:rerun-if-changed=windows-version.rc.in");
    if env::var("FORGE_RELEASE_METADATA").as_deref() != Ok("1") {
        return;
    }
    assert_eq!(env::var("CARGO_CFG_TARGET_OS").as_deref(), Ok("windows"));
    assert!(env::var("TARGET").unwrap().ends_with("-msvc"));
    let company = clean_string(
        "FORGE_RELEASE_COMPANY_NAME",
        env::var("FORGE_RELEASE_COMPANY_NAME")
            .expect("Maintainer/company display name must be approved before a release build"),
    );
    let version = clean_string("CARGO_PKG_VERSION", env::var("CARGO_PKG_VERSION").unwrap());
    let core_version = version.split(['-', '+']).next().unwrap();
    let parts: Vec<u16> = core_version
        .split('.')
        .map(|part| {
            part.parse::<u16>()
                .expect("PE version components must fit u16")
        })
        .collect();
    assert_eq!(
        parts.len(),
        3,
        "Expected a three-component Cargo package version"
    );
    let fixed = format!("{},{},{},0", parts[0], parts[1], parts[2]);
    let file_version = format!("{}.{}.{}.0", parts[0], parts[1], parts[2]);
    let resource = include_str!("windows-version.rc.in")
        .replace("@FIXED_VERSION@", &fixed)
        .replace("@FILE_VERSION@", &file_version)
        .replace("@PRODUCT_VERSION@", &version)
        .replace("@COMPANY_NAME@", &company);
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let input = out.join("engine-host-version.rc");
    let output = out.join("engine-host-version.res");
    let bytes: Vec<u8> = std::iter::once(0xfeffu16)
        .chain(resource.encode_utf16())
        .flat_map(u16::to_le_bytes)
        .collect();
    fs::write(&input, bytes).expect("Write UTF-16 Windows version resource");
    let status = Command::new(env::var_os("RC_EXE").unwrap_or_else(|| "rc.exe".into()))
        .arg("/nologo")
        .arg("/fo")
        .arg(&output)
        .arg(&input)
        .status()
        .expect("Windows SDK rc.exe must be available in the normal MSVC build environment");
    assert!(
        status.success(),
        "Windows version resource compilation failed"
    );
    println!("cargo:rustc-link-arg-bin=engine-host={}", output.display());
}
