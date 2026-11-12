//! 文件系统尺寸计算。统一使用"实际占用磁盘块"而非逻辑长度，
//! 这样用户看到的可释放空间与 Finder 的"大小"更接近（稀疏文件不会虚高）。

use std::fs::Metadata;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use walkdir::WalkDir;

#[derive(Debug, Default, Clone, Copy)]
pub struct PathSize {
    pub bytes: u64,
    pub files: u64,
    pub skipped: u64,
}

pub fn bytes_on_disk(metadata: &Metadata) -> u64 {
    let blocks = metadata.blocks();
    if blocks > 0 {
        blocks * 512
    } else {
        metadata.len()
    }
}

/// 不跟随符号链接，避免把链接指向的外部内容计入大小。
pub fn path_size(path: &Path) -> PathSize {
    let Ok(meta) = path.symlink_metadata() else {
        return PathSize {
            skipped: 1,
            ..PathSize::default()
        };
    };

    if meta.file_type().is_symlink() {
        return PathSize {
            skipped: 1,
            ..PathSize::default()
        };
    }

    if meta.is_file() {
        return PathSize {
            bytes: bytes_on_disk(&meta),
            files: 1,
            skipped: 0,
        };
    }

    let mut size = PathSize::default();
    for entry in WalkDir::new(path).follow_links(false).min_depth(1) {
        match entry {
            Ok(entry) => {
                let file_type = entry.file_type();
                if file_type.is_symlink() {
                    continue;
                }
                if file_type.is_file() {
                    match entry.metadata() {
                        Ok(metadata) => {
                            size.bytes = size.bytes.saturating_add(bytes_on_disk(&metadata));
                            size.files += 1;
                        }
                        Err(_) => size.skipped += 1,
                    }
                }
            }
            Err(_) => size.skipped += 1,
        }
    }

    size
}
