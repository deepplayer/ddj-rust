//! 日志输出汇聚点
//!
//! 默认日志直接写 stderr。守护模式下 stderr 已重定向到日志文件，
//! 但还需要把同一份日志送进内存环形缓冲 + 广播，供 `ddj attach` 实时回放。
//!
//! 这里用全局钩子（`OnceLock`）而不是泛型参数穿透 `Bootstrap`：
//! 日志是横切关注点，为一个可选的流式控制台把泛型传到每一层不值得。

use std::io::{self, Write};
use std::sync::{Arc, OnceLock};

/// 日志落点。
pub trait ConsoleSink: Send + Sync {
    /// 收到一整行（不含换行符）。
    fn line(&self, line: &str);
}

static SINK: OnceLock<Arc<dyn ConsoleSink>> = OnceLock::new();

/// 安装落点。返回是否安装成功（已安装过则返回 `false`，不覆盖）。
pub fn set_sink(sink: Arc<dyn ConsoleSink>) -> bool {
    SINK.set(sink).is_ok()
}

/// 是否已安装。
#[must_use]
pub fn has_sink() -> bool {
    SINK.get().is_some()
}

/// `tracing_subscriber` 的 `MakeWriter` 实现。
pub(crate) struct SinkWriter;

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for SinkWriter {
    type Writer = SinkWrite;

    fn make_writer(&'a self) -> Self::Writer {
        SinkWrite::default()
    }
}

/// 按行切分的写入器。
#[derive(Default)]
pub(crate) struct SinkWrite {
    buf: String,
}

impl SinkWrite {
    fn drain_lines(&mut self) {
        while let Some(idx) = self.buf.find('\n') {
            let line: String = self.buf[..idx].to_owned();
            self.buf.drain(..=idx);
            if let Some(s) = SINK.get() {
                s.line(&line);
            }
        }
    }
}

impl Write for SinkWrite {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        self.buf.push_str(&String::from_utf8_lossy(b));
        self.drain_lines();
        Ok(b.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for SinkWrite {
    fn drop(&mut self) {
        if !self.buf.is_empty() {
            if let Some(s) = SINK.get() {
                s.line(&self.buf);
            }
        }
    }
}
