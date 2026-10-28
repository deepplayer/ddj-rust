//! 用真实 CSV 实现通商的模板与配置。
//!
//! 数据源（实测）：
//! | 表 | 文件 | 行数 | 关键列 |
//! |---|---|---|---|
//! | `ForeignTraderDB` | `T-通商-ForeignTrader.csv` | 10（id `1201..=1210`） | `tradeNum` / `totalCost` / `everyCost` / `everyReward` / `dropId` |
//! | `ConfigDB` | `B-变量表-Config.csv` | 79 | `TraderTime`（id=111，秒） |
//!
//! ## `totalCost` 走字符串
//! 最大一行 `1210` 是 `4000000000000`（4e12），能进 `i64`；但
//! `totalCost * itemNum` 这种中间量在原版是 `BigInteger`，
//! 为了不改变语义，[`crate::trade::TraderRow`] 里三个金额字段都是 `String`。
//!
//! ## `dropId` 是 `list;,` 型
//! 实测 10 行都是单值（`51`…`60`），但列类型是 `list;`，
//! 所以按 `;` 切开全取（`DropUtils.rand(Player, List<Integer>)` 会逐个抽）。

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use ddj_game_handler::trade::{self, TradeTableSource, TraderRow, TRADER_TIME_DEFAULT};
use ddj_template::raw_table::{load_raw, RawTable};

const TRADER_CSV: &str = "T-通商-ForeignTrader.csv";
const CONFIG_CSV: &str = "B-变量表-Config.csv";

/// 按 `;` 切成非空段。
fn list_of(s: &str) -> Vec<&str> {
    s.split(';')
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .collect()
}

/// 通商模板集合。
#[derive(Debug)]
pub struct CsvTrade {
    rows: HashMap<i32, TraderRow>,
    max_trader_id: i32,
    /// `ConfigDBCache.TraderTime`（秒）。
    trader_time: i64,
}

impl CsvTrade {
    /// 从模板目录加载（该目录下应有 `csv/`）。
    ///
    /// # Errors
    /// 商人表缺失或表头不足 6 行时返回错误文本；配置文件缺失时用默认值。
    pub fn load(template_dir: &Path) -> Result<Self, String> {
        let csv = template_dir.join("csv");
        let t: &RawTable = &load_raw(&csv.join(TRADER_CSV))?;

        let mut rows = HashMap::new();
        let mut max_trader_id = 0;
        for row in &t.rows {
            let Some(id) = t.int(row, "id") else {
                continue;
            };
            let drop_id = t
                .text(row, "dropId")
                .map(|s| {
                    list_of(s)
                        .into_iter()
                        .filter_map(|v| v.parse().ok())
                        .collect::<Vec<i32>>()
                })
                .unwrap_or_default();
            rows.insert(
                id,
                TraderRow {
                    id,
                    name: t.text(row, "name").unwrap_or_default().to_owned(),
                    unlock_chapter: t.int(row, "unlockChapter").unwrap_or(0),
                    trade_num: t.int(row, "tradeNum").unwrap_or(0),
                    total_cost: t.text(row, "totalCost").unwrap_or("0").to_owned(),
                    every_cost: t.text(row, "everyCost").unwrap_or("0").to_owned(),
                    every_reward: t.text(row, "everyReward").unwrap_or("0").to_owned(),
                    drop_id,
                },
            );
            max_trader_id = max_trader_id.max(id);
        }

        Ok(Self {
            rows,
            max_trader_id,
            trader_time: load_trader_time(&csv),
        })
    }

    /// `ConfigDBCache.TraderTime`（秒）。
    #[must_use]
    pub const fn trader_time(&self) -> i64 {
        self.trader_time
    }

    /// 行数，启动日志用。
    #[must_use]
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 是否为空。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// 从 `B-变量表-Config.csv` 读 `TraderTime`（秒）。
fn load_trader_time(csv: &Path) -> i64 {
    let Ok(t) = load_raw(&csv.join(CONFIG_CSV)) else {
        tracing::warn!("通商：变量表缺失，TraderTime 用默认 {TRADER_TIME_DEFAULT}");
        return TRADER_TIME_DEFAULT;
    };
    for row in &t.rows {
        if t.text(row, "name") == Some("TraderTime") {
            if let Some(v) = t
                .text(row, "value")
                .and_then(|v| v.trim().parse::<i64>().ok())
            {
                return v;
            }
        }
    }
    TRADER_TIME_DEFAULT
}

impl TradeTableSource for CsvTrade {
    fn row(&self, id: i32) -> Option<&TraderRow> {
        self.rows.get(&id)
    }

    fn max_trader_id(&self) -> i32 {
        self.max_trader_id
    }

    fn trader_time(&self) -> i64 {
        self.trader_time
    }
}

/// 启动时装配。
pub fn install(template_dir: &Path) {
    match CsvTrade::load(template_dir) {
        Ok(t) => {
            tracing::info!(
                trader_rows = t.len(),
                max_trader_id = t.max_trader_id(),
                trader_time = t.trader_time(),
                "通商表已装载"
            );
            trade::install_tables(Arc::new(t));
        }
        Err(e) => {
            tracing::warn!(error = %e, "通商表装载失败，19002/19003/19004/19005 将回 2201");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn csv_dir() -> Option<PathBuf> {
        std::env::var("DDJ_CSV_DIR").ok().map(PathBuf::from)
    }

    #[test]
    fn parses_list_of() {
        assert_eq!(list_of("51"), vec!["51"]);
        assert_eq!(list_of("51;52"), vec!["51", "52"]);
        assert_eq!(list_of(""), Vec::<&str>::new());
    }

    #[test]
    fn real_csv_has_10_traders() {
        let Some(dir) = csv_dir() else {
            return;
        };
        let t = CsvTrade::load(&dir).expect("装载商人表");
        assert_eq!(t.len(), 10, "实测 10 行");
        assert_eq!(t.max_trader_id(), 1210);
        assert_eq!(t.trader_time(), 3600);
    }

    #[test]
    fn real_csv_rows_match_expected() {
        let Some(dir) = csv_dir() else {
            return;
        };
        let t = CsvTrade::load(&dir).expect("装载商人表");
        let first = t.row(1201).expect("1201");
        assert_eq!(first.name, "天竺商人");
        assert_eq!(first.trade_num, 3);
        assert_eq!(first.total_cost, "25000000");
        assert_eq!(first.every_cost, "500000");
        assert_eq!(first.every_reward, "30000");
        assert_eq!(first.drop_id, vec![51]);
        assert_eq!(first.unlock_chapter, 6);

        let last = t.row(1210).expect("1210");
        assert_eq!(last.total_cost, "4000000000000");
        assert_eq!(last.every_cost, "80000000000");
        assert_eq!(last.every_reward, "600000");
        assert_eq!(last.drop_id, vec![60]);
        assert_eq!(last.unlock_chapter, 200);
    }

    #[test]
    fn every_row_has_trade_num_3() {
        let Some(dir) = csv_dir() else {
            return;
        };
        let t = CsvTrade::load(&dir).expect("装载商人表");
        for id in 1201..=1210 {
            let row = t.row(id).unwrap_or_else(|| panic!("缺 {id}"));
            assert_eq!(row.trade_num, 3, "{id} 的 tradeNum 实测为 3");
            assert_eq!(row.drop_id.len(), 1, "{id} 的 dropId 实测单值");
        }
    }
}
