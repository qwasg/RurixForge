// 给 forge-setup.exe 嵌入图标、清单(按调用者权限运行、Per-Monitor V2 DPI、通用控件 6)与版本信息。
fn main() {
    let ui_files = ["index.html", "setup.css", "setup.js", "particles.js"];
    for name in ui_files {
        println!("cargo:rerun-if-changed=ui/{name}");
    }
    println!("cargo:rerun-if-changed=resources/setup.rc");
    println!("cargo:rerun-if-changed=resources/setup.manifest");
    println!("cargo:rerun-if-changed=../../../packages/client/public/favicon.ico");
    // 出包脚本传入产品版本(根 package.json),资源管理器「详细信息」里显示的是 RurixForge 的版本。
    println!("cargo:rerun-if-env-changed=FORGE_SETUP_VERSION");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let version = std::env::var("FORGE_SETUP_VERSION")
            .ok()
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| std::env::var("CARGO_PKG_VERSION").unwrap());
        let mut parts: Vec<u16> = version.split('.').map(|p| p.parse().unwrap_or(0)).collect();
        parts.resize(4, 0);
        let numeric = parts.iter().map(u16::to_string).collect::<Vec<_>>().join(",");
        let macros = [format!("FORGE_VERSION_NUM={numeric}"), format!("FORGE_VERSION_STR=\"{version}\"")];
        embed_resource::compile("resources/setup.rc", &macros)
            .manifest_required()
            .expect("嵌入 Windows 资源失败");
    }
}
