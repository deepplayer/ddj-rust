//! 表行结构体
//!
//! 字段名与原 MySQL 表列一一对应（`data/sql/platform.sql` + `game_*.sql`）。
//! 只放高频表；其余由 `cargo xt gen-tables` 补齐。

use chrono::{DateTime, Utc};

/// `player` 表。
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct PlayerRow {
    /// 主键。
    pub player_id: i64,
    /// 账号 ID（关联 `platform.user`）。
    pub user_id: i64,
    /// 角色名。
    pub name: String,
    /// 等级。
    pub level: i32,
    /// 经验。
    pub exp: i64,
    /// VIP 等级。
    pub vip: i32,
    /// 最后登录时间。
    pub last_login: Option<DateTime<Utc>>,
}

/// `item` 表。
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ItemRow {
    /// 主键。
    pub id: i64,
    /// 角色 ID。
    pub player_id: i64,
    /// 模板 ID（对应 CSV `item` 表的 `id`）。
    pub template_id: i32,
    /// 数量。
    pub num: i64,
}

/// `mail` 表。
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct MailRow {
    /// 主键。
    pub id: i64,
    /// 收件角色。
    pub player_id: i64,
    /// 标题。
    pub title: String,
    /// 正文。
    pub content: String,
    /// 附件（JSON）。
    pub attach: Option<String>,
    /// 是否已领。
    pub claimed: i8,
    /// 发送时间。
    pub send_time: Option<DateTime<Utc>>,
}
