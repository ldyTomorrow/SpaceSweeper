#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod cleaner;
mod fs_util;
mod permissions;
mod safety;
mod scan_manager;
mod scanner;

use cleaner::{DeleteReport, PermanentDeleteSession, PermanentTicket};
use scan_manager::ScanManager;
use scanner::cache::{ScanEvent, ScanSummary};
use scanner::large_files::{LargeScanEvent, LargeScanOptions, LargeScanSummary};
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use tauri::ipc::Channel;
use tauri::State;

struct AppState {
    permanent: Arc<PermanentDeleteSession>,
    scans: Arc<ScanManager>,
}

fn to_paths(paths: Vec<String>) -> Vec<PathBuf> {
    paths.into_iter().map(PathBuf::from).collect()
}

/// 流式扫描：命令等待扫描结束，但结果通过 Channel 逐条送达前端。
#[tauri::command]
async fn scan_library_caches(on_event: Channel<ScanEvent>) -> Result<ScanSummary, String> {
    tauri::async_runtime::spawn_blocking(move || scanner::cache::scan_library_caches(&on_event))
        .await
        .map_err(|error| format!("扫描线程异常：{error}"))?
}

/// B 类大文件扫描：前端生成 scan_id，后端注册取消令牌并通过 Channel 推送进度。
#[tauri::command]
async fn scan_large_files(
    scan_id: String,
    options: LargeScanOptions,
    on_event: Channel<LargeScanEvent>,
    state: State<'_, AppState>,
) -> Result<LargeScanSummary, String> {
    let manager = state.scans.clone();
    let cancel = manager.begin(&scan_id)?;

    let joined = tauri::async_runtime::spawn_blocking(move || {
        scanner::large_files::scan_large_files(&on_event, cancel, &options)
    })
    .await;

    manager.finish(&scan_id);
    joined.map_err(|error| format!("扫描线程异常：{error}"))?
}

#[tauri::command]
fn cancel_scan(scan_id: String, state: State<'_, AppState>) -> bool {
    state.scans.cancel(&scan_id)
}

#[tauri::command]
fn reveal_in_finder(path: String) -> Result<(), String> {
    let target = PathBuf::from(path);
    if !target.is_absolute() {
        return Err("路径不是绝对路径".to_string());
    }
    if target.symlink_metadata().is_err() {
        return Err("目标不存在或无权访问".to_string());
    }

    let status = Command::new("open")
        .arg("-R")
        .arg(&target)
        .status()
        .map_err(|error| format!("无法打开 Finder：{error}"))?;

    if status.success() {
        Ok(())
    } else {
        Err("Finder 无法定位该路径".to_string())
    }
}

#[tauri::command]
async fn move_items_to_trash(paths: Vec<String>, allow_ask: bool) -> Result<DeleteReport, String> {
    let paths = to_paths(paths);
    let report =
        tauri::async_runtime::spawn_blocking(move || cleaner::move_to_trash(&paths, allow_ask))
            .await
            .map_err(|error| format!("删除线程异常：{error}"))?;
    Ok(report)
}

/// 永久删除第一步：后端签发与路径清单绑定的一次性令牌。
/// 当前版本的 UI 只开放废纸篓，此命令为后续"永久删除"入口预留。
#[tauri::command]
fn prepare_permanent_delete(paths: Vec<String>, state: State<'_, AppState>) -> PermanentTicket {
    state.permanent.issue(to_paths(paths))
}

/// 永久删除第二步：必须携带有效令牌，且路径清单必须与签发时完全一致。
#[tauri::command]
async fn permanent_delete(
    paths: Vec<String>,
    token: String,
    allow_ask: bool,
    state: State<'_, AppState>,
) -> Result<DeleteReport, String> {
    let paths = to_paths(paths);
    let session = state.permanent.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let policy = safety::SafetyPolicy::macos_default();
        session.execute(&policy, &token, &paths, allow_ask)
    })
    .await
    .map_err(|error| format!("删除线程异常：{error}"))?
}

#[tauri::command]
fn check_full_disk_access() -> bool {
    permissions::has_full_disk_access()
}

#[tauri::command]
fn open_full_disk_access_settings() -> Result<(), String> {
    Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles")
        .status()
        .map_err(|error| format!("无法打开系统设置：{error}"))?;
    Ok(())
}

fn main() {
    tauri::Builder::default()
        .manage(AppState {
            permanent: Arc::new(PermanentDeleteSession::default()),
            scans: Arc::new(ScanManager::default()),
        })
        .invoke_handler(tauri::generate_handler![
            scan_library_caches,
            scan_large_files,
            cancel_scan,
            reveal_in_finder,
            move_items_to_trash,
            prepare_permanent_delete,
            permanent_delete,
            check_full_disk_access,
            open_full_disk_access_settings
        ])
        .run(tauri::generate_context!())
        .expect("SpaceSweeper 启动失败");
}
