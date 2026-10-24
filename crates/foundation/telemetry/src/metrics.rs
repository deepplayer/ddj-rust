//! 指标
//!
//! 原系统只有日志，没有指标。这里把关键量全部暴露到 `/metrics`。

use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use std::sync::OnceLock;

static HANDLE: OnceLock<PrometheusHandle> = OnceLock::new();

/// 已定义的指标名 —— 集中在一处，避免拼写错误。
pub mod names {
    /// 当前在线连接数。
    pub const CONNECTIONS: &str = "ddj_connections";
    /// 当前在线玩家数。
    pub const ONLINE_PLAYERS: &str = "ddj_online_players";
    /// 收到的消息总数。
    pub const MSGS_IN: &str = "ddj_messages_received_total";
    /// 发出的消息总数。
    pub const MSGS_OUT: &str = "ddj_messages_sent_total";
    /// handler 处理耗时（毫秒）。
    pub const HANDLER_MS: &str = "ddj_handler_duration_ms";
    /// 被守卫拒绝的次数。
    pub const GUARD_REJECTS: &str = "ddj_guard_rejects_total";
    /// DB 查询耗时。
    pub const DB_MS: &str = "ddj_db_query_duration_ms";
}

/// 安装 Prometheus 导出器。
pub fn install() {
    let builder = PrometheusBuilder::new();
    if let Ok(h) = builder.install_recorder() {
        let _ = HANDLE.set(h);
    }
}

/// 渲染当前指标文本（`/metrics` 端点用）。
#[must_use]
pub fn render() -> String {
    HANDLE
        .get()
        .map_or_else(String::new, PrometheusHandle::render)
}
