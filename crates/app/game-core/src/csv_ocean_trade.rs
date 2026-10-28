//! 用真实 CSV 实现远洋贸易的模板与配置。
//!
//! 数据源（实测）：
//! | 表 | 文件 | 行数 | 关键列 |
//! |---|---|---|---|
//! | `OceanTradeDB` | `Y-远洋贸易-OceanTrade.csv` | 400 | `power` / `cost`（`BigInteger` 串）/ `reward` / `dropId` |
//! | `OceanTradeRankDB` | `Y-远洋贸易排名-OceanTradeRank.csv` | 13 | `rank`（`list;,`）/ `reward` |
//! | `ConfigDB` | `B-变量表-Config.csv` | 79 | `OceanTradeSeasonTime` / `CheckpointCostInterval` / `OceanTradeRankList*` |
//!
//! ## `cost` 列已贴 `i64` 上限
//! 第 400 行 `cost = 4680000000000000000`，而 `i64::MAX = 9223372036854775807`；
//! 换成 `i64` 尚且能装，但 `power * cost` 这个中间量一定溢出，所以
//! [`crate::ocean_trade`] 全程用 `BigInt`，这里只存原始字符串。
//!
//! ## `reward` 列格式
//! `"list;,="` → `4=5;3=300000` → `[(4, 5), (3, 300_000)]`。
//! 原版 `ResourcesJson.createResourcesJsons(List<List<String>>)` 对任一段
//! `size() != 2` 直接 `return null`（**整列为空，不是跳过**）；这里对不合法的段跳过
//! 是因为线上 400 行全部是 2 段 —— 差异写在 AUDIT A24。

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use ddj_game_handler::ocean_trade::{self, CheckpointRow, CheckpointTables, OceanTradeTables};
use ddj_template::raw_table::{load_raw, RawTable};

const CHECKPOINT_CSV: &str = "Y-远洋贸易-OceanTrade.csv";
const RANK_CSV: &str = "Y-远洋贸易排名-OceanTradeRank.csv";
const CONFIG_CSV: &str = "B-变量表-Config.csv";

/// 按 `;` 切成非空段。
fn list_of(s: &str) -> Vec<&str> {
    s.split(';').map(str::trim).filter(|x| !x.is_empty()).collect()
}

/// `list;,=` → `Vec<(i32, i64)>`：`4=5` → `(4, 5)`。
fn res_pairs(s: &str) -> Vec<(i32, i64)> {
    list_of(s)
        .into_iter()
        .filter_map(|seg| {
            let mut it = seg.split('=');
            let a = it.next()?.trim().parse().ok()?;
            let b = it.next()?.trim().parse().ok()?;
            Some((a, b))
        })
        .collect()
}

/// 远洋贸易配置（来自 `B-变量表-Config.csv`）。
#[derive(Debug, Clone, Copy)]
pub struct OceanTradeConfig {
    /// `OceanTradeSeasonTime`（id=146）：赛季时长，天。实测 `7`。
    pub season_days: i64,
    /// `CheckpointCostInterval`（id=41）的第 0 项，**原版只取 `get(0)`**。
    /// 实测 `0.2`，已放大 `10^5`（见 `ocean_trade::RATE_SCALE`）。
    pub min_rate_scaled: i64,
    /// `OceanTradeRankListDisplayQuantity`（id=181）：榜上显示人数。实测 `500`。
    pub rank_display: usize,
    /// `OceanTradeRankListActualNumber`（id=182）：实际维护的榜长度。实测 `2000`。
    pub rank_actual: usize,
}

impl Default for OceanTradeConfig {
    fn default() -> Self {
        Self {
            season_days: 7,
            min_rate_scaled: ocean_trade::MIN_RATE_SCALED,
            rank_display: 500,
            rank_actual: 2000,
        }
    }
}

/// 远洋贸易模板集合。
#[derive(Debug)]
pub struct CsvOceanTrade {
    checkpoints: CheckpointTables,
    rewards: HashMap<i32, Vec<(i32, i64)>>,
    config: OceanTradeConfig,
    /// `OceanTradeRankDB` 的原始行：`(rank, reward)`，`rank` 升序。
    rank_rewards: Vec<(i32, Vec<(i32, i64)>)>,
}

impl CsvOceanTrade {
    /// 从模板目录加载（该目录下应有 `csv/`）。
    ///
    /// # Errors
    /// 关卡表缺失或表头不足 6 行时返回错误文本；配置文件缺失时用默认值。
    pub fn load(template_dir: &Path) -> Result<Self, String> {
        let csv = template_dir.join("csv");
        let cp_t: &RawTable = &load_raw(&csv.join(CHECKPOINT_CSV))?;

        let mut rows = Vec::with_capacity(cp_t.rows.len());
        let mut rewards = HashMap::new();
        for row in &cp_t.rows {
            let Some(id) = cp_t.int(row, "id") else {
                continue;
            };
            rows.push(CheckpointRow {
                id,
                // 原版是 `String` 字段，这里原样保留，避免 `i64` 丢精度。
                power: cp_t.text(row, "power").unwrap_or("0").to_owned(),
                cost: cp_t.text(row, "cost").unwrap_or("0").to_owned(),
                drop_id: cp_t.int(row, "dropId").unwrap_or(0),
            });
            rewards.insert(
                id,
                cp_t.text(row, "reward").map(res_pairs).unwrap_or_default(),
            );
        }

        let config = load_config(&csv);
        let rank_rewards = load_rank(&csv);

        Ok(Self {
            checkpoints: CheckpointTables::new(rows),
            rewards,
            config,
            rank_rewards,
        })
    }

    /// 关卡表。
    #[must_use]
    pub const fn checkpoints(&self) -> &CheckpointTables {
        &self.checkpoints
    }

    /// 某关的奖励（`OceanTradeDB.getRewardJson()`）。
    ///
    /// 原版还会在 `dropId > 0` 时调 `DropUtils.rand(player, dropId)` 追加掉落；
    /// 那部分需要 `Player`，由 `install_ocean_trade.rs` 在运行时拼进来。
    #[must_use]
    pub fn reward(&self, checkpoint_id: i32) -> Vec<(i32, i64)> {
        self.rewards
            .get(&checkpoint_id)
            .cloned()
            .unwrap_or_default()
    }

    /// 配置。
    #[must_use]
    pub const fn config(&self) -> OceanTradeConfig {
        self.config
    }

    /// 排行榜奖励（`OceanTradeRankDB`）：`(rank, reward)` 升序。
    #[must_use]
    pub fn rank_rewards(&self) -> &[(i32, Vec<(i32, i64)>)] {
        &self.rank_rewards
    }

    /// 三张表的规模，启动日志用。
    #[must_use]
    pub fn counts(&self) -> (usize, usize, usize) {
        (
            self.checkpoints.len(),
            self.rewards.len(),
            self.rank_rewards.len(),
        )
    }
}

/// 从 `B-变量表-Config.csv` 读远洋贸易的 4 个变量。
fn load_config(csv: &Path) -> OceanTradeConfig {
    let mut cfg = OceanTradeConfig::default();
    let Ok(t) = load_raw(&csv.join(CONFIG_CSV)) else {
        tracing::warn!("远洋贸易：变量表缺失，用默认配置");
        return cfg;
    };
    for row in &t.rows {
        let Some(name) = t.text(row, "name") else {
            continue;
        };
        let Some(value) = t.text(row, "value") else {
            continue;
        };
        match name {
            "OceanTradeSeasonTime" => {
                if let Ok(v) = value.trim().parse() {
                    cfg.season_days = v;
                }
            }
            "CheckpointCostInterval" => {
                // `0.2;0.5;0.8;1.2;2` → 只取第 0 项 0.2 → 放大 10^5 = 20000
                if let Some(first) = list_of(value).first() {
                    if let Ok(v) = first.parse::<f64>() {
                        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                        {
                            cfg.min_rate_scaled =
                                (v * f64::from(10_i32).powi(ocean_trade::RATE_SCALE as i32)) as i64;
                        }
                    }
                }
            }
            "OceanTradeRankListDisplayQuantity" => {
                if let Ok(v) = value.trim().parse() {
                    cfg.rank_display = v;
                }
            }
            "OceanTradeRankListActualNumber" => {
                if let Ok(v) = value.trim().parse() {
                    cfg.rank_actual = v;
                }
            }
            _ => {}
        }
    }
    cfg
}

/// 从 `Y-远洋贸易排名-OceanTradeRank.csv` 读排行榜奖励。
fn load_rank(csv: &Path) -> Vec<(i32, Vec<(i32, i64)>)> {
    let Ok(t) = load_raw(&csv.join(RANK_CSV)) else {
        tracing::warn!("远洋贸易：排行榜奖励表缺失");
        return Vec::new();
    };
    let mut out = Vec::with_capacity(t.rows.len());
    for row in &t.rows {
        // `rank` 列是 `list;,` 型（`1;1`、`5;11;20`），取**最后一段**当排名。
        let Some(rank_cell) = t.text(row, "rank") else {
            continue;
        };
        let Some(rank) = list_of(&rank_cell).last().and_then(|v| v.parse().ok()) else {
            continue;
        };
        out.push((
            rank,
            t.text(row, "reward").map(res_pairs).unwrap_or_default(),
        ));
    }
    out.sort_by_key(|(rank, _)| *rank);
    out
}

impl OceanTradeTables for CsvOceanTrade {
    fn checkpoints(&self) -> &CheckpointTables {
        &self.checkpoints
    }

    fn reward(&self, checkpoint_id: i32) -> Vec<(i32, i64)> {
        CsvOceanTrade::reward(self, checkpoint_id)
    }

    fn drop_id(&self, checkpoint_id: i32) -> i32 {
        self.checkpoints
            .get(checkpoint_id)
            .map_or(0, |row| row.drop_id)
    }

    fn season_days(&self) -> i64 {
        self.config.season_days
    }

    fn min_rate_scaled(&self) -> i64 {
        self.config.min_rate_scaled
    }

    fn rank_display(&self) -> usize {
        self.config.rank_display
    }

    fn rank_actual(&self) -> usize {
        self.config.rank_actual
    }
}

/// 启动时装配。
pub fn install(template_dir: &Path) {
    match CsvOceanTrade::load(template_dir) {
        Ok(t) => {
            let (cp, rw, rk) = t.counts();
            tracing::info!(
                checkpoints = cp,
                rewards = rw,
                rank_rows = rk,
                season_days = t.config().season_days,
                min_rate_scaled = t.config().min_rate_scaled,
                "远洋贸易表已装载"
            );
            ocean_trade::install_tables(Arc::new(t));
        }
        Err(e) => {
            tracing::warn!(error = %e, "远洋贸易表装载失败，41001/41003 将回 3002");
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
    fn parses_res_pairs() {
        assert_eq!(res_pairs("4=5;3=300000"), vec![(4, 5), (3, 300_000)]);
        assert_eq!(res_pairs(""), Vec::<(i32, i64)>::new());
        // 不合法段被跳过（原版会整列返回 null，见 AUDIT A24）
        assert_eq!(res_pairs("4=5;bad"), vec![(4, 5)]);
    }

    #[test]
    fn real_csv_loads_400_checkpoints() {
        let Some(dir) = csv_dir() else {
            return;
        };
        let t = CsvOceanTrade::load(&dir).expect("装载关卡表");
        assert_eq!(t.checkpoints().len(), 400, "实测 400 行");
        assert_eq!(t.checkpoints().first_id(), 1);
        assert_eq!(t.checkpoints().last_id(), 400);
        // 首行与末行逐字核对
        let first = t.checkpoints().get(1).expect("关卡 1");
        assert_eq!(first.power, "2000000");
        assert_eq!(first.cost, "12250000000");
        assert_eq!(first.drop_id, 121);
        let last = t.checkpoints().get(400).expect("关卡 400");
        assert_eq!(last.power, "200067000000");
        assert_eq!(last.cost, "4680000000000000000");
        assert_eq!(last.drop_id, 128);
    }

    #[test]
    fn real_csv_rewards_match_first_row() {
        let Some(dir) = csv_dir() else {
            return;
        };
        let t = CsvOceanTrade::load(&dir).expect("装载关卡表");
        assert_eq!(t.reward(1), vec![(4, 5), (3, 300_000)]);
        assert!(t.reward(99_999).is_empty(), "不存在的关卡返回空");
    }

    #[test]
    fn real_config_reads_ocean_trade_vars() {
        let Some(dir) = csv_dir() else {
            return;
        };
        let t = CsvOceanTrade::load(&dir).expect("装载关卡表");
        let cfg = t.config();
        assert_eq!(cfg.season_days, 7, "OceanTradeSeasonTime");
        assert_eq!(cfg.min_rate_scaled, 20_000, "CheckpointCostInterval[0] = 0.2");
        assert_eq!(cfg.rank_display, 500);
        assert_eq!(cfg.rank_actual, 2000);
    }

    #[test]
    fn real_rank_csv_has_13_rows() {
        let Some(dir) = csv_dir() else {
            return;
        };
        let t = CsvOceanTrade::load(&dir).expect("装载关卡表");
        assert_eq!(t.rank_rewards().len(), 13, "实测 13 数据行");
        assert_eq!(t.rank_rewards()[0].0, 1, "按 rank 升序");
        // `rank` 列是 `list;,` 型（`1;1`、`5;11;20`），取最后一段。
        // 第 5 行实测 `5,11;20,...` ⇒ rank = 20。
        assert_eq!(t.rank_rewards()[4].0, 20, "`5;11;20` 取末段 20");
        assert!(!t.rank_rewards()[0].1.is_empty(), "第 1 名奖励非空");
    }

    #[test]
    fn all_checkpoint_costs_fit_bigint_and_ids_are_contiguous() {
        let Some(dir) = csv_dir() else {
            return;
        };
        let t = CsvOceanTrade::load(&dir).expect("装载关卡表");
        for id in 1..=400 {
            let row = t.checkpoints().get(id).unwrap_or_else(|| panic!("缺关卡 {id}"));
            assert!(!row.cost.is_empty(), "关卡 {id} cost 为空");
            assert!(!row.power.is_empty(), "关卡 {id} power 为空");
        }
    }
}
