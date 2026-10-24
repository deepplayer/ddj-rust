//! 模块清单
//!
//! 对应 Java：`serverModuleListConfig.xml`。原文件里 60 多个 `<module>` 节点，
//! 每个靠 `class` 全名反射实例化，拼错只有启动时才报错。
//!
//! 这里改为枚举 + 显式列表：`ServiceKind::modules()` 返回该服务要启动的模块，
//! 编译器保证每个名字都存在。

use ddj_runtime::Phase;

/// 服务的模块清单。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModuleKind {
    /// 数据源。
    Database,
    /// 缓存。
    Cache,
    /// 配置表。
    Templates,
    /// 会话管理。
    Session,
    /// TCP 监听。
    TcpListener,
    /// HTTP 监听。
    HttpListener,
    /// RPC 监听。
    RpcListener,
    /// 消息注册表。
    Registry,
    /// 定时任务。
    Scheduler,
    /// 在线指标。
    Metrics,
}

impl ModuleKind {
    /// 所在启动阶段。
    #[must_use]
    pub const fn phase(self) -> Phase {
        match self {
            Self::Database | Self::Cache => Phase::PreInit,
            Self::Templates | Self::Registry => Phase::Init,
            Self::Session | Self::Scheduler | Self::Metrics => Phase::Start,
            Self::TcpListener | Self::HttpListener | Self::RpcListener => Phase::Ready,
        }
    }

    /// 名称。
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Database => "database",
            Self::Cache => "cache",
            Self::Templates => "templates",
            Self::Session => "session",
            Self::TcpListener => "tcp",
            Self::HttpListener => "http",
            Self::RpcListener => "rpc",
            Self::Registry => "registry",
            Self::Scheduler => "scheduler",
            Self::Metrics => "metrics",
        }
    }

    /// 依赖的模块：必须先启动。
    #[must_use]
    pub const fn requires(self) -> &'static [Self] {
        match self {
            Self::Database | Self::Metrics => &[],
            Self::Cache => &[Self::Database],
            Self::Templates => &[Self::Database],
            Self::Registry => &[],
            Self::Session => &[],
            Self::Scheduler => &[Self::Session],
            Self::TcpListener => &[Self::Registry, Self::Session],
            Self::HttpListener => &[Self::Database],
            Self::RpcListener => &[Self::Registry],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deps_come_first() {
        for m in [
            ModuleKind::Database,
            ModuleKind::Cache,
            ModuleKind::Templates,
            ModuleKind::Registry,
            ModuleKind::Session,
            ModuleKind::Scheduler,
            ModuleKind::TcpListener,
            ModuleKind::HttpListener,
            ModuleKind::RpcListener,
            ModuleKind::Metrics,
        ] {
            for dep in m.requires() {
                assert!(
                    dep.phase() <= m.phase(),
                    "{} 依赖的 {} 阶段更晚",
                    m.name(),
                    dep.name()
                );
            }
        }
    }
}
