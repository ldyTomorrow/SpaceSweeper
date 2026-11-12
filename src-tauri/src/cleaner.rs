//! 删除执行器：默认废纸篓，永久删除必须持有后端签发的一次性令牌。
//!
//! 安全要点：
//! - 每一项在真正调用删除 API 之前都会重新跑一遍 `SafetyPolicy`，不信任扫描阶段的结论；
//! - 永久删除不接受前端自造的"确认"布尔值：必须先调用 prepare 获得令牌，
//!   令牌与路径清单绑定、5 分钟过期、只能消费一次；
//! - 永久删除整批先校验再执行，任何一项不合法则整批拒绝，避免删到一半才发现越界；
//! - 日志逐项打印完整路径，事后可审计。

use crate::fs_util::path_size;
use crate::safety::{SafetyPolicy, SafetyTier};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

pub const PERMANENT_TOKEN_TTL: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Clone, Copy, Default, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DeleteMode {
    #[default]
    Trash,
    Permanent,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemovedEntry {
    pub path: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkippedEntry {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteReport {
    pub mode: DeleteMode,
    pub removed: Vec<RemovedEntry>,
    pub skipped: Vec<SkippedEntry>,
    pub freed_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PermanentTicket {
    pub token: String,
    pub expires_in_secs: u64,
}

#[derive(Debug)]
struct Ticket {
    paths: Vec<PathBuf>,
    issued_at: Instant,
}

/// 令牌表需要跨命令共享，放在 Tauri 的 managed state 中。
#[derive(Debug, Default)]
pub struct PermanentDeleteSession {
    tickets: Mutex<HashMap<String, Ticket>>,
}

impl PermanentDeleteSession {
    pub fn issue(&self, paths: Vec<PathBuf>) -> PermanentTicket {
        let token = Uuid::new_v4().to_string();
        let mut guard = self
            .tickets
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        // 顺手清理过期令牌，避免长期驻留。
        guard.retain(|_, ticket| ticket.issued_at.elapsed() < PERMANENT_TOKEN_TTL);
        guard.insert(
            token.clone(),
            Ticket {
                paths,
                issued_at: Instant::now(),
            },
        );

        PermanentTicket {
            token,
            expires_in_secs: PERMANENT_TOKEN_TTL.as_secs(),
        }
    }

    pub fn execute(
        &self,
        policy: &SafetyPolicy,
        token: &str,
        paths: &[PathBuf],
        allow_ask: bool,
    ) -> Result<DeleteReport, String> {
        let ticket = {
            let mut guard = self
                .tickets
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            guard.remove(token)
        }
        .ok_or_else(|| "确认令牌无效或已被使用，请重新发起删除".to_string())?;

        if ticket.issued_at.elapsed() >= PERMANENT_TOKEN_TTL {
            return Err("确认令牌已过期，请重新发起删除".to_string());
        }
        if !same_path_set(&ticket.paths, paths) {
            return Err("确认令牌与待删除清单不一致，已拒绝执行".to_string());
        }

        // 永久删除是不可逆操作：整批先校验，任何一项失败都不执行。
        let running_bundles = running_cache_bundle_ids();
        let caches_root = dirs::home_dir().map(|home| home.join("Library/Caches"));

        for path in paths {
            let tier = policy
                .validate_for_deletion(path, allow_ask)
                .map_err(|reason| format!("{}：{}", path.display(), reason))?;
            if let Some(root) = caches_root.as_deref() {
                if let Some(reason) = running_cache_guard(path, &running_bundles, root) {
                    return Err(format!("{}：{}", path.display(), reason));
                }
            }
            if tier == SafetyTier::Ask {
                if let Some(reason) = ask_tier_running_guard(path) {
                    return Err(format!("{}：{}", path.display(), reason));
                }
            }
        }

        let mut report = DeleteReport {
            mode: DeleteMode::Permanent,
            ..DeleteReport::default()
        };

        for path in paths {
            let size = path_size(path).bytes;

            let result = match path.symlink_metadata() {
                // 防御 TOCTOU：校验后目标被替换成符号链接时仍然跳过。
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    Err("目标在校验后变为符号链接，已跳过".to_string())
                }
                Ok(metadata) if metadata.is_dir() => {
                    fs::remove_dir_all(path).map_err(|error| error.to_string())
                }
                Ok(_) => fs::remove_file(path).map_err(|error| error.to_string()),
                Err(error) => Err(error.to_string()),
            };

            match result {
                Ok(()) => {
                    audit("PERMANENT_DELETE", path);
                    report.freed_bytes = report.freed_bytes.saturating_add(size);
                    report.removed.push(RemovedEntry {
                        path: path.to_string_lossy().into_owned(),
                        size,
                    });
                }
                Err(reason) => {
                    audit("PERMANENT_FAIL", path);
                    report.skipped.push(SkippedEntry {
                        path: path.to_string_lossy().into_owned(),
                        reason,
                    });
                }
            }
        }

        Ok(report)
    }
}

/// 使用 AppState 中的会话级白名单执行删除（包含用户授权的自定义目录）。
/// `trash` 在 macOS 上调用 NSFileManager.trashItemAtURL，文件会保留原始路径信息，
/// Finder 废纸篓里的「放回原处」因此可用。
pub fn move_to_trash_with_policy(
    paths: &[PathBuf],
    allow_ask: bool,
    policy: &SafetyPolicy,
) -> DeleteReport {
    move_to_trash_with(paths, allow_ask, policy, |path| match trash::delete(path) {
        Ok(()) => {
            audit("TRASH_OK", path);
            Ok(())
        }
        Err(error) => {
            audit("TRASH_FAIL", path);
            Err(error.to_string())
        }
    })
}

/// 可注入删除动作的版本，便于单元测试验证"什么路径根本不会走到删除 API"。
pub fn move_to_trash_with<F>(
    paths: &[PathBuf],
    allow_ask: bool,
    policy: &SafetyPolicy,
    mut trash_fn: F,
) -> DeleteReport
where
    F: FnMut(&Path) -> Result<(), String>,
{
    let mut report = DeleteReport {
        mode: DeleteMode::Trash,
        ..DeleteReport::default()
    };

    // lsappinfo 每次批量删除只调用一次；拿不到运行列表时降级为空集合，
    // 不影响白名单这条底线。
    let running_bundles = running_cache_bundle_ids();
    let caches_root = dirs::home_dir().map(|home| home.join("Library/Caches"));

    for path in paths {
        let tier = match policy.validate_for_deletion(path, allow_ask) {
            Ok(tier) => tier,
            Err(reason) => {
                report.skipped.push(SkippedEntry {
                    path: path.to_string_lossy().into_owned(),
                    reason: reason.to_string(),
                });
                continue;
            }
        };

        if let Some(root) = caches_root.as_deref() {
            if let Some(reason) = running_cache_guard(path, &running_bundles, root) {
                report.skipped.push(SkippedEntry {
                    path: path.to_string_lossy().into_owned(),
                    reason,
                });
                continue;
            }
        }

        if tier == SafetyTier::Ask {
            if let Some(reason) = ask_tier_running_guard(path) {
                report.skipped.push(SkippedEntry {
                    path: path.to_string_lossy().into_owned(),
                    reason,
                });
                continue;
            }
        }

        let size = path_size(path).bytes;
        match trash_fn(path) {
            Ok(()) => {
                report.freed_bytes = report.freed_bytes.saturating_add(size);
                report.removed.push(RemovedEntry {
                    path: path.to_string_lossy().into_owned(),
                    size,
                });
            }
            Err(reason) => report.skipped.push(SkippedEntry {
                path: path.to_string_lossy().into_owned(),
                reason,
            }),
        }
    }

    report
}

/// 正在运行的 App 持有 ask 档数据时跳过，避免 Docker / 模拟器数据损坏。
fn ask_tier_running_guard(path: &Path) -> Option<String> {
    let home = dirs::home_dir()?;

    let checks: [(&str, &[&str]); 3] = [
        (
            "Library/Containers/com.docker.docker",
            &["Docker", "Docker Desktop"],
        ),
        ("Library/Developer/Xcode/iOS DeviceSupport", &["Xcode"]),
        ("Library/Developer/CoreSimulator/Devices", &["Simulator"]),
    ];

    for (relative, processes) in checks {
        if path.starts_with(home.join(relative))
            && processes.iter().any(|process| process_is_running(*process))
        {
            return Some("相关应用正在运行，已跳过。请先退出应用再重试".to_string());
        }
    }

    None
}

/// `~/Library/Caches/<BundleID>` 对应的 App 正在运行时不清理该缓存，
/// 避免应用正在读写缓存时被删除。只对一级缓存目录生效，精确匹配 bundle id。
fn running_cache_guard(
    path: &Path,
    running_bundles: &HashSet<String>,
    caches_root: &Path,
) -> Option<String> {
    if path.parent() != Some(caches_root) {
        return None;
    }

    let name = path.file_name()?.to_string_lossy();
    if running_bundles.contains(name.as_ref()) {
        return Some(format!(
            "{name} 对应的应用正在运行，已跳过。请退出应用后重试"
        ));
    }

    None
}

fn running_cache_bundle_ids() -> HashSet<String> {
    let Ok(output) = Command::new("lsappinfo").arg("list").output() else {
        return HashSet::new();
    };
    if !output.status.success() {
        return HashSet::new();
    }

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let value = line
                .trim()
                .strip_prefix("bundleID=")?
                .trim()
                .trim_matches('"');
            if value.is_empty() || value == "[ NULL ]" {
                None
            } else {
                Some(value.to_string())
            }
        })
        .collect()
}

fn process_is_running(name: &str) -> bool {
    Command::new("pgrep")
        .args(["-x", name])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// 删除审计：控制台 + `~/Library/Logs/SpaceSweeper/audit.log`。
/// 单元测试不写用户主目录，避免测试产生副作用。
pub(crate) fn audit(action: &str, path: &Path) {
    eprintln!("[{action}] {}", path.display());
    if cfg!(test) {
        return;
    }

    let Some(home) = dirs::home_dir() else {
        return;
    };
    let log_dir = home.join("Library/Logs/SpaceSweeper");
    if fs::create_dir_all(&log_dir).is_err() {
        return;
    }

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    let Ok(mut file) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_dir.join("audit.log"))
    else {
        return;
    };

    use std::io::Write as _;
    let _ = writeln!(file, "{timestamp}\t{action}\t{}", path.display());
}

fn same_path_set(left: &[PathBuf], right: &[PathBuf]) -> bool {
    if left.len() != right.len() {
        return false;
    }

    let mut left: Vec<String> = left
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect();
    let mut right: Vec<String> = right
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect();
    left.sort();
    right.sort();
    left == right
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::os::unix::fs::symlink;
    use tempfile::TempDir;

    struct Fixture {
        _temp: TempDir,
        policy: SafetyPolicy,
        safe_root: PathBuf,
        ask_root: PathBuf,
    }

    fn fixture() -> Fixture {
        let temp = TempDir::new().unwrap();
        let safe_root = temp.path().join("Library/Caches");
        let ask_root = temp.path().join("Developer/Devices");
        fs::create_dir_all(&safe_root).unwrap();
        fs::create_dir_all(&ask_root).unwrap();

        let policy = SafetyPolicy::new(vec![safe_root.clone()], vec![ask_root.clone()]);
        Fixture {
            _temp: temp,
            policy,
            safe_root,
            ask_root,
        }
    }

    fn write_file(path: &Path, bytes: usize) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, vec![0u8; bytes]).unwrap();
    }

    #[test]
    fn trashes_valid_child_through_injected_trash() {
        let f = fixture();
        let file = f.safe_root.join("app/cache.bin");
        write_file(&file, 4096);

        let calls = Cell::new(0);
        let report = move_to_trash_with(&[file.clone()], false, &f.policy, |_| {
            calls.set(calls.get() + 1);
            Ok(())
        });

        assert_eq!(calls.get(), 1);
        assert_eq!(report.removed.len(), 1);
        assert!(report.skipped.is_empty());
        assert!(report.freed_bytes >= 4096);
    }

    #[test]
    fn rejects_outside_whitelist_before_calling_trash() {
        let f = fixture();
        let outside = f._temp.path().join("Documents/keep.txt");
        write_file(&outside, 128);

        let calls = Cell::new(0);
        let report = move_to_trash_with(&[outside], false, &f.policy, |_| {
            calls.set(calls.get() + 1);
            Ok(())
        });

        assert_eq!(calls.get(), 0);
        assert!(report.removed.is_empty());
        assert_eq!(report.skipped.len(), 1);
    }

    #[test]
    fn rejects_whitelist_root_itself() {
        let f = fixture();
        let calls = Cell::new(0);
        let report = move_to_trash_with(&[f.safe_root.clone()], false, &f.policy, |_| {
            calls.set(calls.get() + 1);
            Ok(())
        });

        assert_eq!(calls.get(), 0);
        assert!(report.skipped[0].reason.contains("根目录"));
    }

    #[test]
    fn ask_tier_requires_explicit_allow() {
        let f = fixture();
        let file = f.ask_root.join("device/symbols.bin");
        write_file(&file, 1024);

        let denied = move_to_trash_with(&[file.clone()], false, &f.policy, |_| Ok(()));
        assert!(denied.removed.is_empty());
        assert!(denied.skipped[0].reason.contains("确认"));

        let allowed = move_to_trash_with(&[file], true, &f.policy, |_| Ok(()));
        assert_eq!(allowed.removed.len(), 1);
    }

    #[test]
    fn never_trashes_symlink() {
        let f = fixture();
        let outside = f._temp.path().join("Documents/secret.txt");
        write_file(&outside, 128);
        let link = f.safe_root.join("link");
        symlink(&outside, &link).unwrap();

        let calls = Cell::new(0);
        let report = move_to_trash_with(&[link], false, &f.policy, |_| {
            calls.set(calls.get() + 1);
            Ok(())
        });

        assert_eq!(calls.get(), 0);
        assert!(report.removed.is_empty());
        assert!(report.skipped[0].reason.contains("符号链接"));
    }

    #[test]
    fn skips_direct_cache_of_running_bundle() {
        let caches_root = PathBuf::from("/Users/example/Library/Caches");
        let item = caches_root.join("com.apple.Safari");
        let nested = caches_root.join("Google/Chrome");

        let running: HashSet<String> = ["com.apple.Safari".to_string()].into_iter().collect();
        assert!(running_cache_guard(&item, &running, &caches_root).is_some());
        assert!(running_cache_guard(&item, &HashSet::new(), &caches_root).is_none());
        // 只对一级 bundle id 目录生效，嵌套缓存交给路径白名单兜底。
        assert!(running_cache_guard(&nested, &running, &caches_root).is_none());
    }

    #[test]
    fn permanent_delete_requires_matching_single_use_token() {
        let f = fixture();
        let first = f.safe_root.join("app/one.bin");
        let second = f.safe_root.join("app/two.bin");
        write_file(&first, 2048);
        write_file(&second, 2048);

        let session = PermanentDeleteSession::default();
        let ticket = session.issue(vec![first.clone()]);

        assert!(session
            .execute(&f.policy, "forged-token", &[first.clone()], false)
            .is_err());

        let ticket2 = session.issue(vec![first.clone()]);
        assert!(session
            .execute(&f.policy, &ticket2.token, &[second.clone()], false)
            .is_err());

        let report = session
            .execute(&f.policy, &ticket.token, &[first.clone()], false)
            .unwrap();
        assert_eq!(report.removed.len(), 1);
        assert!(!first.exists());

        // 令牌只能消费一次。
        assert!(session
            .execute(&f.policy, &ticket.token, &[first], false)
            .is_err());
    }

    #[test]
    fn permanent_delete_rejects_unsafe_batch_without_deleting_anything() {
        let f = fixture();
        let safe_file = f.safe_root.join("app/cache.bin");
        let outside = f._temp.path().join("Documents/keep.txt");
        write_file(&safe_file, 1024);
        write_file(&outside, 1024);

        let session = PermanentDeleteSession::default();
        let ticket = session.issue(vec![safe_file.clone(), outside.clone()]);

        assert!(session
            .execute(
                &f.policy,
                &ticket.token,
                &[safe_file.clone(), outside.clone()],
                false
            )
            .is_err());
        assert!(safe_file.exists());
        assert!(outside.exists());
    }
}
