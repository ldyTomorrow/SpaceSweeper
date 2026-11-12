//! B 类发现：大文件、旧文件、旧安装包。
//!
//! 一次遍历同时维护三个 Top N，避免对大目录做三遍 I/O：
//! - 大文件：按文件大小排序；
//! - 旧文件：大小 × 陈旧天数排序；
//! - 旧安装包：.dmg/.pkg/.zip，超过阈值未修改。
//!
//! 安全规则：
//! - 只扫描固定的用户目录（下载/桌面/文档/影片），不接受前端传入任意绝对路径；
//! - 不跟随符号链接；
//! - 跳过隐藏目录、`.git`、包含 `.git` 的仓库根目录、应用/照片包目录和常见构建目录；
//! - 删除仍由 cleaner.rs 做二次白名单校验，并在删除前检查祖先目录是否为 Git 工作树。

use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::cmp::{Ordering as CmpOrdering, Reverse};
use std::collections::BinaryHeap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::ipc::Channel;
use walkdir::{DirEntry, WalkDir};

const MB: u64 = 1024 * 1024;
const GB: u64 = 1024 * MB;

pub const DEFAULT_MIN_SIZE: u64 = 100 * MB;
pub const DEFAULT_OLD_DAYS: u64 = 365;
pub const DEFAULT_INSTALLER_DAYS: u64 = 180;
pub const DEFAULT_TOP_N: usize = 50;
const INSTALLER_MIN_BYTES: u64 = 50 * MB;
const CANCEL_CHECK_EVERY: u64 = 256;

pub const ROOT_NAMES: [&str; 4] = ["Downloads", "Desktop", "Documents", "Movies"];

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LargeScanOptions {
    /// 只接受 ROOT_NAMES 中的名称；为空/缺省表示全部默认目录。
    pub roots: Option<Vec<String>>,
    pub min_size_bytes: Option<u64>,
    pub old_days: Option<u64>,
    pub installer_days: Option<u64>,
    pub top_n: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LargeFileCategory {
    Big,
    Old,
    Installer,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LargeFileItem {
    pub id: String,
    pub path: String,
    pub name: String,
    pub size: u64,
    pub modified_ms: Option<u64>,
    pub age_days: u64,
    pub category: LargeFileCategory,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LargeScanSummary {
    pub roots: Vec<String>,
    pub files_scanned: u64,
    pub bytes_scanned: u64,
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
pub enum LargeScanEvent {
    Started {
        roots: Vec<String>,
        min_size_bytes: u64,
        old_days: u64,
        installer_days: u64,
        top_n: usize,
    },
    Progress {
        files_scanned: u64,
        bytes_scanned: u64,
        skipped_entries: u64,
        elapsed_ms: u64,
    },
    Finished {
        cancelled: bool,
        big: Vec<LargeFileItem>,
        old: Vec<LargeFileItem>,
        installers: Vec<LargeFileItem>,
        summary: LargeScanSummary,
    },
}

#[derive(Debug, Default)]
struct ScanCounters {
    files_scanned: AtomicU64,
    bytes_scanned: AtomicU64,
    skipped_entries: AtomicU64,
}

#[derive(Debug, Default)]
struct RootScan {
    big: Vec<LargeFileItem>,
    old: Vec<LargeFileItem>,
    installers: Vec<LargeFileItem>,
}

/// 生产入口：扫描固定用户目录，流式推送进度，返回三组 Top N。
pub fn scan_large_files(
    channel: &Channel<LargeScanEvent>,
    cancel: Arc<AtomicBool>,
    options: &LargeScanOptions,
) -> Result<LargeScanSummary, String> {
    let started = Instant::now();
    let roots = resolve_roots(options.roots.as_deref())?;
    let min_size = options.min_size_bytes.unwrap_or(DEFAULT_MIN_SIZE).max(1);
    let old_days = options.old_days.unwrap_or(DEFAULT_OLD_DAYS).max(1);
    let installer_days = options
        .installer_days
        .unwrap_or(DEFAULT_INSTALLER_DAYS)
        .max(1);
    let top_n = options.top_n.unwrap_or(DEFAULT_TOP_N).clamp(1, 500);
    let now = now_ms();

    let root_strings: Vec<String> = roots
        .iter()
        .map(|root| root.to_string_lossy().into_owned())
        .collect();

    channel
        .send(LargeScanEvent::Started {
            roots: root_strings.clone(),
            min_size_bytes: min_size,
            old_days,
            installer_days,
            top_n,
        })
        .map_err(|error| format!("无法建立扫描结果流：{error}"))?;

    let counters = Arc::new(ScanCounters::default());
    let stop_reporter = Arc::new(AtomicBool::new(false));
    let reporter = spawn_reporter(
        channel.clone(),
        stop_reporter.clone(),
        counters.clone(),
        started,
    );

    let results: Vec<RootScan> = roots
        .par_iter()
        .map(|root| {
            scan_root(
                root,
                min_size,
                old_days,
                installer_days,
                INSTALLER_MIN_BYTES,
                top_n,
                now,
                cancel.as_ref(),
                counters.as_ref(),
            )
        })
        .collect();

    stop_reporter.store(true, Ordering::Relaxed);
    let _ = reporter.join();

    let cancelled = cancel.load(Ordering::Relaxed);
    let (big, old, installers) = merge_top(results, top_n);
    let summary = LargeScanSummary {
        roots: root_strings,
        files_scanned: counters.files_scanned.load(Ordering::Relaxed),
        bytes_scanned: counters.bytes_scanned.load(Ordering::Relaxed),
        skipped_entries: counters.skipped_entries.load(Ordering::Relaxed),
        duration_ms: started.elapsed().as_millis() as u64,
        cancelled,
    };

    channel
        .send(LargeScanEvent::Finished {
            cancelled,
            big,
            old,
            installers,
            summary: summary.clone(),
        })
        .map_err(|error| format!("扫描完成但无法发送结果：{error}"))?;

    Ok(summary)
}

#[allow(clippy::too_many_arguments)]
fn scan_root(
    root: &Path,
    min_size: u64,
    old_days: u64,
    installer_days: u64,
    installer_min_bytes: u64,
    top_n: usize,
    now: u64,
    cancel: &AtomicBool,
    counters: &ScanCounters,
) -> RootScan {
    let mut big = TopN::new(top_n);
    let mut old = TopN::new(top_n);
    let mut installers = TopN::new(top_n);
    let mut visited: u64 = 0;

    let walker = WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(should_descend);

    for entry in walker {
        if cancel.load(Ordering::Relaxed) {
            break;
        }

        visited += 1;
        if visited % CANCEL_CHECK_EVERY == 0 && cancel.load(Ordering::Relaxed) {
            break;
        }

        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => {
                counters.skipped_entries.fetch_add(1, Ordering::Relaxed);
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
                counters.skipped_entries.fetch_add(1, Ordering::Relaxed);
                continue;
            }
        };

        let size = metadata.len();
        counters.files_scanned.fetch_add(1, Ordering::Relaxed);
        counters.bytes_scanned.fetch_add(size, Ordering::Relaxed);

        if size == 0 {
            continue;
        }

        let modified_ms = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis() as u64);
        let age_days = modified_ms
            .map(|modified| now.saturating_sub(modified) / 86_400_000)
            .unwrap_or(0);

        let path = entry.path();
        let path_string = path.to_string_lossy().into_owned();
        let name = entry.file_name().to_string_lossy().into_owned();

        if size >= min_size {
            let item = make_item(
                &path_string,
                &name,
                size,
                modified_ms,
                age_days,
                LargeFileCategory::Big,
                format!("大于 {} 的大文件；删除前请确认不再需要。", human_size(size)),
            );
            big.push(size as u128, item);
        }

        if size >= min_size && age_days >= old_days {
            let item = make_item(
                &path_string,
                &name,
                size,
                modified_ms,
                age_days,
                LargeFileCategory::Old,
                format!(
                    "{} 天未修改且大于 {}；可能已被遗忘，删除前请确认。",
                    age_days,
                    human_size(size)
                ),
            );
            old.push(size as u128 * age_days.max(1) as u128, item);
        }

        if size >= installer_min_bytes && age_days >= installer_days && is_installer(path) {
            let item = make_item(
                &path_string,
                &name,
                size,
                modified_ms,
                age_days,
                LargeFileCategory::Installer,
                format!("{} 天未修改的安装包；通常可以重新下载。", age_days),
            );
            installers.push(size as u128 * age_days.max(1) as u128, item);
        }
    }

    RootScan {
        big: big.into_vec(),
        old: old.into_vec(),
        installers: installers.into_vec(),
    }
}

fn make_item(
    path: &str,
    name: &str,
    size: u64,
    modified_ms: Option<u64>,
    age_days: u64,
    category: LargeFileCategory,
    reason: String,
) -> LargeFileItem {
    LargeFileItem {
        id: path.to_string(),
        path: path.to_string(),
        name: name.to_string(),
        size,
        modified_ms,
        age_days,
        category,
        reason,
    }
}

fn merge_top(
    results: Vec<RootScan>,
    top_n: usize,
) -> (Vec<LargeFileItem>, Vec<LargeFileItem>, Vec<LargeFileItem>) {
    let mut big: Vec<LargeFileItem> = results
        .iter()
        .flat_map(|result| result.big.iter().cloned())
        .collect();
    big.sort_by(|left, right| {
        right
            .size
            .cmp(&left.size)
            .then_with(|| left.path.cmp(&right.path))
    });
    big.truncate(top_n);

    let mut old: Vec<LargeFileItem> = results
        .iter()
        .flat_map(|result| result.old.iter().cloned())
        .collect();
    old.sort_by(|left, right| {
        item_score(right)
            .cmp(&item_score(left))
            .then_with(|| right.size.cmp(&left.size))
            .then_with(|| left.path.cmp(&right.path))
    });
    old.truncate(top_n);

    let mut installers: Vec<LargeFileItem> = results
        .iter()
        .flat_map(|result| result.installers.iter().cloned())
        .collect();
    installers.sort_by(|left, right| {
        item_score(right)
            .cmp(&item_score(left))
            .then_with(|| right.size.cmp(&left.size))
            .then_with(|| left.path.cmp(&right.path))
    });
    installers.truncate(top_n);

    (big, old, installers)
}

fn item_score(item: &LargeFileItem) -> u128 {
    item.size as u128 * item.age_days.max(1) as u128
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
            return Err(format!("P0 只支持默认用户目录，收到未知目录：{name}"));
        }
    }

    let mut roots = Vec::new();
    for name in names {
        let path = home.join(&name);
        let Ok(metadata) = path.symlink_metadata() else {
            continue;
        };
        // 不跟随目录级符号链接；带符号链接的下载/影片目录会被跳过。
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

#[cfg(test)]
fn default_root_paths() -> Vec<PathBuf> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    ROOT_NAMES
        .iter()
        .map(|name| home.join(name))
        .filter(|path| path.is_dir())
        .collect()
}

fn should_descend(entry: &DirEntry) -> bool {
    if entry.depth() == 0 {
        return true;
    }

    let file_type = entry.file_type();
    if file_type.is_symlink() {
        return false;
    }

    let name = entry.file_name().to_string_lossy();
    if name.starts_with('.') {
        return false;
    }

    if file_type.is_dir() {
        if is_package_dir(&name) || is_build_dir(&name) {
            return false;
        }
        // 包含 .git（目录或 worktree 文件）的目录视为仓库根，整棵子树不扫描。
        if entry.path().join(".git").symlink_metadata().is_ok() {
            return false;
        }
    }

    true
}

fn is_package_dir(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let extension = lower.rsplit_once('.').map(|(_, ext)| ext).unwrap_or("");
    matches!(
        extension,
        "app"
            | "photoslibrary"
            | "fcpbundle"
            | "imovielibrary"
            | "logicx"
            | "band"
            | "xcworkspace"
            | "xcodeproj"
    )
}

fn is_build_dir(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "node_modules" | "target" | "build" | "deriveddata" | "pods" | "dist"
    )
}

fn is_installer(path: &Path) -> bool {
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(extension.as_str(), "dmg" | "pkg" | "zip")
}

fn human_size(bytes: u64) -> String {
    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else {
        format!("{} MB", bytes.max(MB) / MB)
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn spawn_reporter(
    channel: Channel<LargeScanEvent>,
    stop: Arc<AtomicBool>,
    counters: Arc<ScanCounters>,
    started: Instant,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(150));
            if stop.load(Ordering::Relaxed) {
                break;
            }
            let _ = channel.send(LargeScanEvent::Progress {
                files_scanned: counters.files_scanned.load(Ordering::Relaxed),
                bytes_scanned: counters.bytes_scanned.load(Ordering::Relaxed),
                skipped_entries: counters.skipped_entries.load(Ordering::Relaxed),
                elapsed_ms: started.elapsed().as_millis() as u64,
            });
        }
    })
}

#[derive(Debug)]
struct RankedItem {
    score: u128,
    item: LargeFileItem,
}

impl PartialEq for RankedItem {
    fn eq(&self, other: &Self) -> bool {
        self.score == other.score
            && self.item.size == other.item.size
            && self.item.path == other.item.path
    }
}

impl Eq for RankedItem {}

impl PartialOrd for RankedItem {
    fn partial_cmp(&self, other: &Self) -> Option<CmpOrdering> {
        Some(self.cmp(other))
    }
}

impl Ord for RankedItem {
    fn cmp(&self, other: &Self) -> CmpOrdering {
        self.score
            .cmp(&other.score)
            .then_with(|| self.item.size.cmp(&other.item.size))
            .then_with(|| self.item.path.cmp(&other.item.path))
    }
}

/// 维护最多 N 个最大元素的小顶堆。
struct TopN {
    limit: usize,
    heap: BinaryHeap<Reverse<RankedItem>>,
}

impl TopN {
    fn new(limit: usize) -> Self {
        Self {
            limit: limit.max(1),
            heap: BinaryHeap::new(),
        }
    }

    fn push(&mut self, score: u128, item: LargeFileItem) {
        let ranked = RankedItem { score, item };
        if self.heap.len() < self.limit {
            self.heap.push(Reverse(ranked));
            return;
        }

        if let Some(Reverse(minimum)) = self.heap.peek() {
            if ranked > *minimum {
                self.heap.pop();
                self.heap.push(Reverse(ranked));
            }
        }
    }

    fn into_vec(self) -> Vec<LargeFileItem> {
        self.heap
            .into_iter()
            .map(|Reverse(ranked)| ranked.item)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write_file(path: &Path, bytes: usize) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, vec![0u8; bytes]).unwrap();
    }

    fn future_now() -> u64 {
        now_ms().saturating_add(400 * 86_400_000)
    }

    fn empty_counters() -> ScanCounters {
        ScanCounters::default()
    }

    #[test]
    fn scan_root_collects_three_categories_and_skips_protected_entries() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        write_file(&root.join("normal.bin"), 128);
        write_file(&root.join("old.dmg"), 128);
        write_file(&root.join(".hidden.bin"), 128);
        write_file(&root.join(".hiddendir/file.bin"), 128);
        write_file(&root.join("repo/.git/HEAD"), 16);
        write_file(&root.join("repo/large.bin"), 128);
        write_file(&root.join("Foo.app/Contents/binary"), 128);
        write_file(&root.join("node_modules/pkg/index.js"), 128);

        let cancel = AtomicBool::new(false);
        let counters = empty_counters();
        let result = scan_root(root, 1, 365, 180, 1, 50, future_now(), &cancel, &counters);

        let big_paths: Vec<_> = result.big.iter().map(|item| item.path.as_str()).collect();
        assert!(big_paths.iter().any(|path| path.ends_with("normal.bin")));
        assert!(big_paths.iter().any(|path| path.ends_with("old.dmg")));
        assert!(!big_paths.iter().any(|path| path.contains("repo")));
        assert!(!big_paths.iter().any(|path| path.contains("Foo.app")));
        assert!(!big_paths.iter().any(|path| path.contains("node_modules")));
        assert!(!big_paths.iter().any(|path| path.contains(".hidden")));

        assert!(result.old.iter().any(|item| item.name == "normal.bin"));
        assert_eq!(result.installers.len(), 1);
        assert_eq!(result.installers[0].name, "old.dmg");
        assert!(counters.files_scanned.load(Ordering::Relaxed) >= 2);
    }

    #[test]
    fn top_n_keeps_largest_files_only() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        write_file(&root.join("small.bin"), 10);
        write_file(&root.join("medium.bin"), 20);
        write_file(&root.join("large.bin"), 30);

        let cancel = AtomicBool::new(false);
        let counters = empty_counters();
        let result = scan_root(root, 1, 365, 180, 1, 2, now_ms(), &cancel, &counters);

        let sizes: Vec<u64> = result.big.iter().map(|item| item.size).collect();
        let mut sizes = sizes;
        sizes.sort_unstable_by(|left, right| right.cmp(left));
        assert_eq!(sizes, vec![30, 20]);
    }

    #[test]
    fn cancellation_stops_the_walk() {
        let temp = TempDir::new().unwrap();
        write_file(&temp.path().join("large.bin"), 128);

        let cancel = AtomicBool::new(true);
        let counters = empty_counters();
        let result = scan_root(
            temp.path(),
            1,
            365,
            180,
            1,
            50,
            now_ms(),
            &cancel,
            &counters,
        );

        assert!(result.big.is_empty());
        assert_eq!(counters.files_scanned.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn resolve_roots_rejects_unknown_names() {
        let result = resolve_roots(Some(&["NotAllowed".to_string()]));
        assert!(result.is_err());
    }

    #[test]
    fn default_root_paths_are_under_home() {
        let roots = default_root_paths();
        let home = dirs::home_dir().unwrap();
        for root in roots {
            assert!(root.starts_with(&home));
        }
    }

    #[test]
    fn large_scan_events_serialize_with_camel_case_fields() {
        let event = LargeScanEvent::Progress {
            files_scanned: 10,
            bytes_scanned: 2048,
            skipped_entries: 1,
            elapsed_ms: 300,
        };
        let value = serde_json::to_value(event).unwrap();
        assert_eq!(value["type"], "progress");
        assert_eq!(value["filesScanned"].as_u64(), Some(10));
        assert_eq!(value["bytesScanned"].as_u64(), Some(2048));
    }
}
