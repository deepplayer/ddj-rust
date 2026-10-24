//! 真实 PostgreSQL 集成测试
//!
//! 需要外部提供一个可写的库：设 `DATABASE_URL` 才跑，否则直接跳过（CI 无 PG 时不报错）。
//!
//! 验证三件事：
//! 1. 迁移能在空库上执行（幂等，重复跑不报错）
//! 2. `ON CONFLICT (player_id, template_id)` 的累加语义正确
//! 3. `GREATEST(0, ...)` 能兜住扣成负数的情况

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
// 集成测试是独立 crate，不继承 lib 里的 `cfg_attr(test, allow(...))`，需自己声明。

use sqlx::PgPool;

async fn pool() -> Option<PgPool> {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("跳过：未设置 DATABASE_URL");
        return None;
    };
    Some(PgPool::connect(&url).await.expect("连库"))
}

#[tokio::test]
async fn migrations_are_idempotent() {
    let Some(pool) = pool().await else { return };

    ddj_db::migration::run(&pool).await.expect("首次迁移");
    ddj_db::migration::run(&pool).await.expect("二次迁移应幂等");

    let n: i64 = sqlx::query_scalar::<sqlx::Postgres, i64>(
        "SELECT count(*) FROM information_schema.tables \
         WHERE table_schema = 'public' AND table_name IN ('player','item','mail','guild')",
    )
    .fetch_one(&pool)
    .await
    .expect("查表");
    assert_eq!(n, 4, "四张核心表应全部建好");
}

#[tokio::test]
async fn item_upsert_accumulates_and_clamps() {
    let Some(pool) = pool().await else { return };
    ddj_db::migration::run(&pool).await.expect("迁移");

    let player_id: i64 = 900_000_001;
    sqlx::query::<sqlx::Postgres>("DELETE FROM item WHERE player_id = $1")
        .bind(player_id)
        .execute(&pool)
        .await
        .expect("清场");

    // 负数 delta 必须夹在 VALUES 里，否则会被 CHECK (num >= 0) 拒掉
    let upsert = "INSERT INTO item (player_id, template_id, num) VALUES ($1, $2, GREATEST(0, $3)) \
                  ON CONFLICT (player_id, template_id) \
                  DO UPDATE SET num = GREATEST(0, item.num + $3)";

    // +10 → 10
    sqlx::query::<sqlx::Postgres>(upsert)
        .bind(player_id)
        .bind(11002)
        .bind(10i64)
        .execute(&pool)
        .await
        .expect("首次插入");

    // +5 → 15
    sqlx::query::<sqlx::Postgres>(upsert)
        .bind(player_id)
        .bind(11002)
        .bind(5i64)
        .execute(&pool)
        .await
        .expect("累加");

    let num: i64 = sqlx::query_scalar::<sqlx::Postgres, i64>(
        "SELECT num FROM item WHERE player_id = $1 AND template_id = $2",
    )
    .bind(player_id)
    .bind(11002)
    .fetch_one(&pool)
    .await
    .expect("读回");
    assert_eq!(num, 15);

    // -100 → 被 GREATEST 夹到 0，不会变负
    sqlx::query::<sqlx::Postgres>(upsert)
        .bind(player_id)
        .bind(11002)
        .bind(-100i64)
        .execute(&pool)
        .await
        .expect("扣减");

    let num: i64 = sqlx::query_scalar::<sqlx::Postgres, i64>(
        "SELECT num FROM item WHERE player_id = $1 AND template_id = $2",
    )
    .bind(player_id)
    .bind(11002)
    .fetch_one(&pool)
    .await
    .expect("读回");
    assert_eq!(num, 0, "扣到负数应被夹到 0");

    sqlx::query::<sqlx::Postgres>("DELETE FROM item WHERE player_id = $1")
        .bind(player_id)
        .execute(&pool)
        .await
        .expect("收尾");
}

/// 全量平台表平移的回归测试。
///
/// `0002_platform.sql` 由 `tools/mysql2pg.py` 从 `data/sql/platform.sql` 生成，
/// 这里锁住三件事：表数、列数、种子行数。任何一项变了都要先确认是预期改动，
/// 否则说明转换器或源 DDL 出了问题。
#[tokio::test]
async fn platform_schema_is_fully_translated() {
    let Some(pool) = pool().await else { return };

    ddj_db::migration::run(&pool).await.expect("迁移");

    let tables: i64 = sqlx::query_scalar::<sqlx::Postgres, i64>(
        "SELECT count(*) FROM information_schema.tables \
         WHERE table_schema = 'public' AND table_type = 'BASE TABLE' \
           AND table_name NOT LIKE '\\_%'",
    )
    .fetch_one(&pool)
    .await
    .expect("查表数");
    // 277 张平台表 + 仓储层自用的 player/item/mail/guild（`_sqlx_migrations` 不算）
    assert_eq!(tables, 281, "表数不符：277 平台表 + 4 核心表");

    let columns: i64 = sqlx::query_scalar::<sqlx::Postgres, i64>(
        "SELECT count(*) FROM information_schema.columns WHERE table_schema = 'public'",
    )
    .fetch_one(&pool)
    .await
    .expect("查列数");
    // 与 MySQL 源库逐表逐列比对过，2409 + 四张核心表
    assert!(columns >= 2409, "列数 {columns} < 2409，平台表列缺失");

    // 种子行数：源 dump 里 37 张表有数据，合计 722 行
    let rows: i64 = sqlx::query_scalar::<sqlx::Postgres, i64>(
        "SELECT (SELECT count(*) FROM bf_activity) \
              + (SELECT count(*) FROM bf_activity_type) \
              + (SELECT count(*) FROM bf_config)",
    )
    .fetch_one(&pool)
    .await
    .expect("查种子行数");
    assert_eq!(rows, 11 + 100 + 79, "种子数据未完整导入");

    // identity 序列必须被种子数据校正过，否则新插入会和种子 id 撞车
    let next: i64 = sqlx::query_scalar::<sqlx::Postgres, i64>(
        "INSERT INTO bf_activity (\"actId\", \"typeId\", name, description) \
         VALUES (1, 1, '__seq_probe__', 'probe') RETURNING \"dbId\"",
    )
    .fetch_one(&pool)
    .await
    .expect("自增列插入");
    assert!(next > 43614, "identity 序列没校正，新 id {next} 与种子冲突");
    sqlx::query("DELETE FROM bf_activity WHERE name = '__seq_probe__'")
        .execute(&pool)
        .await
        .expect("清理探针行");
}
