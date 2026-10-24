//! # ddj-config — 配置体系
//!
//! 取代 Java 侧 63 个 XML（`serverModuleListConfig.xml` / `gameServerConfig.xml` /
//! `redisConfig.xml` / `jetty_n.xml` / `nginxConfig.xml` / `GameReadWriteLockConfig.xml` …）。
//!
//! ## 加载顺序（后者覆盖前者）
//! 1. 内置默认值（`Default` impl）
//! 2. `config/{service}.toml`
//! 3. `config/{service}.{env}.toml`
//! 4. 环境变量 `DDJ__SECTION__KEY`
//! 5. 启动参数 `--set section.key=value`
//!
//! ## 子模块
//! - [`loader`]  文件 + 环境变量合并
//! - [`schema`]  各服务的配置结构体定义
//! - [`validate`] 启动前校验（端口范围、超时正负、必填项）

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod loader;
pub mod schema;
pub mod validate;

pub use loader::{ConfigLoader, Environment};
pub use schema::Config;
pub use validate::Validate;
