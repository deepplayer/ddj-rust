//! 从 PG `bf_activity` 装载活动模板。
//!
//! 对应 Java：`ActivityDbTemplateCache`（`bf_activity` 表，实测 11 行）。
//!
//! ## 为什么读 PG 而不是 CSV
//! 活动表是**运营配置表**，在原版里就在 `platform` 库
//! （`data/sql/platform.sql:147` 附近的 `bf_activity`），不是策划 CSV。
//! Rust 侧的 PG 种子（`crates/data/db/migrations/0003_platform_seed.sql`）
//! 已导入同样的 11 行。
//!
//! ## 实测数据特征
//! | 特征 | 值 | 影响 |
//! |---|---|---|
//! | `timeType` | **全部 = 1**（绝对时间） | `timeType = 2` 分支（按开服时间偏移）无数据可验 |
//! | 时间范围 | 全是 `2023-07-20 09:00:00 ~ 2026-07-26 23:59:59` | 「当前」已全部结束 |
//! | `switchStatus` | 10 行 = 1，`actId=100043242` = 0 | `isOpen` **不看**它，见纯逻辑模块文档 |
//!
//! ⚠「全部已结束」是**原始数据的真实状态**，不是移植 bug。
//! 在当前数据下活动族处理器都会走「结束/未开放」分支 ——
//! 这正好用来验证错误出口，验不到成功路径。
//!
//! ## 四列时间可能为 NULL
//! `NULL` → `Option::None` → 该行时间判定全 `false`（判「未开放」）。
//! 原版 `UtilsTools.getDateTime(null, ...)` 返回 `0L`，
//! 会让 `isEnd()` 在 `now > 0` 时**恒真**。这里选「缺失即不开放」是更保守的降级 ——
//! 只会让活动晚开，不会误开。已记入 `AUDIT.md` A21。

use ddj_game_handler::activity_template::{self, ActivityCache, ActivityTemplate};

/// `bf_activity` 的原始行（列名与 PG 表一致）。
///
/// ⚠ `switchStatus` 在 PG 里是 `smallint`（不是 `bool`），
/// 时间列是 `varchar(64) NOT NULL DEFAULT '0'`（不是 `NULL` 可空）——
/// 所以空值会以字符串 `"0"` 出现，`parse_datetime("0")` 返回 `None`。
type RawRow = (
    i32,            // actId
    i32,            // typeId
    String,         // name
    i32,            // timeType
    String,         // startShowTime（NOT NULL，缺省 "0"）
    String,         // endShowTime
    String,         // startTime
    String,         // endTime
    i16,            // switchStatus（smallint）
);

/// 从 PG 读 `bf_activity` 全表。
///
/// # Errors
/// 查询失败时返回错误文本。
pub async fn load(pool: &sqlx::PgPool) -> Result<ActivityCache, String> {
    // 列名在 PG 里是驼峰（迁移脚本保留了原 MySQL 命名），SQL 里必须加双引号。
    let rows: Vec<RawRow> = sqlx::query_as(
        "SELECT \"actId\", \"typeId\", name, \"timeType\", \
                \"startShowTime\", \"endShowTime\", \"startTime\", \"endTime\", \"switchStatus\" \
         FROM bf_activity ORDER BY \"actId\"",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("查 bf_activity 失败: {e}"))?;

    let parse = |v: &str| activity_template::parse_datetime(v);
    let out: Vec<ActivityTemplate> = rows
        .into_iter()
        .map(|(act_id, type_id, name, time_type, sshow, eshow, st, et, sw)| ActivityTemplate {
            act_id,
            type_id,
            name,
            time_type,
            start_show: parse(&sshow),
            end_show: parse(&eshow),
            start: parse(&st),
            end: parse(&et),
            switch_status: sw != 0,
        })
        .collect();
    Ok(ActivityCache::new(out))
}
