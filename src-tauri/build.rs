fn main() {
    // no-GUI 构建（--no-default-features）不编译 GUI 插件（dialog/updater 等），
    // capabilities 目录里引用的 dialog:default 等权限随之不存在，tauri-build 校验会失败。
    // 无 GUI 即无 IPC 权限面，改指向专用空目录（见 docs/WEBUI_NO_WEBVIEW2_PLAN.md 阶段 3）。
    #[cfg(not(feature = "gui"))]
    {
        let dir = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap())
            .join("capabilities-no-gui");
        std::fs::create_dir_all(&dir).expect("failed to create capabilities-no-gui dir");
        tauri_build::try_build(
            tauri_build::Attributes::new().capabilities_path_pattern("capabilities-no-gui/*.json"),
        )
        .expect("failed to run tauri-build");
    }

    #[cfg(feature = "gui")]
    tauri_build::build();
}
