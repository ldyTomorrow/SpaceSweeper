//! 扫描器的共享过滤规则。
//!
//! 这些规则同时服务于大文件、重复文件和空间分析：
//! - 不跟随符号链接；
//! - 不进入 `.git`、应用包、照片库与常见构建产物目录；
//! - 版本库根目录整棵子树跳过，避免把源代码资产当作可清理内容。

use std::path::Path;
use walkdir::DirEntry;

const MB: u64 = 1024 * 1024;
const GB: u64 = 1024 * MB;

/// 清理类扫描的目录下钻规则：隐藏目录、符号链接、仓库根、应用包和构建目录都不进入。
pub fn should_descend(entry: &DirEntry) -> bool {
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

pub fn is_package_dir(name: &str) -> bool {
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

pub fn is_build_dir(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "node_modules" | "target" | "build" | "deriveddata" | "pods" | "dist"
    )
}

pub fn is_installer(path: &Path) -> bool {
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(extension.as_str(), "dmg" | "pkg" | "zip")
}

pub fn human_size(bytes: u64) -> String {
    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else {
        format!("{} MB", bytes.max(MB) / MB)
    }
}
