//! A 类：`~/Library/Caches` 最小流式扫描器。
//!
//! 设计取舍：
//! - 以 `~/Library/Caches` 的**一级子项**作为结果粒度（每个应用/工具一个条目），
//!   避免把几十万个缓存文件直接推给前端；条目大小递归汇总。
//! - 并行计算条目大小，算完一个就通过 Tauri Channel 推一个，前端不必等待全量扫描。
//! - 扫描器只展示白名单内的 safe 档路径；符号链接、`.git`、越界路径在进入列表前就被过滤。
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

pub fn scan_library_caches(channel: &Channel<ScanEvent>) -> Result<ScanSummary, String> {
    let started_at = Instant::now();

    let home = dirs::home_dir().ok_or_else(|| "无法定位当前用户主目录".to_string())?;
    let root = home.join("Library/Caches");
    let read_dir =
        fs::read_dir(&root).map_err(|error| format!("无法读取 {}：{}", root.display(), error))?;

    let mut candidates: Vec<PathBuf> = Vec::new();
    let mut skipped_entries: u64 = 0;

    for entry in read_dir {
        match entry {
            Ok(entry) => match entry.file_type() {
                // 符号链接不参与扫描：删除时也会被 safety 拒绝，提前过滤避免误导用户。
                Ok(file_type) if file_type.is_symlink() => skipped_entries += 1,
                Ok(_) => candidates.push(entry.path()),
                Err(_) => skipped_entries += 1,
            },
            Err(_) => skipped_entries += 1,
        }
    }

    let total_entries = candidates.len();
    channel
        .send(ScanEvent::Started {
            root: root.to_string_lossy().into_owned(),
            total_entries,
        })
        .map_err(|error| format!("无法建立扫描结果流：{error}"))?;

    let policy = SafetyPolicy::macos_default();
    let items_done = AtomicUsize::new(0);
    let last_reported = AtomicUsize::new(0);
    let file_count = AtomicU64::new(0);
    let total_bytes = AtomicU64::new(0);
    let denied_entries = AtomicU64::new(0);
    let item_count = AtomicUsize::new(0);

    candidates.par_iter().for_each(|path| {
        let (item, skipped) = build_item(path, &policy);
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
        root: root.to_string_lossy().into_owned(),
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

fn build_item(path: &Path, policy: &SafetyPolicy) -> (Option<CacheItem>, u64) {
    // 防御性二次校验：即使扫描范围写错，也不把任何非 safe 路径送进 UI。
    match policy.classify(path) {
        Ok(SafetyTier::Safe) => {}
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
    let is_dir = metadata.is_dir();

    let item = CacheItem {
        id: path.to_string_lossy().into_owned(),
        path: path.to_string_lossy().into_owned(),
        name: name.to_string_lossy().into_owned(),
        size: size.bytes,
        file_count: size.files,
        category: if is_dir {
            "应用缓存"
        } else {
            "缓存文件"
        }
        .to_string(),
        safety_tier: SafetyTier::Safe,
        last_modified: modified_ms(&metadata),
        reason: "应用缓存：删除后应用会在需要时自动重建，不影响个人文档与设置".to_string(),
    };

    (Some(item), size.skipped)
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

        // 白名单根目录本身不可作为条目。
        assert!(build_item(temp.path(), &policy).0.is_none());
        assert!(build_item(&file, &policy).0.is_some());
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
