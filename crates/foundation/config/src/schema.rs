//! 配置结构体
//!
//! 取代 Java 侧 63 个 XML。一份 `config/{service}.toml` 覆盖全部。

use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

/// 顶层配置。
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// 服务身份。
    #[serde(default)]
    pub service: ServiceCfg,
    /// 网络监听。
    #[serde(default)]
    pub net: NetCfg,
    /// 数据库。
    #[serde(default)]
    pub db: DbCfg,
    /// Redis。
    #[serde(default)]
    pub redis: RedisCfg,
    /// 模板表。
    #[serde(default)]
    pub template: TemplateCfg,
    /// 日志与追踪。
    #[serde(default)]
    pub telemetry: TelemetryCfg,
    /// 玩法调优。
    #[serde(default)]
    pub gameplay: GameplayCfg,
}

/// 服务身份。
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceCfg {
    /// 服务名：`platform` / `game` / `cross` / `battle` / `hall` / `world`。
    pub name: String,
    /// 区服 ID（`platform` 服为 0）。
    pub server_id: i32,
    /// 跨服组 ID（不跨服填 0）。
    pub cross_group: i32,
}

impl Default for ServiceCfg {
    fn default() -> Self {
        Self {
            name: "game".into(),
            server_id: 1,
            cross_group: 0,
        }
    }
}

/// 网络配置。
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NetCfg {
    /// 客户端 TCP 监听地址（对应原 `10544`）。
    pub tcp_listen: SocketAddr,
    /// HTTP 监听地址（对应原 Jetty `1544`）。
    pub http_listen: SocketAddr,
    /// 跨服 RPC 监听地址（对应原 `4544`）。
    pub rpc_listen: Option<SocketAddr>,
    /// 空闲断开秒数。
    pub idle_timeout_secs: u64,
    /// 单 IP 最大连接数。
    pub max_conn_per_ip: usize,
    /// 全服最大连接数。
    pub max_conn_total: usize,
}

impl Default for NetCfg {
    fn default() -> Self {
        Self {
            // 用 `from` 而不是 `"".parse().expect()`：这是编译期常量，
            // 不会 panic，也满足「生产代码禁止 expect」的约束。
            tcp_listen: SocketAddr::from(([0, 0, 0, 0], 10_544)),
            http_listen: SocketAddr::from(([127, 0, 0, 1], 1_544)),
            rpc_listen: None,
            idle_timeout_secs: 120,
            max_conn_per_ip: 8,
            max_conn_total: 20_000,
        }
    }
}

/// 数据库。
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DbCfg {
    /// 账号库（原 `platform`，277 表）。
    pub platform_url: String,
    /// 本服游戏库（原 `game_7010544`，173 表）。
    pub game_url: String,
    /// 连接池上限。
    pub max_connections: u32,
    /// 获取连接超时秒数。
    pub acquire_timeout_secs: u64,
}

impl Default for DbCfg {
    fn default() -> Self {
        Self {
            platform_url: "postgres://game:pass@127.0.0.1:5432/platform".into(),
            game_url: "postgres://game:pass@127.0.0.1:5432/game_7010544".into(),
            max_connections: 16,
            acquire_timeout_secs: 5,
        }
    }
}

/// Redis。
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RedisCfg {
    /// 连接串。
    pub url: String,
    /// key 前缀（多服共用一个 Redis 时隔离）。
    pub key_prefix: String,
    /// 分布式锁默认超时毫秒。
    pub lock_ttl_ms: u64,
}

impl Default for RedisCfg {
    fn default() -> Self {
        Self {
            url: "redis://127.0.0.1:6379".into(),
            key_prefix: "ddj".into(),
            lock_ttl_ms: 3_000,
        }
    }
}

/// 模板表。
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateCfg {
    /// CSV 目录（原 `data/apps/game_7010544/template`）。
    pub dir: String,
    /// 启动时全量加载。
    pub load_on_start: bool,
    /// 类型 token 异常时是否中断（`sting` 之类的笔误）。
    pub strict: bool,
}

impl Default for TemplateCfg {
    fn default() -> Self {
        Self {
            dir: "templates".into(),
            load_on_start: true,
            strict: false,
        }
    }
}

/// 可观测性。
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TelemetryCfg {
    /// 日志级别。
    pub log_level: String,
    /// 日志格式：`json` / `pretty`。
    pub log_format: String,
    /// 是否开 Prometheus。
    pub metrics: bool,
}

impl Default for TelemetryCfg {
    fn default() -> Self {
        Self {
            log_level: "info".into(),
            log_format: "json".into(),
            metrics: true,
        }
    }
}

/// 玩法调优参数。
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GameplayCfg {
    /// 单次道具使用数量上限（原服务端 `useLimit` 的服务端侧兜底）。
    pub item_use_limit: u32,
    /// 背包容量。
    pub bag_capacity: u32,
    /// 邮件保留天数。
    pub mail_keep_days: i64,
}

impl Default for GameplayCfg {
    fn default() -> Self {
        Self {
            item_use_limit: 999,
            bag_capacity: 500,
            mail_keep_days: 30,
        }
    }
}
