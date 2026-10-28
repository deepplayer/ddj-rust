//! 用真实 CSV 装载系统开放表。
//!
//! 数据源：`G-功能开启表-SystemOpen.csv`（实测 **100** 行数据）。
//!
//! ## 两列都是 `list;` 分隔
//! | 列 | 实测样例 | 解析结果 |
//! |---|---|---|
//! | `unlockType` | `13;4` | `[13, 4]` |
//! | `value` | `5;5` | `["5", "5"]` |
//!
//! 单值时没有分隔符（`13` / `7`），切分后照样得到一项。
//!
//! ## ⚠ 两列长度可以不相等
//! 实测所有行都相等，但 `case 18` 的语义是「遍历**整个** `value` 列表找门客 id」，
//! 与 `unlockType` 的下标无关。所以这里**不做长度校验**，
//! 长度不齐时由 [`ddj_game_handler::system_open::is_open`] 按原版语义跳过缺项。
//!
//! ## 空值
//! `value` 列可能有空串（实测没有，但表结构允许）→ 解析成空 `Vec`，
//! `is_open` 会把每个 `unlockType` 都判成「缺 value → 不命中」→ 整行不开放。
//! 与原版 `values.get(i)` 抛 `IndexOutOfBounds` 的可观测结果一致。

use std::path::Path;
use std::sync::Arc;

use ddj_game_handler::install_system_open::install;
use ddj_game_handler::system_open::{SystemOpenCache, SystemOpenTemplate};
use ddj_template::raw_table::{load_raw, RawTable};

const SYSTEM_OPEN_CSV: &str = "G-功能开启表-SystemOpen.csv";

/// 解析 `13;4` / `5;5` 形式的列表。
///
/// 空串 → 空 `Vec`；单项 → 一项。
fn split_list(s: &str) -> Vec<String> {
    if s.trim().is_empty() {
        return Vec::new();
    }
    s.split(';')
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .map(str::to_owned)
        .collect()
}

/// 解析 `unlockType` 列（`list;` → `Vec<i32>`）。
fn split_ints(s: &str) -> Vec<i32> {
    split_list(s)
        .iter()
        .filter_map(|x| x.parse().ok())
        .collect()
}

/// 从模板目录加载（该目录下应有 `csv/`）。
///
/// # Errors
/// 文件缺失或表头不足 6 行时返回错误文本。
pub fn load(template_dir: &Path) -> Result<SystemOpenCache, String> {
    let csv = template_dir.join("csv");
    let t: &RawTable = &load_raw(&csv.join(SYSTEM_OPEN_CSV))?;

    let mut rows = Vec::with_capacity(t.rows.len());
    for row in &t.rows {
        let Some(id) = t.int(row, "id") else { continue };
        rows.push(SystemOpenTemplate {
            id,
            unlock_type: t.text(row, "unlockType").map(split_ints).unwrap_or_default(),
            value: t.text(row, "value").map(split_list).unwrap_or_default(),
            name: t.text(row, "name").unwrap_or_default().to_owned(),
        });
    }
    Ok(SystemOpenCache::new(rows))
}

/// 启动时装配；失败只记日志（`is_open` 恒 `false` = 全部功能关闭）。
pub fn install_from(template_dir: &Path) {
    match load(template_dir) {
        Ok(c) => {
            tracing::info!(rows = c.len(), "系统开放表已装载");
            install(c);
        }
        Err(e) => {
            tracing::warn!(error = %e, "系统开放表装载失败，所有功能按未开放处理");
            install(SystemOpenCache::default());
        }
    }
}

/// 供 `game-core` 用的便捷入口（保持与其它 `csv_*` 模块一致的签名）。
pub fn install_dir(template_dir: &Path) {
    install_from(template_dir);
}

/// 服级事实的类型别名（避免 `game-core` 直接依赖 `game-handler` 的具体类型）。
pub type SharedServerFacts = Arc<ddj_game_handler::install_system_open::ServerFacts>;

/// 从 PG `bf_server` 读本服 `open_time` 并注入。
///
/// 对应 `GsServer.open_time`（`data/sql/platform.sql:6241` 的 `bf_server` 表）。
/// 查不到（表里没有该 `server_id`，或查询失败）时**不注入**，
/// `open_server_day` 会按 `open_time = 0` 返回 1（开服当天）——
/// 这与原版「`gsServer` 为 null 时 `getOpenServerDay` NPE」相比是更保守的降级：
/// 功能会**晚开**而不是提前开。
pub async fn install_server_facts(pool: &sqlx::PgPool, server_id: i32) {
    let row: Result<Option<i64>, sqlx::Error> =
        sqlx::query_scalar("SELECT open_time FROM bf_server WHERE server_id = $1 LIMIT 1")
            .bind(i64::from(server_id))
            .fetch_optional(pool)
            .await;
    match row {
        Ok(Some(open_time)) => {
            tracing::info!(open_time, server_id, "开服时间已装载");
            ddj_game_handler::install_system_open::install_server(
                ddj_game_handler::install_system_open::ServerFacts { open_time },
            );
        }
        Ok(None) => {
            tracing::warn!(server_id, "bf_server 里没有该区服，开服天数按第 1 天处理");
        }
        Err(e) => {
            tracing::warn!(error = %e, "查 bf_server.open_time 失败，开服天数按第 1 天处理");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tpl_dir() -> Option<PathBuf> {
        let d = std::env::var("DDJ_CSV_DIR").ok()?;
        let p = PathBuf::from(&d);
        // `DDJ_CSV_DIR` 指向 `.../template/csv`，加载器要的是 `.../template`
        Some(p.parent().map_or(p.clone(), Path::to_path_buf))
    }

    #[test]
    fn splits_multi_values() {
        assert_eq!(split_ints("13;4"), vec![13, 4]);
        assert_eq!(split_list("5;5"), vec!["5".to_owned(), "5".to_owned()]);
        assert_eq!(split_ints("7"), vec![7]);
        assert!(split_ints("").is_empty());
    }

    #[test]
    fn loads_real_table() {
        let Some(dir) = tpl_dir() else {
            eprintln!("跳过：未设置 DDJ_CSV_DIR");
            return;
        };
        let c = load(&dir).expect("加载 SystemOpen");
        assert_eq!(c.len(), 100, "实测 100 行");
        // 120 = 焚香祈福，`13 = 2` → 开服 2 天
        let t = c.get(120).expect("120 行");
        assert_eq!(t.unlock_type, vec![13]);
        assert_eq!(t.value, vec!["2".to_owned()]);
        assert_eq!(t.name, "change_word_607");
        // 27 = 产业客栈，`13;4 = 5;5`
        let t = c.get(27).expect("27 行");
        assert_eq!(t.unlock_type, vec![13, 4]);
        assert_eq!(t.value, vec!["5".to_owned(), "5".to_owned()]);
        // 404 = （日本）神话抽卡2，`18 = 1015;1005;1046;1047;1048`
        let t = c.get(404).expect("404 行");
        assert_eq!(t.unlock_type, vec![18]);
        assert_eq!(t.value.len(), 5);
    }

    #[test]
    fn real_table_rows_with_absent_cases_are_closed() {
        let Some(dir) = tpl_dir() else {
            eprintln!("跳过：未设置 DDJ_CSV_DIR");
            return;
        };
        let c = load(&dir).expect("加载 SystemOpen");
        let facts = ddj_game_handler::system_open::PlayerFacts {
            level: 999,
            vip_lv: 99,
            open_server_day: 9999,
            building_count: 999,
            ..Default::default()
        };
        // 实测 type=9 的是 108 行（子嗣-全部培养）——isOpen 没有 case 9
        let t = c.get(108).expect("108 行");
        assert_eq!(t.unlock_type, vec![9]);
        assert!(!c.is_open(108, &facts), "type 9 无 case → 永不开放");
        // 实测 type=12 的是 114 行（产业界面-关卡按钮）
        assert_eq!(c.get(114).map(|t| t.unlock_type.clone()), Some(vec![12]));
        assert!(!c.is_open(114, &facts), "type 12 无 case → 永不开放");
    }
}
