//! GM 后端实现：把判定层（`ddj_http::gm`）与实际存储接起来
//!
//! 原版是 `data/web/gm/gmPay.php` 里的裸 SQL：
//! ```php
//! $u = queryUser($userid);                  // SELECT ... FROM user WHERE id = ?
//! $hasCdk = ...("SELECT 1 FROM user_cdk WHERE userid='$userid'");
//! sendPostRequest("http://127.0.0.1:1544" . $url, $code);   // 返回值被丢弃
//! ```
//!
//! 这里只做 IO。判定与响应文案仍在 `ddj_http::gm`，分工是刻意的：
//! 判定层不碰数据库、可单测；IO 层只负责把三个问题问清楚。
//!
//! ## 与原版的差异（有意）
//! - 原版把 `$userid` 直接拼进 SQL（注入面），这里全部绑定参数。
//! - 原版 `queryUser` 查的是 `user` 表；本项目的账号在 `player` 表，
//!   所以 `player_exists` 同时按 `player_id` 与 `user_id` 匹配。

use async_trait::async_trait;
use ddj_http::{install_gm_backend, GmBackend};
use std::sync::Arc;

use crate::wiring::Components;

/// 基于 PostgreSQL 与本地 HTTP 的 GM 后端。
pub struct PgGmBackend {
    db: Arc<ddj_db::DbPools>,
    /// 原版 `sendPostRequest` 的目标：`http://127.0.0.1:1544`。
    push_base: String,
}

impl std::fmt::Debug for PgGmBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PgGmBackend")
            .field("push_base", &self.push_base)
            .finish_non_exhaustive()
    }
}

impl PgGmBackend {
    /// 用装配好的组件构造。
    #[must_use]
    pub fn new(comps: &Components) -> Self {
        Self {
            db: Arc::clone(&comps.db),
            push_base: "http://127.0.0.1:1544".to_string(),
        }
    }
}

#[async_trait]
impl GmBackend for PgGmBackend {
    async fn player_exists(&self, userid: &str) -> Result<bool, String> {
        // 原版 `SELECT ... FROM user WHERE id = '$userid'`；本项目的账号在 player 表。
        let row: Option<(i32,)> = sqlx::query_as(
            "SELECT 1 FROM player WHERE player_id::text = $1 OR user_id::text = $1 LIMIT 1",
        )
        .bind(userid)
        .fetch_optional(&self.db.game)
        .await
        .map_err(|e| format!("查询玩家失败: {e}"))?;
        Ok(row.is_some())
    }

    async fn has_cdk(&self, userid: &str) -> Result<bool, String> {
        // 原版 `SELECT 1 FROM user_cdk WHERE userid = '$userid'`。
        let row: Option<(i32,)> =
            sqlx::query_as("SELECT 1 FROM user_cdk WHERE userid = $1 LIMIT 1")
                .bind(userid)
                .fetch_optional(&self.db.platform)
                .await
                .map_err(|e| format!("查询 CDK 记录失败: {e}"))?;
        Ok(row.is_some())
    }

    async fn push(&self, path: &str, body: &str) -> Result<(), String> {
        // 原版 `sendPostRequest("http://127.0.0.1:1544" . $url, $code)`，**返回值被丢弃**。
        // 这里做一次真正的 best-effort POST，但同样不把失败当业务失败。
        let addr = "127.0.0.1:1544";
        let mut stream = tokio::net::TcpStream::connect(addr)
            .await
            .map_err(|e| format!("连接 {addr} 失败: {e}"))?;
        let req = format!(
            "POST {path} HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        use tokio::io::AsyncWriteExt;
        stream
            .write_all(req.as_bytes())
            .await
            .map_err(|e| format!("发送失败: {e}"))?;
        Ok(())
    }
}

/// 装配 GM 后端。重复调用被忽略（`install_gm_backend` 内部是 `OnceLock`）。
pub fn install(comps: &Components) {
    install_gm_backend(Arc::new(PgGmBackend::new(comps)));
    tracing::info!(base = %"http://127.0.0.1:1544", "GM 后端已装配");
}
