//! 空间分析：只读扫描目录树并聚合大小，供前端树图下钻。
//!
//! 安全边界：
//! - 只允许分析主目录或 `/Volumes` 下的具体目录；系统路径与卷根拒绝；
//! - 不跟随符号链接；
//! - `.git`、应用包与照片库不展开，只显示聚合大小；
//! - 目录节点永远不可删除，文件节点是否可删由 `SafetyPolicy` 决定。

use crate::fs_util::{bytes_on_disk, path_size};
use crate::safety::{SafetyPolicy, SafetyTier};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, UNIX_EPOCH};
use tauri::ipc::Channel;

use super::rules::is_package_dir;

const DEFAULT_MAX_DEPTH: usize = 4;
const DEFAULT_MAX_CHILDREN: usize = 18;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpaceScanOptions {
    pub root: Option<String>,
    pub max_depth: Option<usize>,
    pub max_children: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpaceNode {
    pub id: String,
    pub path: String,
    pub name: String,
    pub size: u64,
    pub file_count: u64,
    pub is_dir: bool,
    pub safety_tier: Option<SafetyTier>,
    pub deletable: bool,
    pub reason: String,
    pub modified_ms: Option<u64>,
    pub hidden_children: usize,
    pub is_synthetic: bool,
    pub children: Vec<SpaceNode>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpaceSummary {
    pub root: String,
    pub files_scanned: u64,
    pub bytes_scanned: u64,
    pub skipped_entries: u64,
    pub duration_ms: u64,
    pub cancelled: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpaceAnalysis {
    pub tree: SpaceNode,
    pub summary: SpaceSummary,
}

#[derive(Debug, Clone, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SpaceScanEvent {
    Started {
        root: String,
    },
    Progress {
        files_scanned: u64,
        bytes_scanned: u64,
        skipped_entries: u64,
        elapsed_ms: u64,
    },
}

#[derive(Debug, Default)]
struct ScanStats {
    files: AtomicU64,
    bytes: AtomicU64,
    skipped: AtomicU64,
}

struct ScanContext<'a> {
    policy: &'a SafetyPolicy,
    cancel: &'a AtomicBool,
    stats: &'a ScanStats,
    max_depth: usize,
    max_children: usize,
}

pub fn analyze_space(
    channel: &Channel<SpaceScanEvent>,
    cancel: Arc<AtomicBool>,
    options: &SpaceScanOptions,
    policy: &SafetyPolicy,
) -> Result<SpaceAnalysis, String> {
    let started = Instant::now();
    let root = resolve_analysis_root(options.root.as_deref())?;
    let max_depth = options.max_depth.unwrap_or(DEFAULT_MAX_DEPTH).clamp(1, 8);
    let max_children = options
        .max_children
        .unwrap_or(DEFAULT_MAX_CHILDREN)
        .clamp(4, 80);

    channel
        .send(SpaceScanEvent::Started {
            root: root.to_string_lossy().into_owned(),
        })
        .map_err(|error| format!("无法建立空间分析进度流：{error}"))?;

    let stats = Arc::new(ScanStats::default());
    let stop_reporter = Arc::new(AtomicBool::new(false));
    let reporter = spawn_reporter(
        channel.clone(),
        stop_reporter.clone(),
        stats.clone(),
        started,
    );

    let mut context = ScanContext {
        policy,
        cancel: cancel.as_ref(),
        stats: stats.as_ref(),
        max_depth,
        max_children,
    };
    let tree = analyze_dir(&root, 0, &mut context)
        .ok_or_else(|| format!("无法读取目录：{}", root.display()))?;

    stop_reporter.store(true, Ordering::Relaxed);
    let _ = reporter.join();

    let summary = SpaceSummary {
        root: root.to_string_lossy().into_owned(),
        files_scanned: stats.files.load(Ordering::Relaxed),
        bytes_scanned: stats.bytes.load(Ordering::Relaxed),
        skipped_entries: stats.skipped.load(Ordering::Relaxed),
        duration_ms: started.elapsed().as_millis() as u64,
        cancelled: cancel.load(Ordering::Relaxed),
    };

    Ok(SpaceAnalysis { tree, summary })
}

fn resolve_analysis_root(requested: Option<&str>) -> Result<PathBuf, String> {
    let path = match requested {
        Some(value) if !value.trim().is_empty() => PathBuf::from(value),
        _ => dirs::home_dir().ok_or_else(|| "无法定位当前用户主目录".to_string())?,
    };

    if !path.is_absolute() {
        return Err("分析目录必须是绝对路径".to_string());
    }
    if path
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err("分析目录不能包含 ..".to_string());
    }

    let metadata = path
        .symlink_metadata()
        .map_err(|_| "所选目录不存在或无权访问".to_string())?;
    if metadata.file_type().is_symlink() {
        return Err("不分析符号链接目录".to_string());
    }
    if !metadata.is_dir() {
        return Err("请选择目录，而不是文件".to_string());
    }

    let canonical = path
        .canonicalize()
        .map_err(|_| "无法解析所选目录".to_string())?;
    if !crate::safety::is_analysis_root_allowed(&canonical) {
        return Err("只允许分析主目录或外接卷内的目录".to_string());
    }
    Ok(canonical)
}

fn analyze_dir(path: &Path, depth: usize, context: &mut ScanContext<'_>) -> Option<SpaceNode> {
    if context.cancel.load(Ordering::Relaxed) {
        return None;
    }

    let metadata = fs::symlink_metadata(path).ok()?;
    let read_dir = match fs::read_dir(path) {
        Ok(read_dir) => read_dir,
        Err(_) => {
            context.stats.skipped.fetch_add(1, Ordering::Relaxed);
            return None;
        }
    };

    let mut children = Vec::new();
    let mut total_size: u64 = 0;
    let mut total_files: u64 = 0;

    for entry in read_dir {
        if context.cancel.load(Ordering::Relaxed) {
            break;
        }

        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => {
                context.stats.skipped.fetch_add(1, Ordering::Relaxed);
                continue;
            }
        };

        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(_) => {
                context.stats.skipped.fetch_add(1, Ordering::Relaxed);
                continue;
            }
        };
        if file_type.is_symlink() {
            context.stats.skipped.fetch_add(1, Ordering::Relaxed);
            continue;
        }

        let child_path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();

        if file_type.is_file() {
            let metadata = match entry.metadata() {
                Ok(metadata) => metadata,
                Err(_) => {
                    context.stats.skipped.fetch_add(1, Ordering::Relaxed);
                    continue;
                }
            };
            let size = bytes_on_disk(&metadata);
            context.stats.files.fetch_add(1, Ordering::Relaxed);
            context.stats.bytes.fetch_add(size, Ordering::Relaxed);

            let node = file_node(child_path, name, size, &metadata, context.policy);
            total_size = total_size.saturating_add(size);
            total_files = total_files.saturating_add(1);
            children.push(node);
            continue;
        }

        if !file_type.is_dir() {
            continue;
        }

        let leaf = depth >= context.max_depth || is_package_dir(&name) || name == ".git";
        if leaf {
            let size = path_size(&child_path);
            let leaf_modified = entry.metadata().ok();
            context.stats.files.fetch_add(size.files, Ordering::Relaxed);
            context.stats.bytes.fetch_add(size.bytes, Ordering::Relaxed);
            context
                .stats
                .skipped
                .fetch_add(size.skipped, Ordering::Relaxed);

            total_size = total_size.saturating_add(size.bytes);
            total_files = total_files.saturating_add(size.files);
            children.push(SpaceNode {
                id: child_path.to_string_lossy().into_owned(),
                path: child_path.to_string_lossy().into_owned(),
                name,
                size: size.bytes,
                file_count: size.files,
                is_dir: true,
                safety_tier: None,
                deletable: false,
                reason: "目录未展开：可点击「在此目录重新分析」继续下钻；目录本身不可删除"
                    .to_string(),
                modified_ms: leaf_modified.as_ref().and_then(modified_ms),
                hidden_children: 0,
                is_synthetic: false,
                children: Vec::new(),
            });
            continue;
        }

        let Some(node) = analyze_dir(&child_path, depth + 1, context) else {
            if context.cancel.load(Ordering::Relaxed) {
                break;
            }
            continue;
        };
        total_size = total_size.saturating_add(node.size);
        total_files = total_files.saturating_add(node.file_count);
        children.push(node);
    }

    children.sort_by(|left, right| {
        right
            .size
            .cmp(&left.size)
            .then_with(|| left.name.cmp(&right.name))
    });

    let mut hidden_children = 0;
    if children.len() > context.max_children {
        hidden_children = children.len() - context.max_children;
        let hidden: Vec<SpaceNode> = children.split_off(context.max_children);
        let hidden_size: u64 = hidden.iter().map(|node| node.size).sum();
        let hidden_files: u64 = hidden.iter().map(|node| node.file_count).sum();
        children.push(SpaceNode {
            id: format!("synthetic:{}", path.to_string_lossy()),
            path: String::new(),
            name: format!("其他 {hidden_children} 项"),
            size: hidden_size,
            file_count: hidden_files,
            is_dir: false,
            safety_tier: None,
            deletable: false,
            reason: "未在树图中展开的其余条目；可进入具体目录后再查看".to_string(),
            modified_ms: None,
            hidden_children: 0,
            is_synthetic: true,
            children: Vec::new(),
        });
    }

    Some(SpaceNode {
        id: path.to_string_lossy().into_owned(),
        path: path.to_string_lossy().into_owned(),
        name: display_name(path, depth),
        size: total_size,
        file_count: total_files,
        is_dir: true,
        safety_tier: None,
        deletable: false,
        reason: "目录节点仅支持在 Finder 中定位；请进入目录选择具体文件".to_string(),
        modified_ms: modified_ms(&metadata),
        hidden_children,
        is_synthetic: false,
        children,
    })
}

fn file_node(
    path: PathBuf,
    name: String,
    size: u64,
    metadata: &fs::Metadata,
    policy: &SafetyPolicy,
) -> SpaceNode {
    let (safety_tier, deletable, reason) = match policy.classify(&path) {
        Ok(tier) => {
            let reason = match tier {
                SafetyTier::Safe => "位于删除白名单内，可移入废纸篓",
                SafetyTier::Ask => "位于需确认区域，删除前必须显式确认",
            };
            (Some(tier), true, reason.to_string())
        }
        Err(reason) => (None, false, reason.to_string()),
    };

    SpaceNode {
        id: path.to_string_lossy().into_owned(),
        path: path.to_string_lossy().into_owned(),
        name,
        size,
        file_count: 1,
        is_dir: false,
        safety_tier,
        deletable,
        reason,
        modified_ms: modified_ms(metadata),
        hidden_children: 0,
        is_synthetic: false,
        children: Vec::new(),
    }
}

fn display_name(path: &Path, depth: usize) -> String {
    if depth == 0 {
        dirs::home_dir()
            .filter(|home| path == home)
            .map(|_| "主目录".to_string())
            .unwrap_or_else(|| {
                path.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.to_string_lossy().into_owned())
            })
    } else {
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string_lossy().into_owned())
    }
}

fn modified_ms(metadata: &fs::Metadata) -> Option<u64> {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as u64)
}

fn spawn_reporter(
    channel: Channel<SpaceScanEvent>,
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
            let _ = channel.send(SpaceScanEvent::Progress {
                files_scanned: stats.files.load(Ordering::Relaxed),
                bytes_scanned: stats.bytes.load(Ordering::Relaxed),
                skipped_entries: stats.skipped.load(Ordering::Relaxed),
                elapsed_ms: started.elapsed().as_millis() as u64,
            });
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_file(path: &Path, bytes: usize) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, vec![0u8; bytes]).unwrap();
    }

    fn test_context<'a>(
        policy: &'a SafetyPolicy,
        cancel: &'a AtomicBool,
        stats: &'a ScanStats,
        max_children: usize,
    ) -> ScanContext<'a> {
        ScanContext {
            policy,
            cancel,
            stats,
            max_depth: 4,
            max_children,
        }
    }

    fn find<'a>(node: &'a SpaceNode, name: &str) -> Option<&'a SpaceNode> {
        if node.name == name {
            return Some(node);
        }
        node.children.iter().find_map(|child| find(child, name))
    }

    #[test]
    fn aggregates_sizes_and_marks_files_deletable() {
        let temp = tempfile::TempDir::new().unwrap();
        let root = temp.path();
        write_file(&root.join("a.bin"), 8192);
        write_file(&root.join("sub/b.bin"), 16384);
        write_file(&root.join("Foo.app/Contents/binary"), 4096);
        write_file(&root.join(".git/HEAD"), 128);

        let policy = SafetyPolicy::new(vec![root.to_path_buf()], vec![]);
        let cancel = AtomicBool::new(false);
        let stats = ScanStats::default();
        let mut context = test_context(&policy, &cancel, &stats, 18);
        let tree = analyze_dir(root, 0, &mut context).unwrap();

        assert!(tree.size >= 28 * 1024);
        assert!(tree.file_count >= 3);
        let file = find(&tree, "a.bin").unwrap();
        assert!(file.deletable);
        assert_eq!(file.safety_tier, Some(SafetyTier::Safe));
        let sub = find(&tree, "sub").unwrap();
        assert!(sub.is_dir);
        assert!(!sub.deletable);
        let app = find(&tree, "Foo.app").unwrap();
        assert!(app.is_dir);
        assert!(!app.deletable);
        assert!(find(&tree, "HEAD").is_none());
    }

    #[test]
    fn truncates_children_and_adds_synthetic_node() {
        let temp = tempfile::TempDir::new().unwrap();
        let root = temp.path();
        for index in 0..5 {
            write_file(&root.join(format!("file-{index}.bin")), 1024 * (index + 1));
        }

        let policy = SafetyPolicy::new(vec![root.to_path_buf()], vec![]);
        let cancel = AtomicBool::new(false);
        let stats = ScanStats::default();
        let mut context = test_context(&policy, &cancel, &stats, 2);
        let tree = analyze_dir(root, 0, &mut context).unwrap();

        assert_eq!(tree.hidden_children, 3);
        assert!(tree.children.iter().any(|child| child.is_synthetic));
    }

    #[test]
    fn cancellation_stops_before_reading() {
        let temp = tempfile::TempDir::new().unwrap();
        write_file(&temp.path().join("a.bin"), 128);

        let policy = SafetyPolicy::new(vec![temp.path().to_path_buf()], vec![]);
        let cancel = AtomicBool::new(true);
        let stats = ScanStats::default();
        let mut context = test_context(&policy, &cancel, &stats, 18);

        assert!(analyze_dir(temp.path(), 0, &mut context).is_none());
    }

    #[test]
    fn space_events_serialize_with_camel_case_fields() {
        let event = SpaceScanEvent::Progress {
            files_scanned: 3,
            bytes_scanned: 4096,
            skipped_entries: 1,
            elapsed_ms: 120,
        };
        let value = serde_json::to_value(event).unwrap();
        assert_eq!(value["type"], "progress");
        assert_eq!(value["filesScanned"].as_u64(), Some(3));
        assert_eq!(value["bytesScanned"].as_u64(), Some(4096));
    }
}
