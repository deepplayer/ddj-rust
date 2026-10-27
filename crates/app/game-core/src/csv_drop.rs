//! 用真实 CSV 实现掉落三表。
//!
//! 数据源（实测）：
//! | 表 | 文件 | 行数 | 关键列 |
//! |---|---|---|---|
//! | `DropOneDB` | `D-掉落1-DropOne.csv` | 407 | `extractNum` / `idTwo` / `givenDropId` |
//! | `DropTwoDB` | `D-掉落2-DropTwo.csv` | 762 | `formulaId` / `signId` / `formulaValue` / `items` / `probability` |
//! | `DropSignDB` | `D-掉落下标-DropSign.csv` | 73 | `signType` / `addType` / `resetType` |
//!
//! ## 三张表的 `list` 列格式
//! - `idTwo`：类型 `list;`，值 `101;102` → `[101, 102]`
//! - `givenDropId`：类型 `"list;,="` → `2=401;6=401` → `[(2,401), (6,401)]`
//! - `signId`：`list;` → `1;2`
//! - `formulaValue`：`list;` → `100;0;100`
//! - `items`：`"list;,="` → `241=300=300;241=120=120` → 每段 3 个数
//! - `probability`：`list;` → `200000;800000`
//!
//! ## ⚠ `items` 段内固定 3 个 `=`
//! 实测 3024 段**全部**是 `id=min=max`（3 个数，2 个等号）。
//! 解析时按 `=` 切，少于 3 段的行跳过并告警。
//!
//! ## ⚠ `givenDropId` 段内固定 1 个 `=`
//! 实测 49 段全部是 `第几次=二级id`。
//!
//! ## ⚠ `items` 段数与 `probability` 段数必须相等
//! `DropTwoDB.dbinit` 按 `items.size()` 遍历、取 `probability.get(i)`，
//! 段数不匹配会抛 `IndexOutOfBoundsException`。这里跳过并告警。

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use ddj_game_handler::drop::{self, DropItem, DropOne, DropSign, DropTwo};
use ddj_game_handler::pray::PrayTemplate;
use ddj_template::raw_table::{load_raw, RawTable};

const ONE_CSV: &str = "D-掉落1-DropOne.csv";
const TWO_CSV: &str = "D-掉落2-DropTwo.csv";
const SIGN_CSV: &str = "D-掉落下标-DropSign.csv";
const PRAY_CSV: &str = "F-焚香祈愿-Pray.csv";

/// 按 `;` 切成非空段。
fn list_of(s: &str) -> Vec<&str> {
    s.split(';').map(str::trim).filter(|x| !x.is_empty()).collect()
}

/// `list;` → `Vec<i32>`。
fn ints(s: &str) -> Vec<i32> {
    list_of(s).into_iter().filter_map(|x| x.parse().ok()).collect()
}

/// `list;,=` → `Vec<(i32, i32)>`（`givenDropId`：`2=401`）。
fn int_pairs(s: &str) -> Vec<(i32, i32)> {
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

/// `list;,=` 但数量是 `i64`：`263=1` → `(263, 1)`（Pray 的消耗/奖励列）。
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

/// `"list;,="` 的 items 段：`241=300=300` → `(241, 300, 300)`。
fn item_triples(s: &str) -> Vec<(i32, i64, i64)> {
    list_of(s)
        .into_iter()
        .filter_map(|seg| {
            let parts: Vec<&str> = seg.split('=').map(str::trim).collect();
            if parts.len() < 3 {
                return None;
            }
            Some((
                parts[0].parse().ok()?,
                parts[1].parse().ok()?,
                parts[2].parse().ok()?,
            ))
        })
        .collect()
}

/// 掉落三表。
#[derive(Debug, Default)]
pub struct CsvDropTables {
    one: HashMap<i32, DropOne>,
    two: HashMap<i32, DropTwo>,
    sign: HashMap<i32, DropSign>,
}

impl CsvDropTables {
    /// 从模板目录加载（该目录下应有 `csv/`）。
    ///
    /// # Errors
    /// 任一文件缺失或表头不足 6 行时返回错误文本。
    pub fn load(template_dir: &Path) -> Result<Self, String> {
        let csv = template_dir.join("csv");
        let one_t: &RawTable = &load_raw(&csv.join(ONE_CSV))?;
        let two_t: &RawTable = &load_raw(&csv.join(TWO_CSV))?;
        let sign_t: &RawTable = &load_raw(&csv.join(SIGN_CSV))?;

        let mut one = HashMap::new();
        for row in &one_t.rows {
            let Some(id) = one_t.int(row, "id") else {
                continue;
            };
            one.insert(
                id,
                DropOne {
                    id,
                    extract_num: one_t.int(row, "extractNum").unwrap_or(0),
                    id_two: one_t.text(row, "idTwo").map(ints).unwrap_or_default(),
                    given_drop: one_t
                        .text(row, "givenDropId")
                        .map(int_pairs)
                        .unwrap_or_default(),
                },
            );
        }

        let mut two = HashMap::new();
        let mut mismatch = 0usize;
        for row in &two_t.rows {
            let Some(id) = two_t.int(row, "id") else {
                continue;
            };
            let triples = two_t.text(row, "items").map(item_triples).unwrap_or_default();
            let probs = two_t
                .text(row, "probability")
                .map(ints)
                .unwrap_or_default();
            if triples.len() != probs.len() {
                // `DropTwoDB.dbinit` 会按 items.size() 取 probability.get(i) → 越界
                mismatch += 1;
                continue;
            }
            two.insert(
                id,
                DropTwo {
                    id,
                    formula_id: two_t.int(row, "formulaId").unwrap_or(0),
                    sign_id: two_t.text(row, "signId").map(ints).unwrap_or_default(),
                    formula_value: two_t
                        .text(row, "formulaValue")
                        .map(ints)
                        .unwrap_or_default(),
                    items: triples
                        .into_iter()
                        .zip(probs)
                        .map(|((item_id, num_min, num_max), weight)| DropItem {
                            item_id,
                            num_min,
                            num_max,
                            weight,
                        })
                        .collect(),
                },
            );
        }
        if mismatch > 0 {
            tracing::warn!(mismatch, "掉落2 有行的 items/probability 段数不一致，已跳过");
        }

        let mut sign = HashMap::new();
        for row in &sign_t.rows {
            let Some(id) = sign_t.int(row, "id") else {
                continue;
            };
            sign.insert(
                id,
                DropSign {
                    id,
                    sign_type: sign_t.int(row, "signType").unwrap_or(0),
                    add_type: sign_t.int(row, "addType").unwrap_or(0),
                    reset_type: sign_t.int(row, "resetType").unwrap_or(0),
                    time_reset: sign_t.int(row, "timeReset").unwrap_or(0),
                },
            );
        }

        Ok(Self { one, two, sign })
    }

    /// `DropOneDBCache.get(id)`。
    #[must_use]
    pub fn one(&self, id: i32) -> Option<&DropOne> {
        self.one.get(&id)
    }

    /// `DropTwoDBCache.get(id)`。
    #[must_use]
    pub fn two(&self, id: i32) -> Option<&DropTwo> {
        self.two.get(&id)
    }

    /// `DropSignDBCache.get(id)`。
    #[must_use]
    pub fn sign(&self, id: i32) -> Option<&DropSign> {
        self.sign.get(&id)
    }

    /// 三表行数（自检用）。
    #[must_use]
    pub fn counts(&self) -> (usize, usize, usize) {
        (self.one.len(), self.two.len(), self.sign.len())
    }
}

impl CsvDropTables {
    /// 加载 `F-焚香祈愿-Pray.csv`（实测 **1 行**：`day=1`）。
    ///
    /// # Errors
    /// 文件缺失或表头不足 6 行时返回错误文本。
    pub fn load_pray(template_dir: &Path) -> Result<Vec<PrayTemplate>, String> {
        let csv = template_dir.join("csv");
        let t: &RawTable = &load_raw(&csv.join(PRAY_CSV))?;
        let mut out = Vec::new();
        for row in &t.rows {
            let Some(id) = t.int(row, "id") else { continue };
            out.push(PrayTemplate {
                id,
                day: t.int(row, "day").unwrap_or(0),
                one_cost: t.text(row, "oneCost").map(res_pairs).unwrap_or_default(),
                ten_cost: t.text(row, "tenCost").map(res_pairs).unwrap_or_default(),
                luck_num: t.int(row, "luckNum").unwrap_or(0),
                every_luck_num: t.int(row, "everyLuckNum").unwrap_or(0),
                drop_id: t.int(row, "dropId").unwrap_or(0),
                core_reward: t
                    .text(row, "coreReward")
                    .map(res_pairs)
                    .unwrap_or_default(),
            });
        }
        // `PrayDBCache.init` 按 `day` 升序排序
        out.sort_by_key(|p| p.day);
        Ok(out)
    }
}

impl drop::DropTables for CsvDropTables {
    fn one(&self, id: i32) -> Option<DropOne> {
        self.one.get(&id).cloned()
    }
    fn two(&self, id: i32) -> Option<DropTwo> {
        self.two.get(&id).cloned()
    }
    fn sign(&self, id: i32) -> Option<DropSign> {
        self.sign.get(&id).copied()
    }
}

/// 装配焚香祈愿表；失败只记日志（17071 退化为 PRAY_ERROR_UN_OPEN）。
pub fn install_pray(template_dir: &Path) {
    match CsvDropTables::load_pray(template_dir) {
        Ok(list) => {
            tracing::info!(pray_rows = list.len(), "祈愿表已装载");
            ddj_game_handler::install_pray::install_templates(list);
        }
        Err(e) => {
            tracing::warn!(error = %e, "祈愿表装载失败，17071 将回 3901");
        }
    }
}

/// 启动时装配；失败只记日志（所有抽卡退化为「抽空」）。
pub fn install(template_dir: &Path) {
    match CsvDropTables::load(template_dir) {
        Ok(t) => {
            let (a, b, c) = t.counts();
            tracing::info!(drop_one = a, drop_two = b, drop_sign = c, "掉落表已装载");
            drop::install_tables(Arc::new(t));
        }
        Err(e) => {
            tracing::warn!(error = %e, "掉落表装载失败，所有抽卡退化为空");
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
    fn parses_ints() {
        assert_eq!(ints("101;102"), vec![101, 102]);
        assert_eq!(ints(""), Vec::<i32>::new());
        assert_eq!(ints("1;bad;3"), vec![1, 3]);
    }

    #[test]
    fn parses_int_pairs() {
        assert_eq!(int_pairs("2=401;6=401"), vec![(2, 401), (6, 401)]);
        assert_eq!(int_pairs(""), Vec::<(i32, i32)>::new());
    }

    #[test]
    fn parses_item_triples() {
        assert_eq!(item_triples("241=300=300"), vec![(241, 300, 300)]);
        // 少于 3 段 → 跳过
        assert_eq!(item_triples("241=300"), Vec::<(i32, i64, i64)>::new());
    }

    #[test]
    fn loads_real_tables() {
        let Some(dir) = csv_dir() else {
            eprintln!("跳过：未设置 DDJ_CSV_DIR");
            return;
        };
        let tpl = dir.parent().map_or_else(|| dir.clone(), Path::to_path_buf);
        let t = CsvDropTables::load(&tpl).expect("加载掉落三表");
        let (one, two, sign) = t.counts();
        assert_eq!(one, 407, "DropOne 实测 407 行");
        assert_eq!(two, 762, "DropTwo 实测 762 行");
        assert_eq!(sign, 73, "DropSign 实测 73 行");

        // 1 号：百宝袋，抽 1 次，候选 101/102
        let o1 = t.one(1).expect("1 号 DropOne");
        assert_eq!(o1.extract_num, 1);
        assert_eq!(o1.id_two, vec![101, 102]);
        assert!(o1.given_drop.is_empty());

        // 4 号有强制掉落：第 2 次和第 6 次出 401
        let o4 = t.one(4).expect("4 号 DropOne");
        assert_eq!(o4.given_drop, vec![(2, 401), (6, 401)]);

        // 101：百宝袋（高级），2 项，权重 200000/800000
        let d101 = t.two(101).expect("101 号 DropTwo");
        assert_eq!(d101.items.len(), 2);
        assert_eq!(d101.items[0].item_id, 241);
        assert_eq!(d101.items[0].weight, 200_000);
        assert_eq!(d101.items[1].weight, 800_000);

        // 下标 1：signType=1, addType=0, resetType=1
        let s1 = t.sign(1).expect("1 号 DropSign");
        assert_eq!(s1.sign_type, 1);
        assert_eq!(s1.add_type, 0);
        assert_eq!(s1.reset_type, 1);
    }

    #[test]
    fn loads_real_pray_table() {
        let Some(dir) = csv_dir() else {
            eprintln!("跳过：未设置 DDJ_CSV_DIR");
            return;
        };
        let tpl = dir.parent().map_or_else(|| dir.clone(), Path::to_path_buf);
        let list = CsvDropTables::load_pray(&tpl).expect("加载祈愿表");
        assert_eq!(list.len(), 1, "实测只有 1 行");
        let p = &list[0];
        assert_eq!(p.id, 1);
        assert_eq!(p.day, 1);
        assert_eq!(p.one_cost, vec![(263, 1)]);
        assert_eq!(p.ten_cost, vec![(263, 9)]);
        assert_eq!(p.luck_num, 150);
        assert_eq!(p.every_luck_num, 1);
        assert_eq!(p.drop_id, 151);
        assert_eq!(p.core_reward, vec![(262, 1)]);
    }

    #[test]
    fn weight_sum_distribution_matches_measurement() {
        // 实测：745 行正好 1000000，17 行不足
        let Some(dir) = csv_dir() else {
            eprintln!("跳过：未设置 DDJ_CSV_DIR");
            return;
        };
        let tpl = dir.parent().map_or_else(|| dir.clone(), Path::to_path_buf);
        let t = CsvDropTables::load(&tpl).expect("加载掉落三表");
        let mut full = 0;
        let mut short = 0;
        for id in 1..=1_000_000 {
            let Some(two) = t.two(id) else { continue };
            let sum: i32 = two.items.iter().map(|i| i.weight).sum();
            if sum == 1_000_000 {
                full += 1;
            } else {
                short += 1;
            }
        }
        assert_eq!(full, 745, "745 行权重和正好 1000000");
        assert_eq!(short, 17, "17 行不足（有哑元空抽）");
    }
}
