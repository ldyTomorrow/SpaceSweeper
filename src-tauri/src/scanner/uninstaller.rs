//! 应用卸载：发现 `.app`、读取 Bundle ID、匹配 Library 残留、移入废纸篓。
//!
//! 安全边界：
//! - 系统应用（`/System/Applications` 与 `com.apple.*`）只展示，不提供删除；
//! - 只允许删除 `/Applications` 或 `~/Applications` 下的 `.app` 包；
//! - 残留只允许来自固定的 Library 子目录，且必须匹配 Bundle ID 或应用名；
//! - 个人数据残留默认不勾选，必须显式授权；
//! - 所有删除动作在 Rust 侧二次校验，并写入审计日志。

use crate::cleaner::{self, DeleteMode, DeleteReport, RemovedEntry, SkippedEntry};
use crate::fs_util::path_size;
use crate::safety::{SafetyPolicy, SafetyTier};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, UNIX_EPOCH};
use tauri::ipc::Channel;
use walkdir::WalkDir;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledApp {
    pub id: String,
    pub path: String,
    pub name: String,
    pub bundle_id: Option<String>,
    pub version: Option<String>,
    pub size: u64,
    pub modified_ms: Option<u64>,
    pub is_system: bool,
    pub deletable: bool,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppResidual {
    pub id: String,
    pub path: String,
    pub name: String,
    pub size: u64,
    pub category: String,
    pub safety_tier: SafetyTier,
    pub default_selected: bool,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppScanSummary {
    pub roots: Vec<String>,
    pub apps_scanned: u64,
    pub total_bytes: u64,
    pub skipped_entries: u64,
    pub duration_ms: u64,
    pub cancelled: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AppScanEvent {
    Started {
        roots: Vec<String>,
    },
    Item {
        app: InstalledApp,
    },
    Progress {
        apps_scanned: u64,
        total_bytes: u64,
        skipped_entries: u64,
        elapsed_ms: u64,
    },
    Finished {
        cancelled: bool,
        apps: Vec<InstalledApp>,
        summary: AppScanSummary,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UninstallRequest {
    pub app_path: String,
    pub bundle_id: Option<String>,
    pub app_name: String,
    pub residual_paths: Vec<String>,
    pub allow_ask: bool,
}

#[derive(Debug, Default)]
struct ScanStats {
    apps_scanned: AtomicU64,
    total_bytes: AtomicU64,
    skipped_entries: AtomicU64,
}

#[derive(Debug, Clone)]
struct AppCandidate {
    path: PathBuf,
    is_system: bool,
}

const RESIDUAL_ROOTS: &[(&str, &str)] = &[
    ("Library/Caches", "缓存"),
    ("Library/Logs", "日志"),
    ("Library/HTTPStorages", "网络存储"),
    ("Library/WebKit", "WebKit 数据"),
    ("Library/Cookies", "Cookies"),
    ("Library/Application Support", "应用支持"),
    ("Library/Containers", "应用容器"),
    ("Library/Group Containers", "共享容器"),
    ("Library/Application Scripts", "应用脚本"),
    ("Library/Preferences", "偏好设置"),
    ("Library/Preferences/ByHost", "偏好设置（按主机）"),
    ("Library/Saved Application State", "窗口保存状态"),
    ("Library/LaunchAgents", "启动代理"),
];

pub fn scan_applications(
    channel: &Channel<AppScanEvent>,
    cancel: Arc<AtomicBool>,
) -> Result<AppScanSummary, String> {
    let started = Instant::now();
    let roots = app_roots()?;
    let root_strings: Vec<String> = roots
        .iter()
        .map(|root| root.to_string_lossy().into_owned())
        .collect();

    channel
        .send(AppScanEvent::Started {
            roots: root_strings.clone(),
        })
        .map_err(|error| format!("无法建立应用扫描流：{error}"))?;

    let candidates = collect_app_candidates(&roots);
    let stats = Arc::new(ScanStats::default());
    let stop_reporter = Arc::new(AtomicBool::new(false));
    let reporter = spawn_reporter(
        channel.clone(),
        stop_reporter.clone(),
        stats.clone(),
        started,
    );

    let done = AtomicUsize::new(0);
    let apps: Vec<InstalledApp> = candidates
        .par_iter()
        .filter_map(|candidate| {
            if cancel.load(Ordering::Relaxed) {
                return None;
            }
            let app = build_app(candidate, &stats);
            done.fetch_add(1, Ordering::Relaxed);
            let _ = channel.send(AppScanEvent::Item { app: app.clone() });
            Some(app)
        })
        .collect();

    stop_reporter.store(true, Ordering::Relaxed);
    let _ = reporter.join();

    let mut apps = apps;
    apps.sort_by(|left, right| {
        right
            .size
            .cmp(&left.size)
            .then_with(|| left.name.cmp(&right.name))
    });

    let cancelled = cancel.load(Ordering::Relaxed);
    let summary = AppScanSummary {
        roots: root_strings,
        apps_scanned: done.load(Ordering::Relaxed) as u64,
        total_bytes: stats.total_bytes.load(Ordering::Relaxed),
        skipped_entries: stats.skipped_entries.load(Ordering::Relaxed),
        duration_ms: started.elapsed().as_millis() as u64,
        cancelled,
    };

    channel
        .send(AppScanEvent::Finished {
            cancelled,
            apps: apps.clone(),
            summary: summary.clone(),
        })
        .map_err(|error| format!("扫描完成但无法发送应用列表：{error}"))?;

    Ok(summary)
}

pub fn find_app_residuals(bundle_id: Option<&str>, app_name: &str) -> Vec<AppResidual> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    let policy = SafetyPolicy::macos_default();
    let bundle_id = bundle_id.unwrap_or("");
    let mut residuals: Vec<AppResidual> = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for (relative, category) in RESIDUAL_ROOTS {
        let root = home.join(relative);
        let Ok(read_dir) = fs::read_dir(&root) else {
            continue;
        };

        for entry in read_dir {
            let Ok(entry) = entry else { continue };
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_symlink() {
                continue;
            }
            let path = entry.path();
            let Ok(tier) = policy.validate_app_residual(&path, bundle_id, app_name, true) else {
                continue;
            };
            let Ok(canonical) = path.canonicalize() else {
                continue;
            };
            if !seen.insert(canonical.clone()) {
                continue;
            }
            let size = path_size(&canonical);
            let name = canonical
                .file_name()
                .map(|value| value.to_string_lossy().into_owned())
                .unwrap_or_else(|| canonical.to_string_lossy().into_owned());

            residuals.push(AppResidual {
                id: canonical.to_string_lossy().into_owned(),
                path: canonical.to_string_lossy().into_owned(),
                name,
                size: size.bytes,
                category: category.to_string(),
                safety_tier: tier,
                default_selected: tier == SafetyTier::Safe,
                reason: if tier == SafetyTier::Safe {
                    "可再生缓存或日志；默认勾选，可在废纸篓恢复".to_string()
                } else {
                    "个人数据或应用设置；默认不勾选，必须显式确认".to_string()
                },
            });
        }
    }

    residuals.sort_by(|left, right| {
        let left_rank = if left.safety_tier == SafetyTier::Safe {
            0
        } else {
            1
        };
        let right_rank = if right.safety_tier == SafetyTier::Safe {
            0
        } else {
            1
        };
        left_rank
            .cmp(&right_rank)
            .then_with(|| right.size.cmp(&left.size))
            .then_with(|| left.name.cmp(&right.name))
    });
    residuals
}

pub fn uninstall_app(
    request: &UninstallRequest,
    policy: &SafetyPolicy,
) -> Result<DeleteReport, String> {
    let app_path = PathBuf::from(&request.app_path);
    let metadata = app_path
        .symlink_metadata()
        .map_err(|_| format!("应用不存在或无权访问：{}", app_path.display()))?;
    if metadata.file_type().is_symlink() {
        return Err("应用包是符号链接，已拒绝".to_string());
    }

    // 以应用包内 Info.plist 为准，防止前端伪造 Bundle ID 绕过系统应用保护。
    let info = read_app_info(&app_path);
    let Some(actual_bundle_id) = info.bundle_id.clone() else {
        return Err("无法读取应用 Bundle ID，已拒绝卸载".to_string());
    };
    if let Some(requested) = &request.bundle_id {
        if requested != &actual_bundle_id {
            return Err("Bundle ID 与应用包不一致，已拒绝".to_string());
        }
    }
    let bundle_id = Some(actual_bundle_id);
    let app_name = info
        .name
        .clone()
        .unwrap_or_else(|| request.app_name.clone());

    SafetyPolicy::validate_application(&app_path, bundle_id.as_deref())
        .map_err(|reason| format!("{}：{}", app_path.display(), reason))?;

    let known = find_app_residuals(bundle_id.as_deref(), &app_name);
    let known_map: HashMap<String, &AppResidual> = known
        .iter()
        .map(|residual| (residual.path.clone(), residual))
        .collect();

    let mut residual_paths: Vec<PathBuf> = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for raw in &request.residual_paths {
        let path = PathBuf::from(raw);
        let metadata = path
            .symlink_metadata()
            .map_err(|_| format!("残留路径不存在：{raw}"))?;
        if metadata.file_type().is_symlink() {
            return Err(format!("残留路径是符号链接，已拒绝：{raw}"));
        }
        let canonical = path
            .canonicalize()
            .map_err(|_| format!("残留路径不存在：{raw}"))?;
        let key = canonical.to_string_lossy().into_owned();
        let residual = known_map
            .get(&key)
            .ok_or_else(|| format!("残留路径不在可识别清单内：{raw}"))?;
        if residual.safety_tier == SafetyTier::Ask && !request.allow_ask {
            return Err(format!("{}：该残留属于需确认档，未获得显式授权", raw));
        }
        policy
            .validate_app_residual(
                &canonical,
                bundle_id.as_deref().unwrap_or(""),
                &app_name,
                request.allow_ask,
            )
            .map_err(|reason| format!("{raw}：{reason}"))?;
        if seen.insert(canonical.clone()) {
            residual_paths.push(canonical);
        }
    }

    let mut report = DeleteReport {
        mode: DeleteMode::Trash,
        ..DeleteReport::default()
    };

    trash_one(&app_path, "UNINSTALL_APP", &mut report);
    for path in residual_paths {
        trash_one(&path, "UNINSTALL_RESIDUAL", &mut report);
    }

    Ok(report)
}

fn trash_one(path: &Path, action: &str, report: &mut DeleteReport) {
    let size = path_size(path).bytes;
    match trash::delete(path) {
        Ok(()) => {
            cleaner::audit(action, path);
            report.freed_bytes = report.freed_bytes.saturating_add(size);
            report.removed.push(RemovedEntry {
                path: path.to_string_lossy().into_owned(),
                size,
            });
        }
        Err(error) => {
            cleaner::audit(&format!("{action}_FAIL"), path);
            report.skipped.push(SkippedEntry {
                path: path.to_string_lossy().into_owned(),
                reason: error.to_string(),
            });
        }
    }
}

fn app_roots() -> Result<Vec<PathBuf>, String> {
    let home = dirs::home_dir().ok_or_else(|| "无法定位当前用户主目录".to_string())?;
    let candidates = [
        PathBuf::from("/Applications"),
        home.join("Applications"),
        PathBuf::from("/System/Applications"),
    ];
    let roots: Vec<PathBuf> = candidates
        .into_iter()
        .filter(|path| path.is_dir())
        .filter_map(|path| path.canonicalize().ok())
        .collect();

    if roots.is_empty() {
        Err("没有找到应用目录".to_string())
    } else {
        Ok(roots)
    }
}

fn collect_app_candidates(roots: &[PathBuf]) -> Vec<AppCandidate> {
    let mut candidates = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for root in roots {
        let is_system_root = root.starts_with("/System/Applications");
        for entry in WalkDir::new(root)
            .max_depth(2)
            .follow_links(false)
            .into_iter()
            .filter_map(Result::ok)
        {
            if entry.depth() == 0 {
                continue;
            }
            let file_type = entry.file_type();
            if file_type.is_symlink() || !file_type.is_dir() {
                continue;
            }
            let path = entry.path();
            if !is_app_bundle(path) {
                continue;
            }
            let Ok(canonical) = path.canonicalize() else {
                continue;
            };
            if seen.insert(canonical.clone()) {
                candidates.push(AppCandidate {
                    path: canonical,
                    is_system: is_system_root,
                });
            }
        }
    }

    candidates
}

fn build_app(candidate: &AppCandidate, stats: &ScanStats) -> InstalledApp {
    let info = read_app_info(&candidate.path);
    let size = path_size(&candidate.path);
    stats.apps_scanned.fetch_add(1, Ordering::Relaxed);
    stats.total_bytes.fetch_add(size.bytes, Ordering::Relaxed);
    stats
        .skipped_entries
        .fetch_add(size.skipped, Ordering::Relaxed);

    let bundle_id = info.bundle_id;
    let is_system = candidate.is_system
        || bundle_id
            .as_deref()
            .map(|id| id.to_ascii_lowercase().starts_with("com.apple."))
            .unwrap_or(false);

    let validation = SafetyPolicy::validate_application(&candidate.path, bundle_id.as_deref());
    let has_bundle_id = bundle_id.is_some();
    let deletable = validation.is_ok() && !is_system && has_bundle_id;
    let reason = if is_system {
        "系统应用或 Apple 应用，只展示，不提供卸载入口".to_string()
    } else if !has_bundle_id {
        "无法读取 Bundle ID，不能安全匹配残留，已禁止卸载".to_string()
    } else if let Err(reason) = validation {
        reason.to_string()
    } else {
        "用户安装的应用；卸载会将应用与勾选的残留一起移入废纸篓".to_string()
    };

    let name = info.name.unwrap_or_else(|| {
        candidate
            .path
            .file_stem()
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_else(|| candidate.path.to_string_lossy().into_owned())
    });

    InstalledApp {
        id: candidate.path.to_string_lossy().into_owned(),
        path: candidate.path.to_string_lossy().into_owned(),
        name,
        bundle_id,
        version: info.version,
        size: size.bytes,
        modified_ms: fs::symlink_metadata(&candidate.path)
            .ok()
            .and_then(|metadata| metadata.modified().ok())
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis() as u64),
        is_system,
        deletable,
        reason,
    }
}

struct AppInfo {
    name: Option<String>,
    bundle_id: Option<String>,
    version: Option<String>,
}

fn read_app_info(app_path: &Path) -> AppInfo {
    let plist = app_path.join("Contents/Info.plist");
    let mut info = AppInfo {
        name: None,
        bundle_id: None,
        version: None,
    };

    let output = Command::new("plutil")
        .args(["-convert", "json", "-o", "-"])
        .arg(&plist)
        .output();
    let Ok(output) = output else {
        return info;
    };
    if !output.status.success() {
        return info;
    }
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&output.stdout) else {
        return info;
    };

    info.name = value
        .get("CFBundleDisplayName")
        .or_else(|| value.get("CFBundleName"))
        .and_then(|value| value.as_str())
        .map(|value| value.to_string());
    info.bundle_id = value
        .get("CFBundleIdentifier")
        .and_then(|value| value.as_str())
        .map(|value| value.to_string());
    info.version = value
        .get("CFBundleShortVersionString")
        .or_else(|| value.get("CFBundleVersion"))
        .and_then(|value| value.as_str())
        .map(|value| value.to_string());
    info
}

fn is_app_bundle(path: &Path) -> bool {
    path.extension()
        .map(|ext| ext.to_string_lossy().eq_ignore_ascii_case("app"))
        .unwrap_or(false)
}

fn spawn_reporter(
    channel: Channel<AppScanEvent>,
    stop: Arc<AtomicBool>,
    stats: Arc<ScanStats>,
    started: Instant,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(150));
            if stop.load(Ordering::Relaxed) {
                break;
            }
            let _ = channel.send(AppScanEvent::Progress {
                apps_scanned: stats.apps_scanned.load(Ordering::Relaxed),
                total_bytes: stats.total_bytes.load(Ordering::Relaxed),
                skipped_entries: stats.skipped_entries.load(Ordering::Relaxed),
                elapsed_ms: started.elapsed().as_millis() as u64,
            });
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_bundle_metadata_from_info_plist() {
        let temp = tempfile::TempDir::new().unwrap();
        let app = temp.path().join("Foo.app");
        fs::create_dir_all(app.join("Contents")).unwrap();
        fs::write(
            app.join("Contents/Info.plist"),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleIdentifier</key><string>com.example.foo</string>
  <key>CFBundleName</key><string>Foo</string>
  <key>CFBundleShortVersionString</key><string>1.2.3</string>
</dict>
</plist>"#,
        )
        .unwrap();

        let info = read_app_info(&app);
        assert_eq!(info.bundle_id.as_deref(), Some("com.example.foo"));
        assert_eq!(info.name.as_deref(), Some("Foo"));
        assert_eq!(info.version.as_deref(), Some("1.2.3"));
    }

    #[test]
    fn system_roots_are_recognized() {
        let roots = vec![PathBuf::from("/System/Applications")];
        let candidates = collect_app_candidates(&roots);
        assert!(candidates.iter().all(|candidate| candidate.is_system));
    }

    #[test]
    fn app_scan_events_serialize_with_camel_case_fields() {
        let event = AppScanEvent::Progress {
            apps_scanned: 3,
            total_bytes: 4096,
            skipped_entries: 1,
            elapsed_ms: 220,
        };
        let value = serde_json::to_value(event).unwrap();
        assert_eq!(value["type"], "progress");
        assert_eq!(value["appsScanned"].as_u64(), Some(3));
        assert_eq!(value["totalBytes"].as_u64(), Some(4096));
    }
}
