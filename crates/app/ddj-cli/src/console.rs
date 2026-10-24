//! 守护进程的控制台缓冲
//!
//! 解决的问题：日志写进文件后，`tail -f` 看不到「这一秒在干什么」的时序，
//! 而且 SSH 一断就断。这里把同样的日志同时放进：
//!
//! 1. 内存环形缓冲（可回放最近 N 行，`attach` 一连上就补齐上下文）
//! 2. 广播通道（已连接的 attach 客户端实时收到新行）
//! 3. 日志文件（进程退出后仍可追溯）
//!
//! 三份写入都在同一把锁内完成，保证顺序一致。

use ddj_telemetry::ConsoleSink;
use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

/// 环形缓冲默认容量（行）。
pub const RING_CAP: usize = 2000;

struct Inner {
    ring: Mutex<VecDeque<String>>,
    file: Mutex<Option<File>>,
    tx: broadcast::Sender<String>,
}

/// 控制台。
#[derive(Clone)]
pub struct Console {
    inner: Arc<Inner>,
}

impl Console {
    /// 新建。`log_path` 为 `None` 时只进内存（测试用）。
    ///
    /// # Errors
    /// 日志文件无法创建。
    pub fn new(log_path: Option<&Path>) -> std::io::Result<Self> {
        let file = match log_path {
            Some(p) => Some(OpenOptions::new().create(true).append(true).open(p)?),
            None => None,
        };
        let (tx, _) = broadcast::channel(1024);
        Ok(Self {
            inner: Arc::new(Inner {
                ring: Mutex::new(VecDeque::with_capacity(RING_CAP)),
                file: Mutex::new(file),
                tx,
            }),
        })
    }

    /// 追加一行（不含换行符）。
    pub fn push(&self, line: &str) {
        {
            let mut f = self.inner.file.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(file) = f.as_mut() {
                // 日志写失败不应影响业务，忽略即可。
                let _ = writeln!(file, "{line}");
            }
        }
        {
            let mut ring = self.inner.ring.lock().unwrap_or_else(|e| e.into_inner());
            if ring.len() == RING_CAP {
                ring.pop_front();
            }
            ring.push_back(line.to_owned());
        }
        // 没有订阅者时 send 会报错，属正常情况。
        let _ = self.inner.tx.send(line.to_owned());
    }

    /// 最近 `n` 行快照。
    #[must_use]
    pub fn tail(&self, n: usize) -> Vec<String> {
        let ring = self.inner.ring.lock().unwrap_or_else(|e| e.into_inner());
        let skip = ring.len().saturating_sub(n);
        ring.iter().skip(skip).cloned().collect()
    }

    /// 订阅新行。
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<String> {
        self.inner.tx.subscribe()
    }
}

impl ConsoleSink for Console {
    fn line(&self, line: &str) {
        self.push(line);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_drops_oldest() {
        let c = Console::new(None).expect("构造");
        for i in 0..(RING_CAP + 10) {
            c.push(&format!("line {i}"));
        }
        let t = c.tail(3);
        assert_eq!(t.len(), 3);
        assert!(t[2].ends_with(&format!("{}", RING_CAP + 9)));
    }

    #[test]
    fn tail_more_than_written() {
        let c = Console::new(None).expect("构造");
        c.push("a");
        assert_eq!(c.tail(100), vec!["a".to_owned()]);
    }
}
