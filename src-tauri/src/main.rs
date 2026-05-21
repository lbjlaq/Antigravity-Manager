// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    #[cfg(target_os = "linux")]
    {
        // Fix for transparent window on some Linux systems
        // See: https://github.com/spacedriveapp/spacedrive/issues/1512#issuecomment-1758550164
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }

    antigravity_tools_lib::run()
}
