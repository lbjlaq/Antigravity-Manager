fn main() {
    #[cfg(feature = "tauri-app")]
    tauri_build::build();
}
