//! A 类：缓存与可再生产垃圾的流式扫描器。
//!
//! 设计取舍：
//! - 以每个白名单根目录的**一级子项**作为结果粒度（每个应用/工具/项目一个条目），
//!   避免把几十万个缓存文件直接推给前端；条目大小递归汇总。
//! - 并行计算条目大小，算完一个就通过 Tauri Channel 推一个，前端不必等待全量扫描。
//! - 扫描器只展示白名单内的 safe 档路径；符号链接、`.git`、越界路径在进入列表前就被过滤。
//!
//! 需确认区域（ask 档）走独立入口：默认不勾选，只有用户显式确认后才允许删除。
//!
//! 明确不在发现范围：
//! - `~/Library/Containers`、`~/Library/Application Support`：可能包含聊天记录、虚拟机镜像等个人数据；
//! - `~/Library/Preferences`：应用设置，不属于缓存；
//! - `/System`、`/usr`、`/bin`：系统文件，只读且不可回收。
//! 这些目录即使被误传入 cleaner，也会被 safety.rs 拒绝。

use crate::fs_util::path_size;
use crate::safety::{SafetyPolicy, SafetyTier};
use rayon::prelude::*;
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Instant, UNIX_EPOCH};
use tauri::ipc::Channel;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheItem {
    pub id: String,
    pub path: String,
    pub name: String,
    pub size: u64,
    pub file_count: u64,
    pub category: String,
    pub safety_tier: SafetyTier,
    pub is_dir: bool,
    pub last_modified: Option<u64>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ScanEvent {
    Started {
        root: String,
        total_entries: usize,
    },
    Item {
        item: CacheItem,
    },
    Progress {
        items_done: usize,
        items_total: usize,
        file_count: u64,
        total_bytes: u64,
    },
    Finished {
        item_count: usize,
        file_count: u64,
        total_bytes: u64,
        skipped_entries: u64,
        denied_entries: u64,
        duration_ms: u64,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanSummary {
    pub root: String,
    pub item_count: usize,
    pub file_count: u64,
    pub total_bytes: u64,
    pub skipped_entries: u64,
    pub denied_entries: u64,
    pub duration_ms: u64,
}

#[derive(Debug, Clone)]
struct CacheSource {
    path: PathBuf,
    category: &'static str,
    reason: &'static str,
}

#[derive(Debug, Clone)]
struct Candidate {
    path: PathBuf,
    category: &'static str,
    reason: &'static str,
}

/// 兼容旧命令名：默认只扫描 `~/Library/Caches`，可通过 `groups` 扩展。
pub fn scan_library_caches(
    channel: &Channel<ScanEvent>,
    groups: Option<Vec<String>>,
) -> Result<ScanSummary, String> {
    let home = dirs::home_dir().ok_or_else(|| "无法定位当前用户主目录".to_string())?;
    let (sources, labels) = resolve_cache_sources(&home, groups.as_deref())?;
    let root_label = if labels.len() == 1 {
        sources
            .first()
            .map(|source| source.path.to_string_lossy().into_owned())
            .unwrap_or_else(|| labels[0].to_string())
    } else {
        format!("{} 个缓存分组", labels.len())
    };

    let mut candidates = Vec::new();
    let mut skipped_entries = 0;
    for source in &sources {
        skipped_entries += collect_cache_children(&source.path, source, &mut candidates);
    }

    if candidates.is_empty() {
        return Err("所选缓存目录不存在或暂时无法读取".to_string());
    }

    let policy = SafetyPolicy::macos_default();
    scan_candidates(
        channel,
        &root_label,
        candidates,
        &policy,
        SafetyTier::Safe,
        skipped_entries,
    )
}

/// 扫描 ask 档区域的一级子项。根目录本身不会进入结果，因而永远无法整根删除。
pub fn scan_attention_areas(
    channel: &Channel<ScanEvent>,
    policy: &SafetyPolicy,
) -> Result<ScanSummary, String> {
    let home = dirs::home_dir().ok_or_else(|| "无法定位当前用户主目录".to_string())?;
    let mut candidates = Vec::new();
    let mut skipped_entries = 0;

    for root in policy.ask_roots() {
        if !root.is_dir() {
            continue;
        }
        let note = attention_note(root, &home);
        skipped_entries += collect_attention_children(root, note, &mut candidates);
    }

    if candidates.is_empty() {
        return Err("需确认区域不存在或暂时无法读取".to_string());
    }

    scan_candidates(
        channel,
        "需确认区域",
        candidates,
        policy,
        SafetyTier::Ask,
        skipped_entries,
    )
}

fn collect_cache_children(root: &Path, source: &CacheSource, output: &mut Vec<Candidate>) -> u64 {
    let read_dir = match fs::read_dir(root) {
        Ok(read_dir) => read_dir,
        Err(_) => return 1,
    };

    let mut skipped = 0;
    for entry in read_dir {
        match entry {
            Ok(entry) => match entry.file_type() {
                Ok(file_type) if file_type.is_symlink() => skipped += 1,
                Ok(_) => {
                    // 不清理本应用自己的审计日志，保留删除可追溯性。
                    if entry.file_name().to_string_lossy() == "SpaceSweeper" {
                        skipped += 1;
                        continue;
                    }
                    output.push(Candidate {
                        path: entry.path(),
                        category: source.category,
                        reason: source.reason,
                    });
                }
                Err(_) => skipped += 1,
            },
            Err(_) => skipped += 1,
        }
    }
    skipped
}

fn collect_attention_children(root: &Path, note: &'static str, output: &mut Vec<Candidate>) -> u64 {
    let read_dir = match fs::read_dir(root) {
        Ok(read_dir) => read_dir,
        Err(_) => return 1,
    };

    let mut skipped = 0;
    for entry in read_dir {
        match entry {
            Ok(entry) => match entry.file_type() {
                Ok(file_type) if file_type.is_symlink() => skipped += 1,
                Ok(_) => output.push(Candidate {
                    path: entry.path(),
                    category: "需确认区域",
                    reason: note,
                }),
                Err(_) => skipped += 1,
            },
            Err(_) => skipped += 1,
        }
    }
    skipped
}

fn scan_candidates(
    channel: &Channel<ScanEvent>,
    root_label: &str,
    candidates: Vec<Candidate>,
    policy: &SafetyPolicy,
    expected_tier: SafetyTier,
    skipped_entries: u64,
) -> Result<ScanSummary, String> {
    let started_at = Instant::now();
    let total_entries = candidates.len();

    channel
        .send(ScanEvent::Started {
            root: root_label.to_string(),
            total_entries,
        })
        .map_err(|error| format!("无法建立扫描结果流：{error}"))?;

    let items_done = AtomicUsize::new(0);
    let last_reported = AtomicUsize::new(0);
    let file_count = AtomicU64::new(0);
    let total_bytes = AtomicU64::new(0);
    let denied_entries = AtomicU64::new(0);
    let item_count = AtomicUsize::new(0);

    candidates.par_iter().for_each(|candidate| {
        let (item, skipped) = build_item(candidate, policy, expected_tier);
        denied_entries.fetch_add(skipped, Ordering::Relaxed);
        let done = items_done.fetch_add(1, Ordering::Relaxed) + 1;

        if let Some(item) = item {
            item_count.fetch_add(1, Ordering::Relaxed);
            let files = file_count.fetch_add(item.file_count, Ordering::Relaxed) + item.file_count;
            let bytes = total_bytes.fetch_add(item.size, Ordering::Relaxed) + item.size;

            let _ = channel.send(ScanEvent::Item { item });

            // 并行线程的完成顺序不等于编号顺序。用 fetch_max 保证进度条只前进不后退。
            let previous = last_reported.fetch_max(done, Ordering::Relaxed);
            if done > previous {
                let _ = channel.send(ScanEvent::Progress {
                    items_done: done,
                    items_total: total_entries,
                    file_count: files,
                    total_bytes: bytes,
                });
            }
        } else {
            let previous = last_reported.fetch_max(done, Ordering::Relaxed);
            if done > previous {
                let _ = channel.send(ScanEvent::Progress {
                    items_done: done,
                    items_total: total_entries,
                    file_count: file_count.load(Ordering::Relaxed),
                    total_bytes: total_bytes.load(Ordering::Relaxed),
                });
            }
        }
    });

    let duration_ms = started_at.elapsed().as_millis() as u64;
    let summary = ScanSummary {
        root: root_label.to_string(),
        item_count: item_count.load(Ordering::Relaxed),
        file_count: file_count.load(Ordering::Relaxed),
        total_bytes: total_bytes.load(Ordering::Relaxed),
        skipped_entries,
        denied_entries: denied_entries.load(Ordering::Relaxed),
        duration_ms,
    };

    channel
        .send(ScanEvent::Finished {
            item_count: summary.item_count,
            file_count: summary.file_count,
            total_bytes: summary.total_bytes,
            skipped_entries: summary.skipped_entries,
            denied_entries: summary.denied_entries,
            duration_ms,
        })
        .map_err(|error| format!("扫描完成但无法发送汇总：{error}"))?;

    Ok(summary)
}

fn build_item(
    candidate: &Candidate,
    policy: &SafetyPolicy,
    expected_tier: SafetyTier,
) -> (Option<CacheItem>, u64) {
    let path = candidate.path.as_path();

    // 防御性二次校验：即使扫描范围写错，也不把非预期档位送进 UI。
    match policy.classify(path) {
        Ok(tier) if tier == expected_tier => {}
        _ => return (None, 1),
    }

    let size = path_size(path);
    // 空目录/空文件没有回收价值，不进入列表。
    if size.bytes == 0 {
        return (None, size.skipped);
    }

    let Ok(metadata) = fs::symlink_metadata(path) else {
        return (None, size.skipped + 1);
    };
    let Some(name) = path.file_name() else {
        return (None, size.skipped + 1);
    };

    let item = CacheItem {
        id: path.to_string_lossy().into_owned(),
        path: path.to_string_lossy().into_owned(),
        name: name.to_string_lossy().into_owned(),
        size: size.bytes,
        file_count: size.files,
        category: candidate.category.to_string(),
        safety_tier: expected_tier,
        is_dir: metadata.is_dir(),
        last_modified: modified_ms(&metadata),
        reason: candidate.reason.to_string(),
    };

    (Some(item), size.skipped)
}

fn resolve_cache_sources(
    home: &Path,
    requested: Option<&[String]>,
) -> Result<(Vec<CacheSource>, Vec<&'static str>), String> {
    let selected: Vec<String> = match requested {
        Some(groups) if !groups.is_empty() => groups.to_vec(),
        Some(_) => return Err("请至少选择一个缓存分组".to_string()),
        None => vec!["app".to_string()],
    };

    let mut sources = Vec::new();
    let mut labels: Vec<&'static str> = Vec::new();

    let mut add = |path: PathBuf, category: &'static str, reason: &'static str| {
        if path.is_dir() {
            sources.push(CacheSource {
                path,
                category,
                reason,
            });
        }
    };

    for group in selected {
        match group.as_str() {
            "app" => {
                labels.push("应用缓存");
                add(
                    home.join("Library/Caches"),
                    "应用缓存",
                    "应用缓存：删除后应用会在需要时自动重建，不影响个人文档与设置",
                );
            }
            "logs" => {
                labels.push("日志");
                add(
                    home.join("Library/Logs"),
                    "日志",
                    "日志文件：删除后应用会在需要时重新生成，通常不影响文档与设置",
                );
            }
            "developer" => {
                labels.push("开发者缓存");
                add(
                    home.join("Library/Developer/Xcode/DerivedData"),
                    "开发者缓存",
                    "Xcode 构建产物：删除后重新构建即可再生，不影响源代码",
                );
                add(
                    home.join("Library/Developer/CoreSimulator/Caches"),
                    "开发者缓存",
                    "模拟器缓存：删除后可按需重新生成",
                );
            }
            "packages" => {
                labels.push("包管理器缓存");
                add(
                    home.join(".npm/_cacache"),
                    "包管理器缓存",
                    "npm 下载缓存：删除后需要时会重新下载",
                );
                add(
                    home.join(".npm/_logs"),
                    "包管理器缓存",
                    "npm 日志：删除后不影响已安装依赖",
                );
                add(
                    home.join(".gradle/caches"),
                    "包管理器缓存",
                    "Gradle 依赖缓存：删除后下次构建会重新下载",
                );
                add(
                    home.join(".cache/pip"),
                    "包管理器缓存",
                    "pip 下载缓存：删除后需要时会重新下载",
                );
                add(
                    home.join(".cargo/registry/cache"),
                    "包管理器缓存",
                    "Cargo 依赖缓存：删除后下次构建会重新下载",
                );
            }
            other => return Err(format!("未知缓存分组：{other}")),
        }
    }

    if sources.is_empty() {
        return Err("所选缓存目录不存在或不可读取".to_string());
    }
    labels.dedup();
    Ok((sources, labels))
}

fn attention_note(root: &Path, home: &Path) -> &'static str {
    let relative = root
        .strip_prefix(home)
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|_| root.to_string_lossy().into_owned());

    if relative.contains("iOS DeviceSupport") {
        "设备符号文件：删除后需要重新连接设备生成，部分旧系统版本可能无法再生"
    } else if relative.contains("com.docker.docker") {
        "容器、镜像与卷数据：删除会丢失全部容器数据，且无法恢复"
    } else if relative.contains("CoreSimulator/Devices") {
        "模拟器设备状态：删除后模拟器数据不可恢复，需要重新创建"
    } else if relative.contains(".ollama/models") {
        "本地模型权重：删除后需要重新下载"
    } else if relative.contains("huggingface") {
        "模型缓存：删除后需要重新下载"
    } else if relative.contains("MobileSync/Backup") {
        "iOS 备份：属于个人数据，仅在确认不再需要时删除"
    } else {
        "需确认区域：删除可能影响应用或设备数据，请逐项确认"
    }
}

fn modified_ms(metadata: &fs::Metadata) -> Option<u64> {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modified_time_is_serializable() {
        let temp = tempfile::TempDir::new().unwrap();
        let file = temp.path().join("a.bin");
        fs::write(&file, b"hello").unwrap();
        let metadata = fs::symlink_metadata(&file).unwrap();
        assert!(modified_ms(&metadata).is_some());
    }

    #[test]
    fn path_size_counts_files_and_bytes() {
        let temp = tempfile::TempDir::new().unwrap();
        let nested = temp.path().join("nested");
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join("a.bin"), vec![0u8; 8192]).unwrap();
        fs::write(nested.join("b.bin"), vec![0u8; 4096]).unwrap();

        let size = path_size(temp.path());
        assert_eq!(size.files, 2);
        assert!(size.bytes >= 12 * 1024);
    }

    #[test]
    fn path_size_ignores_symlinks() {
        let temp = tempfile::TempDir::new().unwrap();
        let outside = temp.path().join("outside.bin");
        fs::write(&outside, vec![0u8; 4096]).unwrap();
        let root = temp.path().join("root");
        fs::create_dir_all(&root).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("link")).unwrap();

        let size = path_size(&root);
        assert_eq!(size.files, 0);
        assert_eq!(size.bytes, 0);
    }

    #[test]
    fn build_item_requires_child_of_allowed_root() {
        let temp = tempfile::TempDir::new().unwrap();
        let file = temp.path().join("not-a-cache.bin");
        fs::write(&file, b"data").unwrap();
        let policy = SafetyPolicy::new(vec![temp.path().to_path_buf()], vec![]);

        let root_candidate = Candidate {
            path: temp.path().to_path_buf(),
            category: "应用缓存",
            reason: "测试",
        };
        assert!(build_item(&root_candidate, &policy, SafetyTier::Safe)
            .0
            .is_none());

        let child_candidate = Candidate {
            path: file,
            category: "应用缓存",
            reason: "测试",
        };
        assert!(build_item(&child_candidate, &policy, SafetyTier::Safe)
            .0
            .is_some());
    }

    #[test]
    fn scan_events_serialize_with_camel_case_fields() {
        let event = ScanEvent::Started {
            root: "/tmp".to_string(),
            total_entries: 3,
        };
        let value = serde_json::to_value(event).unwrap();

        assert_eq!(value["type"].as_str(), Some("started"));
        assert_eq!(value["totalEntries"].as_u64(), Some(3));

        let progress = ScanEvent::Progress {
            items_done: 1,
            items_total: 3,
            file_count: 4,
            total_bytes: 1024,
        };
        let value = serde_json::to_value(progress).unwrap();
        assert_eq!(value["itemsDone"].as_u64(), Some(1));
        assert_eq!(value["totalBytes"].as_u64(), Some(1024));
    }
}
