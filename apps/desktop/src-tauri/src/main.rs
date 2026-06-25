use std::path::PathBuf;

use ai_handoff_core::dashboard::{
    self, CapsuleList, DashboardSnapshot, LogFile, ReadTextResult,
};

const TEXT_LIMIT: u64 = 512 * 1024;

#[tauri::command]
fn get_dashboard_snapshot() -> Result<DashboardSnapshot, String> {
    Ok(dashboard::dashboard_snapshot())
}

#[tauri::command]
fn list_capsules() -> Result<CapsuleList, String> {
    Ok(dashboard::list_capsules())
}

#[tauri::command]
fn read_capsule(path: String) -> Result<ReadTextResult, String> {
    Ok(dashboard::read_capsule(&PathBuf::from(path), TEXT_LIMIT))
}

#[tauri::command]
fn read_logs() -> Result<Vec<LogFile>, String> {
    Ok(dashboard::read_logs(TEXT_LIMIT))
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            get_dashboard_snapshot,
            list_capsules,
            read_capsule,
            read_logs
        ])
        .run(tauri::generate_context!())
        .expect("error while running AI Handoff desktop app");
}
