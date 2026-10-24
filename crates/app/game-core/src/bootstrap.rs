//! 启动装配
//!
//! 7 个服务的 `main.rs` 都只有一行 `Bootstrap::new(...).run()`，
//! 差异全部收敛到这里：每个 `ServiceKind` 决定要起哪些模块、开哪些端口。

use crate::modules::ModuleKind;
use crate::wiring::Components;
use anyhow::{Context, Result};
use ddj_cache::RedisPool;
use ddj_config::{ConfigLoader, Environment};
use ddj_db::DbPools;
use ddj_registry::HandlerTable;
use ddj_runtime::{Lifecycle, ServiceContext};
use ddj_session::SessionManager;
use ddj_template::TemplateStore;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::watch;

/// 服务种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceKind {
    /// 平台服（10001）：登录、区服列表、充值回调。
    Platform,
    /// 游戏服（10544）：主逻辑。
    Game,
    /// 跨服（4106）：跨服活动、跨服排行。
    Cross,
    /// 大厅（20001）。
    Hall,
    /// 战斗（1544）。
    Battle,
    /// 世界（11544）。
    World,
    /// GM（1544 HTTP）。
    Gm,
}

impl ServiceKind {
    /// 全部服务，供 `ddj list` 与帮助信息枚举。
    pub const ALL: &'static [Self] = &[
        Self::Platform,
        Self::Game,
        Self::Cross,
        Self::Hall,
        Self::Battle,
        Self::World,
        Self::Gm,
    ];

    /// 由名字解析（CLI 参数用）。
    #[must_use]
    pub fn from_name(s: &str) -> Option<Self> {
        Some(match s {
            "platform" => Self::Platform,
            "game" => Self::Game,
            "cross" => Self::Cross,
            "hall" => Self::Hall,
            "battle" => Self::Battle,
            "world" => Self::World,
            "gm" => Self::Gm,
            _ => return None,
        })
    }

    /// 服务名（用于配置与日志）。
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Platform => "platform",
            Self::Game => "game",
            Self::Cross => "cross",
            Self::Hall => "hall",
            Self::Battle => "battle",
            Self::World => "world",
            Self::Gm => "gm",
        }
    }

    /// 要启动的模块清单。
    #[must_use]
    pub const fn modules(self) -> &'static [ModuleKind] {
        match self {
            Self::Platform => &[
                ModuleKind::Database,
                ModuleKind::Cache,
                ModuleKind::Templates,
                ModuleKind::HttpListener,
                ModuleKind::Metrics,
            ],
            Self::Game => &[
                ModuleKind::Database,
                ModuleKind::Cache,
                ModuleKind::Templates,
                ModuleKind::Registry,
                ModuleKind::Session,
                ModuleKind::TcpListener,
                ModuleKind::RpcListener,
                ModuleKind::Scheduler,
                ModuleKind::Metrics,
            ],
            Self::Cross => &[
                ModuleKind::Database,
                ModuleKind::Cache,
                ModuleKind::Templates,
                ModuleKind::Registry,
                ModuleKind::Session,
                ModuleKind::RpcListener,
                ModuleKind::Metrics,
            ],
            Self::Hall | Self::Battle | Self::World => &[
                ModuleKind::Database,
                ModuleKind::Cache,
                ModuleKind::Templates,
                ModuleKind::Registry,
                ModuleKind::Session,
                ModuleKind::TcpListener,
                ModuleKind::Metrics,
            ],
            Self::Gm => &[
                ModuleKind::Database,
                ModuleKind::HttpListener,
                ModuleKind::Metrics,
            ],
        }
    }

    /// 是否需要某个模块。
    #[must_use]
    pub fn needs(self, k: ModuleKind) -> bool {
        self.modules().contains(&k)
    }
}

/// 启动器。
pub struct Bootstrap {
    kind: ServiceKind,
    config_dir: String,
    env: Environment,
    external_stop: Option<watch::Receiver<bool>>,
}

/// 已装配并启动完成的实例。
///
/// 拆出这一步是为了让守护进程接管「何时退出」：
/// 前台模式等信号，守护模式等控制连接发来的 stop。
pub struct Running {
    /// 服务种类。
    pub kind: ServiceKind,
    /// 就绪时刻。
    pub started_at: Instant,
    stop_tx: watch::Sender<bool>,
    stop_rx: watch::Receiver<bool>,
    life: Lifecycle,
    order: Vec<usize>,
    sessions: Arc<SessionManager>,
    handlers: Arc<HandlerTable>,
    templates: Arc<TemplateStore>,
}

/// 运行状态快照（供 `ddj status` 输出）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Stats {
    /// 服务名。
    pub service: String,
    /// 已运行毫秒数。
    pub uptime_ms: u64,
    /// 在线会话数。
    pub online: usize,
    /// 已注册处理器数。
    pub handlers: usize,
    /// 已加载配置表数。
    pub templates: usize,
}

/// 指标来源：可以脱离 [`Running`] 存活，供心跳任务取快照。
///
/// 拆出来是因为 `wait_stop` 会独占 `Running`，而状态查询必须在它运行期间照常工作。
#[derive(Clone)]
pub struct StatsSource {
    kind: ServiceKind,
    started_at: Instant,
    sessions: Arc<SessionManager>,
    handlers: Arc<HandlerTable>,
    templates: Arc<TemplateStore>,
}

impl StatsSource {
    /// 取一份快照。
    #[must_use]
    pub fn snapshot(&self) -> Stats {
        let up = self.started_at.elapsed();
        Stats {
            service: self.kind.name().to_owned(),
            uptime_ms: u64::try_from(up.as_millis()).unwrap_or(u64::MAX),
            online: self.sessions.online(),
            handlers: self.handlers.len(),
            templates: self.templates.loaded(),
        }
    }
}

impl Running {
    /// 取出可独立运行的指标来源。
    #[must_use]
    pub fn stats_source(&self) -> StatsSource {
        StatsSource {
            kind: self.kind,
            started_at: self.started_at,
            sessions: Arc::clone(&self.sessions),
            handlers: Arc::clone(&self.handlers),
            templates: Arc::clone(&self.templates),
        }
    }

    /// 停止信号发送端（守护模式的控制器持有）。
    #[must_use]
    pub fn stop_handle(&self) -> watch::Sender<bool> {
        self.stop_tx.clone()
    }

    /// 运行状态。
    #[must_use]
    pub fn stats(&self) -> Stats {
        self.stats_source().snapshot()
    }

    /// 阻塞到「外部要求停止」或收到 SIGINT/SIGTERM。
    pub async fn wait_stop(&mut self) {
        let mut rx = self.stop_rx.clone();
        tokio::select! {
            _ = wait_signal() => tracing::info!("收到进程信号"),
            res = rx.wait_for(|v| *v) => {
                if res.is_err() {
                    tracing::warn!("停止信号通道断开，按信号等待");
                    wait_signal().await;
                } else {
                    tracing::info!("收到控制连接停止指令");
                }
            }
        }
    }

    /// 优雅停止：先广播停止，再按拓扑逆序关模块。
    pub async fn shutdown(mut self) {
        let _ = self.stop_tx.send(true);
        tracing::info!("服务停止中");
        self.life.stop_all(&self.order).await;
        tracing::info!("服务已停止");
    }
}

/// 等待 Ctrl-C 或 SIGTERM。
async fn wait_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        // 信号注册失败意味着进程无法优雅退出，属于启动期致命错误。
        #[allow(clippy::expect_used)]
        let mut term = signal(SignalKind::terminate()).expect("注册 SIGTERM 失败");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => tracing::info!("收到 SIGINT"),
            _ = term.recv() => tracing::info!("收到 SIGTERM"),
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

impl Bootstrap {
    /// 新建。
    #[must_use]
    pub fn new(kind: ServiceKind) -> Self {
        Self {
            kind,
            config_dir: "config".into(),
            env: Environment::from_env(),
            external_stop: None,
        }
    }

    /// 指定配置目录。
    #[must_use]
    pub fn with_config_dir(mut self, dir: impl Into<String>) -> Self {
        self.config_dir = dir.into();
        self
    }

    /// 指定环境。
    #[must_use]
    pub fn with_env(mut self, env: Environment) -> Self {
        self.env = env;
        self
    }

    /// 交给外部控制停止（守护模式）。
    ///
    /// 未设置时 `wait_stop` 只等进程信号；设置后**两者任一**都能触发退出，
    /// 这样 `kill -TERM` 与 `ddj stop` 都能正常收尾。
    #[must_use]
    pub fn with_stop(mut self, rx: watch::Receiver<bool>) -> Self {
        self.external_stop = Some(rx);
        self
    }

    /// 装配并启动全部模块，返回可控制的实例。
    ///
    /// # Errors
    /// 配置加载失败、端口占用、数据库不可达、模块启动失败。
    pub async fn start(self) -> Result<Running> {
        let cfg = Arc::new(
            ConfigLoader::new(&self.config_dir, self.kind.name())
                .with_env(self.env)
                .load()
                .context("加载配置失败")?,
        );

        ddj_telemetry::init(&cfg.telemetry);

        tracing::info!(
            service = self.kind.name(),
            server_id = cfg.service.server_id,
            env = ?self.env,
            "服务启动中"
        );

        let (stop_tx, stop_rx) = watch::channel(false);

        // ---- PreInit：基础设施 ----
        let db = if self.kind.needs(ModuleKind::Database) {
            Arc::new(DbPools::connect(&cfg.db).await.context("连接数据库失败")?)
        } else {
            unreachable!("所有服务都需要数据库")
        };

        // 不需要缓存的服务（GM / 纯 HTTP）根本不连 Redis。
        // 原实现会去连一条默认地址占位，`RedisPool::connect` 没有超时，
        // 本地没有 Redis 时整个启动就卡在这里 —— 属于依赖声明与实际不符。
        let cache = if self.kind.needs(ModuleKind::Cache) {
            Some(Arc::new(
                RedisPool::connect(&cfg.redis)
                    .await
                    .context("连接 Redis 失败")?,
            ))
        } else {
            None
        };

        // ---- Init：配置表与注册表 ----
        let templates = if self.kind.needs(ModuleKind::Templates) {
            let report = ddj_template::TemplateLoader::new(&cfg.template.dir, cfg.template.strict)
                .scan()
                .map_err(anyhow::Error::msg)
                .context("扫描配置表失败")?;
            tracing::info!(
                tables = report.tables,
                rows = report.rows,
                warnings = report.warnings.len(),
                errors = report.errors.len(),
                "配置表已扫描"
            );
            Arc::new(TemplateStore::new())
        } else {
            Arc::new(TemplateStore::new())
        };

        let mut table = HandlerTable::empty();
        let account_repo = Arc::new(ddj_repo::PgPlayerRepo::new((*db).clone()));
        let lookup: Arc<dyn ddj_game_handler::login::AccountLookup> =
            Arc::new(crate::account::PgAccountLookup::new(account_repo));
        let registered = ddj_game_handler::handlers::register(
            &mut table,
            ddj_game_handler::login::ServerState::healthy(cfg.service.server_id),
            Some(lookup),
        )?;
        tracing::info!(count = registered, "业务处理器已注册");
        let handlers = Arc::new(table);
        let sessions = Arc::new(SessionManager::new());

        let comps = Components {
            cfg: Arc::clone(&cfg),
            db,
            cache,
            templates: Arc::clone(&templates),
            sessions: Arc::clone(&sessions),
            handlers: Arc::clone(&handlers),
        };

        tracing::debug!(?comps, "组件装配完成");

        // 模块工厂无参，组件与停机信号在这里显式注入（见 wiring::install 的说明）。
        crate::wiring::install(comps.clone(), stop_rx.clone());

        // GM 三个端点依赖它；判定层在 ddj-http，这里只装 IO。
        crate::gm_backend::install(&comps);

        // ---- Start / Ready：生命周期 ----
        let ctx = ServiceContext {
            service: self.kind.name(),
            server_id: cfg.service.server_id,
            clock: Arc::new(ddj_core::SystemClock),
            shutdown: stop_rx.clone(),
        };

        // 模块清单已由 ServiceKind::modules() 静态确定；
        // inventory 里注册的模块按名字过滤后交给 Lifecycle 编排。
        let wanted: Vec<&str> = self.kind.modules().iter().map(|m| m.name()).collect();
        let entries = ddj_runtime::registry::select(&wanted);
        tracing::info!(
            declared = wanted.len(),
            registered = entries.len(),
            "模块清单"
        );

        let mut life = Lifecycle::from_entries(&entries);
        life.start_all(&ctx).await.context("模块启动失败")?;
        let order = life.topological().context("模块依赖成环")?;

        tracing::info!(
            service = self.kind.name(),
            online = sessions.online(),
            handlers = handlers.len(),
            templates = templates.loaded(),
            "服务就绪"
        );

        let rx = self.external_stop.unwrap_or_else(|| stop_rx.clone());

        Ok(Running {
            kind: self.kind,
            started_at: Instant::now(),
            stop_tx,
            stop_rx: rx,
            life,
            order,
            sessions,
            handlers,
            templates,
        })
    }

    /// 前台启动并阻塞到退出。
    ///
    /// # Errors
    /// 同 [`Bootstrap::start`]。
    pub async fn run(self) -> Result<()> {
        let mut running = self.start().await?;
        running.wait_stop().await;
        running.shutdown().await;
        Ok(())
    }

    /// 解析监听地址，供各 listener 使用。
    ///
    /// # Errors
    /// 地址非法。
    pub fn listen_addr(&self, cfg: &ddj_config::Config) -> Result<SocketAddr> {
        // `NetCfg.tcp_listen` 已经是解析好的 `SocketAddr`，这里只做存在性检查。
        let addr = cfg.net.tcp_listen;
        if addr.port() == 0 {
            anyhow::bail!("监听端口非法: {addr}");
        }
        Ok(addr)
    }
}
