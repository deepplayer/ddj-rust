//! 日志初始化

use ddj_config::schema::TelemetryCfg;
use tracing_subscriber::filter::EnvFilter;
use tracing_subscriber::fmt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// 日志格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFormat {
    /// 生产用：单行 JSON，便于采集。
    Json,
    /// 开发用：彩色多行。
    Pretty,
}

impl LogFormat {
    /// 从配置字符串解析，非法值退化为 `Json`。
    #[must_use]
    pub fn parse(s: &str) -> Self {
        match s {
            "pretty" => Self::Pretty,
            _ => Self::Json,
        }
    }
}

/// 初始化全局 subscriber。重复调用安全（第二次为空操作）。
pub fn init(cfg: &TelemetryCfg) {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&cfg.log_level));

    let registry = tracing_subscriber::registry().with(filter);
    let format = LogFormat::parse(&cfg.log_format);

    // 安装了流式落点（守护模式）→ 日志同时进内存缓冲，供 `ddj attach` 回放；
    // 未安装 → 直接写 stderr（前台模式）。
    if crate::sink::has_sink() {
        let w = crate::sink::SinkWriter;
        match format {
            LogFormat::Json => {
                let layer = fmt::layer()
                    .json()
                    .with_current_span(true)
                    .with_span_list(false)
                    .with_writer(w);
                let _ = registry.with(layer).try_init();
            }
            LogFormat::Pretty => {
                let layer = fmt::layer().with_target(true).with_writer(w);
                let _ = registry.with(layer).try_init();
            }
        }
        return;
    }

    match format {
        LogFormat::Json => {
            let layer = fmt::layer()
                .json()
                .with_current_span(true)
                .with_span_list(false)
                .with_writer(std::io::stderr);
            let _ = registry.with(layer).try_init();
        }
        LogFormat::Pretty => {
            let layer = fmt::layer().with_target(true).with_writer(std::io::stderr);
            let _ = registry.with(layer).try_init();
        }
    }
}
