fn main() {
    // 02 §7.3 第 2 条:forge.toml 选了 godot 时只警告一行(没有 [render] 的项目不打印任何东西)。
    engine_host::warn_if_forge_toml_selects_godot();
    // 02 §5.3:bin = start_core(from_args_env(RurixBackend)),accept 循环跑在主线程上。
    // 出错时打印与拆分前逐字相同的一行 stderr,并按错误里的退出码退出(参数错 2,其余 1)。
    let started = engine_host::CoreConfig::from_args_env(Box::new(engine_host::RurixBackend::new())).and_then(|cfg| {
        engine_host::start_core(engine_host::CoreConfig { accept: engine_host::AcceptMode::Inline, ..cfg })
    });
    if let Err(e) = started {
        eprintln!("{e}");
        std::process::exit(e.exit_code());
    }
}
