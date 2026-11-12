//! 重复文件：大小分组 → 前 4 KB 哈希 → 完整 BLAKE3。
//!
//! 安全约束：
//! - 只扫描固定的用户目录，不跟随符号链接；
//! - 跳过隐藏目录、版本库、应用包与常见构建目录；
//! - 每组至少保留一份，保留项由后端选择并标记；
//! - 删除前再次校验保留项存在、待删项与保留项哈希一致，再交给 cleaner 做路径白名单校验。

use crate::cleaner::{self, DeleteReport};
use crate::safety::SafetyPolicy;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, UNIX_EPOCH};
use tauri::ipc::Channel;
use walkdir::WalkDir;

use super::rules::should_descend;

const MB: u64 = 1024 * 1024;
const DEFAULT_MIN_SIZE: u64 = MB;
const DEFAULT_MAX_GROUPS: usize = 200;
const MAX_FILES_PER_GROUP: usize = 50;
const HEAD_BYTES: u64 = 4096;
const HASH_CHUNK: usize = 64 * 1024;
pub const ROOT_NAMES: [&str; 4] = ["Downloads", "Desktop", "Documents", "Movies"];

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateScanOptions {
    pub roots: Option<Vec<String>>,
    pub min_size_bytes: Option<u64>,
    pub max_groups: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateFile {
    pub id: String,
    pub path: String,
    pub name: String,
    pub size: u64,
    pub modified_ms: Option<u64>,
    pub is_keep: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateGroup {
    pub id: String,
    pub size: u64,
    pub wasted_bytes: u64,
    pub total_files: usize,
    pub files: Vec<DuplicateFile>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateScanSummary {
    pub roots: Vec<String>,
    pub files_scanned: u64,
    pub bytes_scanned: u64,
    pub files_hashed: u64,
    pub bytes_hashed: u64,
    pub duplicate_groups: usize,
    pub duplicate_files: usize,
    pub reclaimable_bytes: u64,
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
pub enum DuplicateScanEvent {
    Started {
        roots: Vec<String>,
        min_size_bytes: u64,
    },
    Progress {
        files_scanned: u64,
        files_hashed: u64,
        bytes_hashed: u64,
        skipped_entries: u64,
        elapsed_ms: u64,
    },
    Finished {
        cancelled: bool,
        groups: Vec<DuplicateGroup>,
        summary: DuplicateScanSummary,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateDeleteGroup {
    pub id: String,
    pub keep_path: String,
    pub delete_paths: Vec<String>,
}

#[derive(Debug, Default)]
struct ScanStats {
    files_scanned: AtomicU64,
    bytes_scanned: AtomicU64,
    files_hashed: AtomicU64,
    bytes_hashed: AtomicU64,
    skipped_entries: AtomicU64,
}

#[derive(Debug, Clone)]
struct FileEntry {
    path: PathBuf,
    size: u64,
    modified_ms: Option<u64>,
}

pub fn scan_duplicates(
    channel: &Channel<DuplicateScanEvent>,
    cancel: Arc<AtomicBool>,
    options: &DuplicateScanOptions,
) -> Result<DuplicateScanSummary, String> {
    let started = Instant::now();
    let roots = resolve_roots(options.roots.as_deref())?;
    let min_size = options.min_size_bytes.unwrap_or(DEFAULT_MIN_SIZE).max(1);
    let max_groups = options
        .max_groups
        .unwrap_or(DEFAULT_MAX_GROUPS)
        .clamp(1, 2000);

    let root_strings: Vec<String> = roots
        .iter()
        .map(|root| root.to_string_lossy().into_owned())
        .collect();

    channel
        .send(DuplicateScanEvent::Started {
            roots: root_strings.clone(),
            min_size_bytes: min_size,
        })
        .map_err(|error| format!("无法建立重复文件扫描流：{error}"))?;

    let stats = Arc::new(ScanStats::default());
    let stop_reporter = Arc::new(AtomicBool::new(false));
    let reporter = spawn_reporter(
        channel.clone(),
        stop_reporter.clone(),
        stats.clone(),
        started,
    );

    let groups = find_duplicate_groups(&roots, min_size, max_groups, cancel.as_ref(), &stats);

    stop_reporter.store(true, Ordering::Relaxed);
    let _ = reporter.join();

    let cancelled = cancel.load(Ordering::Relaxed);
    let duplicate_files = groups.iter().map(|group| group.total_files).sum();
    let reclaimable_bytes = groups.iter().map(|group| group.wasted_bytes).sum();
    let summary = DuplicateScanSummary {
        roots: root_strings,
        files_scanned: stats.files_scanned.load(Ordering::Relaxed),
        bytes_scanned: stats.bytes_scanned.load(Ordering::Relaxed),
        files_hashed: stats.files_hashed.load(Ordering::Relaxed),
        bytes_hashed: stats.bytes_hashed.load(Ordering::Relaxed),
        duplicate_groups: groups.len(),
        duplicate_files,
        reclaimable_bytes,
        skipped_entries: stats.skipped_entries.load(Ordering::Relaxed),
        duration_ms: started.elapsed().as_millis() as u64,
        cancelled,
    };

    channel
        .send(DuplicateScanEvent::Finished {
            cancelled,
            groups,
            summary: summary.clone(),
        })
        .map_err(|error| format!("扫描完成但无法发送重复文件结果：{error}"))?;

    Ok(summary)
}

/// 校验并执行重复文件删除。返回报告中只包含实际移入废纸篓的文件。
pub fn delete_duplicates(
    groups: &[DuplicateDeleteGroup],
    policy: &SafetyPolicy,
    allow_ask: bool,
    cancel: &AtomicBool,
) -> Result<DeleteReport, String> {
    let paths = validate_duplicate_deletion(groups, policy, allow_ask, cancel)?;
    if paths.is_empty() {
        return Err("没有可删除的重复文件".to_string());
    }
    Ok(cleaner::move_to_trash_with_policy(
        &paths, allow_ask, policy,
    ))
}

/// 删除前的二次校验：保留项存在、待删项与保留项大小/哈希一致、路径仍通过白名单。
pub fn validate_duplicate_deletion(
    groups: &[DuplicateDeleteGroup],
    policy: &SafetyPolicy,
    allow_ask: bool,
    cancel: &AtomicBool,
) -> Result<Vec<PathBuf>, String> {
    if groups.is_empty() {
        return Err("没有选择重复文件分组".to_string());
    }

    let mut keep_paths: HashSet<PathBuf> = HashSet::new();
    let mut delete_paths: Vec<PathBuf> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let hash_stats = ScanStats::default();

    for group in groups {
        if group.id.trim().is_empty() {
            return Err("重复文件分组缺少标识".to_string());
        }
        if cancel.load(Ordering::Relaxed) {
            return Err("删除已取消".to_string());
        }

        let keep = canonical_file(&group.keep_path)?;
        if !keep_paths.insert(keep.clone()) {
            return Err(format!("保留项重复出现：{}", keep.display()));
        }
        let keep_size = file_size(&keep)?;
        let keep_hash = hash_file(&keep, &hash_stats)
            .ok_or_else(|| format!("无法读取保留项：{}", keep.display()))?;

        if group.delete_paths.is_empty() {
            return Err(format!("分组 {} 没有待删除文件", group.id));
        }

        let mut group_count = 0;
        for raw in &group.delete_paths {
            if cancel.load(Ordering::Relaxed) {
                return Err("删除已取消".to_string());
            }
            let path = canonical_file(raw)?;
            if path == keep {
                return Err("保留项不能被选中删除".to_string());
            }
            if !seen.insert(path.clone()) {
                continue;
            }

            let size = file_size(&path)?;
            if size != keep_size {
                return Err(format!("文件大小不一致，已拒绝：{}", path.display()));
            }
            policy
                .validate_for_deletion(&path, allow_ask)
                .map_err(|reason| format!("{}：{}", path.display(), reason))?;

            let delete_hash = hash_file(&path, &hash_stats)
                .ok_or_else(|| format!("无法读取文件：{}", path.display()))?;
            if keep_hash != delete_hash {
                return Err(format!("文件内容不一致，已拒绝：{}", path.display()));
            }

            group_count += 1;
            delete_paths.push(path);
        }

        if group_count == 0 {
            return Err(format!("分组 {} 没有可删除文件", group.id));
        }
    }

    for path in &delete_paths {
        if keep_paths.contains(path) {
            return Err(format!("保留项出现在待删清单中：{}", path.display()));
        }
    }

    Ok(delete_paths)
}

fn find_duplicate_groups(
    roots: &[PathBuf],
    min_size: u64,
    max_groups: usize,
    cancel: &AtomicBool,
    stats: &ScanStats,
) -> Vec<DuplicateGroup> {
    let mut by_size: HashMap<u64, Vec<FileEntry>> = HashMap::new();

    for root in roots {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let walker = WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_entry(should_descend);

        for entry in walker {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => {
                    stats.skipped_entries.fetch_add(1, Ordering::Relaxed);
                    continue;
                }
            };
            if entry.depth() == 0 {
                continue;
            }
            let file_type = entry.file_type();
            if file_type.is_symlink() || !file_type.is_file() {
                continue;
            }
            let metadata = match entry.metadata() {
                Ok(metadata) => metadata,
                Err(_) => {
                    stats.skipped_entries.fetch_add(1, Ordering::Relaxed);
                    continue;
                }
            };
            let size = metadata.len();
            stats.files_scanned.fetch_add(1, Ordering::Relaxed);
            stats.bytes_scanned.fetch_add(size, Ordering::Relaxed);
            if size < min_size {
                continue;
            }
            let modified_ms = metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map(|duration| duration.as_millis() as u64);
            by_size.entry(size).or_default().push(FileEntry {
                path: entry.path().to_path_buf(),
                size,
                modified_ms,
            });
        }
    }

    let mut head_groups: Vec<Vec<FileEntry>> = by_size
        .into_values()
        .filter(|files| files.len() >= 2)
        .flat_map(|files| group_by_head(files, cancel, stats))
        .filter(|files| files.len() >= 2)
        .collect();

    head_groups.sort_by(|left, right| {
        right
            .len()
            .cmp(&left.len())
            .then_with(|| left[0].size.cmp(&right[0].size))
    });

    let mut groups: Vec<DuplicateGroup> = head_groups
        .par_iter()
        .flat_map(|files| build_groups(files, cancel, stats))
        .collect();
    groups.sort_by(|left, right| {
        right
            .wasted_bytes
            .cmp(&left.wasted_bytes)
            .then_with(|| left.id.cmp(&right.id))
    });
    groups.truncate(max_groups);
    groups
}

fn group_by_head(
    files: Vec<FileEntry>,
    cancel: &AtomicBool,
    stats: &ScanStats,
) -> Vec<Vec<FileEntry>> {
    let mut by_head: HashMap<[u8; 32], Vec<FileEntry>> = HashMap::new();
    for file in files {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let Some(head) = hash_head(&file.path, stats) else {
            stats.skipped_entries.fetch_add(1, Ordering::Relaxed);
            continue;
        };
        by_head.entry(head).or_default().push(file);
    }
    by_head
        .into_values()
        .filter(|files| files.len() >= 2)
        .collect()
}

fn build_groups(
    files: &[FileEntry],
    cancel: &AtomicBool,
    stats: &ScanStats,
) -> Vec<DuplicateGroup> {
    if cancel.load(Ordering::Relaxed) {
        return Vec::new();
    }
    let mut by_hash: HashMap<String, Vec<&FileEntry>> = HashMap::new();
    for file in files {
        if cancel.load(Ordering::Relaxed) {
            return Vec::new();
        }
        let Some(hash) = hash_file(&file.path, stats) else {
            stats.skipped_entries.fetch_add(1, Ordering::Relaxed);
            continue;
        };
        by_hash.entry(hash).or_default().push(file);
    }

    by_hash
        .into_iter()
        .filter(|(_, files)| files.len() >= 2)
        .map(|(hash, mut candidates)| {
            candidates.sort_by(|left, right| {
                left.modified_ms
                    .unwrap_or(u64::MAX)
                    .cmp(&right.modified_ms.unwrap_or(u64::MAX))
                    .then_with(|| left.path.cmp(&right.path))
            });

            let total_files = candidates.len();
            let keep_path = candidates[0].path.clone();
            let size = candidates[0].size;
            let shown: Vec<DuplicateFile> = candidates
                .iter()
                .take(MAX_FILES_PER_GROUP)
                .map(|file| DuplicateFile {
                    id: file.path.to_string_lossy().into_owned(),
                    path: file.path.to_string_lossy().into_owned(),
                    name: file
                        .path
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_else(|| file.path.to_string_lossy().into_owned()),
                    size: file.size,
                    modified_ms: file.modified_ms,
                    is_keep: file.path == keep_path,
                })
                .collect();

            DuplicateGroup {
                id: format!("{hash}-{size}"),
                size,
                wasted_bytes: size.saturating_mul(total_files.saturating_sub(1) as u64),
                total_files,
                files: shown,
            }
        })
        .collect()
}

fn hash_head(path: &Path, stats: &ScanStats) -> Option<[u8; 32]> {
    let mut file = File::open(path).ok()?;
    let mut buffer = vec![0u8; HEAD_BYTES as usize];
    let mut read = 0;
    loop {
        match file.read(&mut buffer[read..]) {
            Ok(0) => break,
            Ok(bytes) => {
                read += bytes;
                if read == buffer.len() {
                    break;
                }
            }
            Err(_) => return None,
        }
    }
    stats.files_hashed.fetch_add(1, Ordering::Relaxed);
    stats.bytes_hashed.fetch_add(read as u64, Ordering::Relaxed);
    Some(*blake3::hash(&buffer[..read]).as_bytes())
}

fn hash_file(path: &Path, stats: &ScanStats) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = vec![0u8; HASH_CHUNK];
    loop {
        match file.read(&mut buffer) {
            Ok(0) => break,
            Ok(bytes) => {
                hasher.update(&buffer[..bytes]);
                stats
                    .bytes_hashed
                    .fetch_add(bytes as u64, Ordering::Relaxed);
            }
            Err(_) => return None,
        }
    }
    stats.files_hashed.fetch_add(1, Ordering::Relaxed);
    Some(hasher.finalize().to_hex().to_string())
}

fn canonical_file(raw: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(raw);
    if !path.is_absolute() {
        return Err(format!("不是绝对路径：{raw}"));
    }
    let metadata = path
        .symlink_metadata()
        .map_err(|_| format!("文件不存在或无权访问：{raw}"))?;
    if metadata.file_type().is_symlink() {
        return Err(format!("符号链接不参与删除：{raw}"));
    }
    if !metadata.is_file() {
        return Err(format!("不是普通文件：{raw}"));
    }
    path.canonicalize()
        .map_err(|_| format!("无法解析路径：{raw}"))
}

fn file_size(path: &Path) -> Result<u64, String> {
    std::fs::metadata(path)
        .map(|metadata| metadata.len())
        .map_err(|error| format!("无法读取文件大小 {}：{error}", path.display()))
}

fn resolve_roots(requested: Option<&[String]>) -> Result<Vec<PathBuf>, String> {
    let home = dirs::home_dir().ok_or_else(|| "无法定位当前用户主目录".to_string())?;
    let names: Vec<String> = match requested {
        Some(names) if !names.is_empty() => names.to_vec(),
        Some(_) => return Err("请至少选择一个扫描目录".to_string()),
        None => ROOT_NAMES.iter().map(|name| name.to_string()).collect(),
    };

    for name in &names {
        if !ROOT_NAMES.contains(&name.as_str()) {
            return Err(format!("只支持默认用户目录，收到未知目录：{name}"));
        }
    }

    let mut roots = Vec::new();
    for name in names {
        let path = home.join(&name);
        let Ok(metadata) = path.symlink_metadata() else {
            continue;
        };
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            continue;
        }
        let Ok(canonical) = path.canonicalize() else {
            continue;
        };
        if !roots.contains(&canonical) {
            roots.push(canonical);
        }
    }

    if roots.is_empty() {
        Err("所选目录不存在或不可读取".to_string())
    } else {
        Ok(roots)
    }
}

fn spawn_reporter(
    channel: Channel<DuplicateScanEvent>,
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
            let _ = channel.send(DuplicateScanEvent::Progress {
                files_scanned: stats.files_scanned.load(Ordering::Relaxed),
                files_hashed: stats.files_hashed.load(Ordering::Relaxed),
                bytes_hashed: stats.bytes_hashed.load(Ordering::Relaxed),
                skipped_entries: stats.skipped_entries.load(Ordering::Relaxed),
                elapsed_ms: started.elapsed().as_millis() as u64,
            });
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_file(path: &Path, bytes: &[u8]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    #[test]
    fn finds_duplicate_groups_and_marks_keep_file() {
        let temp = tempfile::TempDir::new().unwrap();
        let root = temp.path();
        let content = vec![7u8; 8192];
        write_file(&root.join("a.bin"), &content);
        write_file(&root.join("nested/b.bin"), &content);
        write_file(&root.join("different.bin"), &vec![8u8; 8192]);

        let policy = SafetyPolicy::new(vec![root.to_path_buf()], vec![]);
        let cancel = AtomicBool::new(false);
        let stats = ScanStats::default();
        let groups = find_duplicate_groups(&[root.to_path_buf()], 1, 10, &cancel, &stats);

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].total_files, 2);
        assert_eq!(
            groups[0].files.iter().filter(|file| file.is_keep).count(),
            1
        );
        assert_eq!(groups[0].wasted_bytes, 8192);
        assert!(groups[0].files.iter().any(|file| file.name == "a.bin"));
        assert!(groups[0].files.iter().any(|file| file.name == "b.bin"));
        assert!(!groups[0]
            .files
            .iter()
            .any(|file| file.name == "different.bin"));

        // 保留项本身仍应通过白名单校验。
        let keep = groups[0]
            .files
            .iter()
            .find(|file| file.is_keep)
            .unwrap()
            .path
            .clone();
        assert!(policy
            .validate_for_deletion(Path::new(&keep), false)
            .is_ok());
    }

    #[test]
    fn validation_rejects_missing_keep_or_hash_mismatch() {
        let temp = tempfile::TempDir::new().unwrap();
        let root = temp.path();
        let content = vec![1u8; 4096];
        write_file(&root.join("a.bin"), &content);
        write_file(&root.join("b.bin"), &content);
        write_file(&root.join("c.bin"), &vec![2u8; 4096]);

        let policy = SafetyPolicy::new(vec![root.to_path_buf()], vec![]);
        let cancel = AtomicBool::new(false);

        let valid = vec![DuplicateDeleteGroup {
            id: "g1".to_string(),
            keep_path: root.join("a.bin").to_string_lossy().into_owned(),
            delete_paths: vec![root.join("b.bin").to_string_lossy().into_owned()],
        }];
        assert!(validate_duplicate_deletion(&valid, &policy, false, &cancel).is_ok());

        let deleting_keep = vec![DuplicateDeleteGroup {
            id: "g1".to_string(),
            keep_path: root.join("a.bin").to_string_lossy().into_owned(),
            delete_paths: vec![
                root.join("a.bin").to_string_lossy().into_owned(),
                root.join("b.bin").to_string_lossy().into_owned(),
            ],
        }];
        assert!(validate_duplicate_deletion(&deleting_keep, &policy, false, &cancel).is_err());

        let mismatch = vec![DuplicateDeleteGroup {
            id: "g1".to_string(),
            keep_path: root.join("a.bin").to_string_lossy().into_owned(),
            delete_paths: vec![root.join("c.bin").to_string_lossy().into_owned()],
        }];
        assert!(validate_duplicate_deletion(&mismatch, &policy, false, &cancel).is_err());
    }

    #[test]
    fn delete_paths_cannot_overlap_any_keep_path() {
        let temp = tempfile::TempDir::new().unwrap();
        let root = temp.path();
        let content = vec![3u8; 4096];
        write_file(&root.join("a.bin"), &content);
        write_file(&root.join("b.bin"), &content);
        write_file(&root.join("c.bin"), &content);

        let policy = SafetyPolicy::new(vec![root.to_path_buf()], vec![]);
        let cancel = AtomicBool::new(false);
        let groups = vec![
            DuplicateDeleteGroup {
                id: "g1".to_string(),
                keep_path: root.join("a.bin").to_string_lossy().into_owned(),
                delete_paths: vec![root.join("b.bin").to_string_lossy().into_owned()],
            },
            DuplicateDeleteGroup {
                id: "g2".to_string(),
                keep_path: root.join("b.bin").to_string_lossy().into_owned(),
                delete_paths: vec![root.join("c.bin").to_string_lossy().into_owned()],
            },
        ];
        assert!(validate_duplicate_deletion(&groups, &policy, false, &cancel).is_err());
    }

    #[test]
    fn duplicate_events_serialize_with_camel_case_fields() {
        let event = DuplicateScanEvent::Progress {
            files_scanned: 4,
            files_hashed: 2,
            bytes_hashed: 8192,
            skipped_entries: 1,
            elapsed_ms: 220,
        };
        let value = serde_json::to_value(event).unwrap();
        assert_eq!(value["type"], "progress");
        assert_eq!(value["filesScanned"].as_u64(), Some(4));
        assert_eq!(value["bytesHashed"].as_u64(), Some(8192));
    }
}
