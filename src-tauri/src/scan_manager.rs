//! 扫描任务注册与取消。
//!
//! 前端生成 `scan_id`，后端注册一个 `AtomicBool` 取消令牌；扫描线程按固定间隔检查。
//! 同一时间只允许一个扫描任务，避免多个大目录扫描同时抢占磁盘 I/O。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Debug, Default)]
pub struct ScanManager {
    jobs: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

impl ScanManager {
    pub fn begin(&self, scan_id: &str) -> Result<Arc<AtomicBool>, String> {
        if scan_id.trim().is_empty() {
            return Err("扫描任务 ID 不能为空".to_string());
        }

        let mut jobs = self
            .jobs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        if !jobs.is_empty() {
            return Err("已有扫描任务正在运行，请先取消或等待完成".to_string());
        }

        let token = Arc::new(AtomicBool::new(false));
        jobs.insert(scan_id.to_string(), token.clone());
        Ok(token)
    }

    pub fn cancel(&self, scan_id: &str) -> bool {
        let jobs = self
            .jobs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        if let Some(token) = jobs.get(scan_id) {
            token.store(true, Ordering::Relaxed);
            true
        } else {
            false
        }
    }

    pub fn finish(&self, scan_id: &str) {
        let mut jobs = self
            .jobs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        jobs.remove(scan_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_one_scan_can_run_at_a_time() {
        let manager = ScanManager::default();
        let first = manager.begin("scan-1").unwrap();
        assert!(!first.load(Ordering::Relaxed));

        assert!(manager.begin("scan-2").is_err());

        manager.finish("scan-1");
        assert!(manager.begin("scan-2").is_ok());
    }

    #[test]
    fn cancel_sets_token_and_unknown_id_returns_false() {
        let manager = ScanManager::default();
        let token = manager.begin("scan-1").unwrap();

        assert!(manager.cancel("scan-1"));
        assert!(token.load(Ordering::Relaxed));
        assert!(!manager.cancel("missing"));

        manager.finish("scan-1");
        assert!(!manager.cancel("scan-1"));
    }
}
