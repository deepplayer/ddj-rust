//! 仓储 trait
//!
//! 领域层只依赖这些 trait。单元测试用内存实现，不需要数据库。

use async_trait::async_trait;
use ddj_core::{PlayerId, Result};

use crate::entity::{ItemRow, MailRow, PlayerRow};

/// 角色仓储。
#[async_trait]
pub trait PlayerRepo: Send + Sync {
    /// 按 ID 查。
    async fn find(&self, id: PlayerId) -> Result<Option<PlayerRow>>;
    /// 保存（全字段覆盖）。
    async fn save(&self, row: &PlayerRow) -> Result<()>;

    /// 按账号找角色（对应原版 `PlayerServerModule.getPlayerByOpenId`）。
    ///
    /// `open_id` 是 `LoginReq_4.openId`，`server_id` 用来区分分服。
    async fn find_by_open_id(&self, open_id: &str, server_id: i32) -> Result<Option<PlayerRow>>;

    /// 建角色。`player_id` 由调用方生成（原版由 `PlayerServerModule` 发号）。
    async fn create(&self, row: &PlayerRow) -> Result<()>;
}

/// 道具仓储。
#[async_trait]
pub trait ItemRepo: Send + Sync {
    /// 取某角色某模板的道具数量。
    async fn count(&self, player: PlayerId, template_id: i32) -> Result<i64>;
    /// 增减（负数为扣减）。
    async fn add(&self, player: PlayerId, template_id: i32, delta: i64) -> Result<()>;
    /// 列出全部道具。
    async fn list(&self, player: PlayerId) -> Result<Vec<ItemRow>>;
}

/// 邮件仓储。
#[async_trait]
pub trait MailRepo: Send + Sync {
    /// 未领奖邮件。
    async fn unclaimed(&self, player: PlayerId) -> Result<Vec<MailRow>>;
    /// 标记已领。
    async fn mark_claimed(&self, mail_id: i64) -> Result<()>;
}

/// 公会仓储。
#[async_trait]
pub trait GuildRepo: Send + Sync {
    /// 角色所属公会。
    async fn guild_of(&self, player: PlayerId) -> Result<Option<i64>>;
}
