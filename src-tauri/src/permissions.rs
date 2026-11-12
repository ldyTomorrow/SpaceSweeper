//! 完全磁盘访问权限（FDA）探测。
//!
//! 这里刻意不做"静默失败"：UI 会在未授权时显示引导页。
//! 探测方式是尝试打开 TCC 数据库；没有 FDA 时系统返回 EPERM（PermissionDenied）。
//! 若探测文件在当前 macOS 版本上不存在，则按已授权处理，避免误伤用户体验。

use std::fs::File;
use std::io::ErrorKind;

pub fn has_full_disk_access() -> bool {
    let Some(home) = dirs::home_dir() else {
        return false;
    };

    let probes = [
        home.join("Library/Application Support/com.apple.TCC/TCC.db"),
        home.join("Library/Safari/History.db"),
    ];

    for probe in probes {
        match File::open(&probe) {
            Ok(_) => return true,
            Err(error) if error.kind() == ErrorKind::PermissionDenied => return false,
            Err(_) => continue,
        }
    }

    true
}
