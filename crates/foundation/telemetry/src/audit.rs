//! 审计日志
//!
//! 原系统 GM 操作、充值、道具增减都没有独立留痕，出问题只能翻普通日志。
//! 这里单独一条 sink，**只追加、不可改**，且带操作者身份。

use serde::{Deserialize, Serialize};

/// 审计事件。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    /// Unix 毫秒。
    pub ts_ms: i64,
    /// 操作者（GM 账号 / 系统）。
    pub actor: String,
    /// 目标角色。
    pub target: Option<i64>,
    /// 动作：`item.grant` / `player.ban` / `pay.confirm` …
    pub action: String,
    /// 变更前。
    pub before: Option<String>,
    /// 变更后。
    pub after: Option<String>,
    /// 来源 IP。
    pub peer: Option<String>,
    /// 追踪 ID。
    pub trace_id: String,
}

/// 审计 sink。
pub trait AuditSink: Send + Sync {
    /// 写入一条。
    fn write(&self, ev: &AuditEvent);
}

/// 写 stderr 的默认实现（生产替换为文件中台 / Kafka）。
#[derive(Debug, Default)]
pub struct StderrSink;

impl AuditSink for StderrSink {
    fn write(&self, ev: &AuditEvent) {
        if let Ok(line) = serde_json::to_string(ev) {
            eprintln!("[AUDIT] {line}");
        }
    }
}
