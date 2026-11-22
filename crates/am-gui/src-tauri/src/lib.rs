#[tauri::command]
fn get_stats_report() -> Result<am_core::stats::MasterAnalyticsReport, String> {
    let storage = am_core::storage::StorageManager::new().map_err(|e| e.to_string())?;
    am_core::stats::StatsEngine::generate_master_report(&storage).map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![get_stats_report])
        .run(tauri::generate_context!())
        .expect("error while running antigravity manager application");
}
