//! 服务契约
//!
//! 原 Java 侧跨服调用无接口约束：`CrossService` 的 40 多个方法靠 Hessian
//! 反射调用，签名变更不会编译失败。这里全部声明为 trait，编译期强约束。

use crate::wire::ServiceTag;
use async_trait::async_trait;

/// 一次 RPC 调用结果。
pub type RpcResult<T> = Result<T, RpcError>;

/// RPC 错误。
#[derive(Debug, thiserror::Error)]
pub enum RpcError {
    /// 目标服务不可达。
    #[error("服务不可达: {0:?}")]
    Unreachable(ServiceTag),
    /// 超时。
    #[error("RPC 超时: {0}ms")]
    Timeout(u64),
    /// 对端返回业务错误。
    #[error("对端错误: {0}")]
    Remote(String),
    /// 编解码失败。
    #[error("编解码失败: {0}")]
    Codec(String),
}

/// 可远程调用的服务。
#[async_trait]
pub trait RemoteService: Send + Sync {
    /// 本服务的标识。
    fn tag(&self) -> ServiceTag;

    /// 调用一次真实实现由各 crate 提供。
    ///
    /// # Errors
    /// 网络 / 超时 / 对端错误。
    async fn call(&self, cmd_id: i32, body: &[u8]) -> RpcResult<Vec<u8>>;
}

/// 跨服路由能力（Game → Cross）。
#[async_trait]
pub trait CrossService: Send + Sync {
    /// 报名跨服活动。
    ///
    /// # Errors
    /// 见 [`RpcError`]。
    async fn sign_up(&self, player: ddj_core::PlayerId, activity: i32) -> RpcResult<bool>;

    /// 查询跨服排行。
    ///
    /// # Errors
    /// 见 [`RpcError`]。
    async fn rank(&self, activity: i32, top: u32) -> RpcResult<Vec<(ddj_core::PlayerId, i64)>>;
}

/// 战斗服能力（Game → Battle）。
#[async_trait]
pub trait BattleService: Send + Sync {
    /// 创建一场战斗。
    ///
    /// # Errors
    /// 见 [`RpcError`]。
    async fn create(&self, player: ddj_core::PlayerId, stage: i32) -> RpcResult<u64>;

    /// 结算。
    ///
    /// # Errors
    /// 见 [`RpcError`]。
    async fn settle(&self, battle_id: u64, win: bool) -> RpcResult<()>;
}
