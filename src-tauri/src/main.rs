#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod cleaner;
mod fs_util;
mod permissions;
mod safety;
mod scan_manager;
mod scanner;

use cleaner::{DeleteReport, PermanentDeleteSession, PermanentTicket};
use safety::SafetyPolicy;
use scan_manager::ScanManager;
use scanner::analyze::{SpaceAnalysis, SpaceScanEvent, SpaceScanOptions};
use scanner::cache::{ScanEvent, ScanSummary};
use scanner::duplicates::{
    DuplicateDeleteGroup, DuplicateScanEvent, DuplicateScanOptions, DuplicateScanSummary,
};
use scanner::large_files::{LargeScanEvent, LargeScanOptions, LargeScanSummary};
use scanner::uninstaller::{AppResidual, AppScanEvent, AppScanSummary, UninstallRequest};
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};
use tauri::ipc::Channel;
use tauri::State;

struct AppState {
    permanent: Arc<PermanentDeleteSession>,
    scans: Arc<ScanManager>,
    policy: Mutex<SafetyPolicy>,
}

fn to_paths(paths: Vec<String>) -> Vec<PathBuf> {
    paths.into_iter().map(PathBuf::from).collect()
}

fn policy_snapshot(state: &State<'_, AppState>) -> SafetyPolicy {
    state
        .policy
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

/// 流式扫描：命令等待扫描结束，但结果通过 Channel 逐条送达前端。
#[tauri::command]
async fn scan_library_caches(
    groups: Option<Vec<String>>,
    on_event: Channel<ScanEvent>,
) -> Result<ScanSummary, String> {
    tauri::async_runtime::spawn_blocking(move || {
        scanner::cache::scan_library_caches(&on_event, groups)
    })
    .await
    .map_err(|error| format!("扫描线程异常：{error}"))?
}

/// 需确认区域扫描：只列出 ask 根目录的子项，根目录本身永不进入结果。
#[tauri::command]
async fn scan_attention_areas(
    on_event: Channel<ScanEvent>,
    state: State<'_, AppState>,
) -> Result<ScanSummary, String> {
    let policy = policy_snapshot(&state);
    tauri::async_runtime::spawn_blocking(move || {
        scanner::cache::scan_attention_areas(&on_event, &policy)
    })
    .await
    .map_err(|error| format!("扫描线程异常：{error}"))?
}

/// 前端用于拼接固定目录路径；不暴露任何额外能力。
#[tauri::command]
fn home_directory() -> Result<String, String> {
    dirs::home_dir()
        .map(|home| home.to_string_lossy().into_owned())
        .ok_or_else(|| "无法定位当前用户主目录".to_string())
}

/// 打开原生目录选择器；取消时返回 None。
#[tauri::command]
fn choose_directory() -> Result<Option<String>, String> {
    let output = Command::new("osascript")
        .args([
            "-e",
            "try",
            "-e",
            "POSIX path of (choose folder with prompt \"选择要分析或清理的目录\")",
            "-e",
            "on error number -128",
            "-e",
            "return \"\"",
            "-e",
            "end try",
        ])
        .output()
        .map_err(|error| format!("无法打开目录选择器：{error}"))?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }

    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() {
        Ok(None)
    } else {
        Ok(Some(path))
    }
}

/// 将用户选择的目录加入当前会话的删除白名单；不写入磁盘，重启后失效。
#[tauri::command]
fn authorize_scan_root(path: String, state: State<'_, AppState>) -> Result<String, String> {
    let canonical = safety::authorize_session_root(&PathBuf::from(path))?;
    let mut policy = state
        .policy
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let resolved = policy.add_safe_root(canonical)?;
    Ok(resolved.to_string_lossy().into_owned())
}

#[tauri::command]
fn list_authorized_roots(state: State<'_, AppState>) -> Vec<String> {
    policy_snapshot(&state)
        .safe_roots()
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect()
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

/// 空间分析：只读扫描，目录节点不可删除，文件节点是否可删由白名单决定。
#[tauri::command]
async fn analyze_space(
    scan_id: String,
    options: SpaceScanOptions,
    on_event: Channel<SpaceScanEvent>,
    state: State<'_, AppState>,
) -> Result<SpaceAnalysis, String> {
    let manager = state.scans.clone();
    let cancel = manager.begin(&scan_id)?;
    let policy = policy_snapshot(&state);

    let joined = tauri::async_runtime::spawn_blocking(move || {
        scanner::analyze::analyze_space(&on_event, cancel, &options, &policy)
    })
    .await;

    manager.finish(&scan_id);
    joined.map_err(|error| format!("分析线程异常：{error}"))?
}

/// 重复文件扫描：大小分组 → 4 KB 哈希 → 完整 BLAKE3。
#[tauri::command]
async fn scan_duplicates(
    scan_id: String,
    options: DuplicateScanOptions,
    on_event: Channel<DuplicateScanEvent>,
    state: State<'_, AppState>,
) -> Result<DuplicateScanSummary, String> {
    let manager = state.scans.clone();
    let cancel = manager.begin(&scan_id)?;

    let joined = tauri::async_runtime::spawn_blocking(move || {
        scanner::duplicates::scan_duplicates(&on_event, cancel, &options)
    })
    .await;

    manager.finish(&scan_id);
    joined.map_err(|error| format!("扫描线程异常：{error}"))?
}

/// 重复文件删除：后端复核保留项与哈希后才执行。
#[tauri::command]
async fn delete_duplicates(
    groups: Vec<DuplicateDeleteGroup>,
    allow_ask: bool,
    state: State<'_, AppState>,
) -> Result<DeleteReport, String> {
    let policy = policy_snapshot(&state);
    tauri::async_runtime::spawn_blocking(move || {
        let cancel = std::sync::atomic::AtomicBool::new(false);
        scanner::duplicates::delete_duplicates(&groups, &policy, allow_ask, &cancel)
    })
    .await
    .map_err(|error| format!("删除线程异常：{error}"))?
}

/// 应用列表扫描：流式推送每个应用，结束时返回按大小排序的完整列表。
#[tauri::command]
async fn scan_applications(
    scan_id: String,
    on_event: Channel<AppScanEvent>,
    state: State<'_, AppState>,
) -> Result<AppScanSummary, String> {
    let manager = state.scans.clone();
    let cancel = manager.begin(&scan_id)?;

    let joined = tauri::async_runtime::spawn_blocking(move || {
        scanner::uninstaller::scan_applications(&on_event, cancel)
    })
    .await;

    manager.finish(&scan_id);
    joined.map_err(|error| format!("扫描线程异常：{error}"))?
}

/// 读取选中应用的 Library 残留；只返回可识别、可校验的路径。
#[tauri::command]
async fn find_app_residuals(
    bundle_id: Option<String>,
    app_name: String,
) -> Result<Vec<AppResidual>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        Ok(scanner::uninstaller::find_app_residuals(
            bundle_id.as_deref(),
            &app_name,
        ))
    })
    .await
    .map_err(|error| format!("读取残留线程异常：{error}"))?
}

/// 卸载应用：应用包与勾选残留一起移入废纸篓，全部路径在 Rust 侧二次校验。
#[tauri::command]
async fn uninstall_app(
    request: UninstallRequest,
    state: State<'_, AppState>,
) -> Result<DeleteReport, String> {
    let policy = policy_snapshot(&state);
    tauri::async_runtime::spawn_blocking(move || {
        scanner::uninstaller::uninstall_app(&request, &policy)
    })
    .await
    .map_err(|error| format!("卸载线程异常：{error}"))?
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
async fn move_items_to_trash(
    paths: Vec<String>,
    allow_ask: bool,
    state: State<'_, AppState>,
) -> Result<DeleteReport, String> {
    let paths = to_paths(paths);
    let policy = policy_snapshot(&state);
    let report = tauri::async_runtime::spawn_blocking(move || {
        cleaner::move_to_trash_with_policy(&paths, allow_ask, &policy)
    })
    .await
    .map_err(|error| format!("删除线程异常：{error}"))?;
    Ok(report)
}

/// 永久删除第一步：后端签发与路径清单绑定的一次性令牌。
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
    let policy = policy_snapshot(&state);
    tauri::async_runtime::spawn_blocking(move || {
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
            policy: Mutex::new(SafetyPolicy::macos_default()),
        })
        .invoke_handler(tauri::generate_handler![
            scan_library_caches,
            scan_attention_areas,
            home_directory,
            choose_directory,
            authorize_scan_root,
            list_authorized_roots,
            scan_large_files,
            cancel_scan,
            analyze_space,
            scan_duplicates,
            delete_duplicates,
            scan_applications,
            find_app_residuals,
            uninstall_app,
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
