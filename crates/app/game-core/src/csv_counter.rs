//! 用真实 CSV 实现计数器恢复模板。
//!
//! 数据源：
//! - `J-计数器恢复-CounterRecovery.csv`（实测 92 行：`type=1` 77 行 / `type=2` 13 行 / `type=4` 2 行）
//! - `V-VIP表-Vip.csv`（18 级 0..17，`counterMax` 列是 `1=3;2=3;...` 的 `list;,=`）
//!
//! ## `cdTime` 是算出来的，不是列
//! `CounterRecoveryDB.dbinit()`：`this.cdTime = ((long) this.cd) * 1000;`
//! CSV 列名是 `cd`（单位**秒**）。
//!
//! ## ⚠ `type=4` 的 2 行
//! `CounterRecoverySet.refresh` 的 `switch` 只有 `case 1/2/3`，`type=4` 落到 `default`
//! 什么都不做 —— 也就是这两条计数器**永远不会自动恢复**。照抄（不补 case 4）。
//!
//! ## ⚠ `everyNum = -1` 的行
//! `type=2` 的行 `cd` 列是空的（`getCdTime()` 返回 `0 * 1000 = 0`），`everyNum = -1`。
//! 因为 `type=2` 只走 `autoRecovery`（直接补满），不碰 `cdTime`，所以没问题。
//! 但如果哪天给它们配了 `type=1`，`cdTime = 0` 会触发除零。

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use ddj_game_handler::counter::CounterTemplate;
use ddj_game_handler::install_counter::{install_templates, CounterTemplates};
use ddj_template::raw_table::{load_raw, load_raw_pk, RawTable};

const COUNTER_CSV: &str = "J-计数器恢复-CounterRecovery.csv";
const VIP_CSV: &str = "V-VIP表-Vip.csv";

/// `counterMax` 列：`1=3;2=3;4=5;...` → `templateId → max`。
fn parse_counter_max(s: &str) -> HashMap<i32, i64> {
    s.split(';')
        .filter_map(|p| {
            let (id, num) = p.split_once('=')?;
            Some((id.trim().parse().ok()?, num.trim().parse().ok()?))
        })
        .collect()
}

/// 计数器模板表 + VIP 上限表。
#[derive(Debug)]
pub struct CsvCounterTemplates {
    tpl: HashMap<i32, CounterTemplate>,
    /// `vipLv → (templateId → max)`。
    vip: HashMap<i32, HashMap<i32, i64>>,
    ids: Vec<i32>,
}

impl CsvCounterTemplates {
    /// 从模板目录加载（该目录下应有 `csv/`）。
    ///
    /// # Errors
    /// 文件缺失或表头不足 6 行时返回错误文本。
    pub fn load(template_dir: &Path) -> Result<Self, String> {
        let csv = template_dir.join("csv");
        let ct: &RawTable = &load_raw(&csv.join(COUNTER_CSV))?;
        let vip_tbl: &RawTable = &load_raw_pk(&csv.join(VIP_CSV), "level")?;

        let mut tpl = HashMap::new();
        let mut ids = Vec::new();
        for row in &ct.rows {
            let Some(id) = ct.int(row, "id") else { continue };
            // `cd` 列可能为空（type=2 的行）→ `getCdTime()` 得 0
            let cd_secs = ct.int(row, "cd").unwrap_or(0);
            let t = CounterTemplate {
                id,
                r#type: ct.int(row, "type").unwrap_or(0),
                cd_time: i64::from(cd_secs) * 1000,
                every_num: i64::from(ct.int(row, "everyNum").unwrap_or(0)),
                item_id: ct.int(row, "itemId").unwrap_or(0),
                is_limit: ct.int(row, "isLimit").unwrap_or(0),
            };
            ids.push(id);
            tpl.insert(id, t);
        }

        let mut vip = HashMap::new();
        for row in &vip_tbl.rows {
            let Some(level) = vip_tbl.int(row, "level") else {
                continue;
            };
            let map = vip_tbl
                .text(row, "counterMax")
                .map(parse_counter_max)
                .unwrap_or_default();
            vip.insert(level, map);
        }

        Ok(Self { tpl, vip, ids })
    }

    /// 模板行数（自检用）。
    #[must_use]
    pub fn len(&self) -> usize {
        self.tpl.len()
    }

    /// 是否为空。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tpl.is_empty()
    }

    /// VIP 等级数（自检用）。
    #[must_use]
    pub fn vip_levels(&self) -> usize {
        self.vip.len()
    }
}

impl CounterTemplates for CsvCounterTemplates {
    fn get(&self, id: i32) -> Option<CounterTemplate> {
        self.tpl.get(&id).copied()
    }

    fn all_ids(&self) -> Vec<i32> {
        self.ids.clone()
    }

    fn counter_max(&self, vip_lv: i32, template_id: i32) -> i64 {
        // `VipDB.getCounterMax(id)`：查不到返回 0
        self.vip
            .get(&vip_lv)
            .and_then(|m| m.get(&template_id).copied())
            .unwrap_or(0)
    }
}

/// 启动时装配；失败只记日志（计数器退化为「查不到模板」）。
pub fn install(template_dir: &Path) {
    match CsvCounterTemplates::load(template_dir) {
        Ok(t) => {
            tracing::info!(
                counter_rows = t.len(),
                vip_levels = t.vip_levels(),
                "计数器模板已装载"
            );
            install_templates(Arc::new(t));
        }
        Err(e) => {
            tracing::warn!(error = %e, "计数器模板装载失败，8001/8002 将返回空列表");
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
    fn parses_counter_max_pairs() {
        let m = parse_counter_max("1=3;2=3;30=120");
        assert_eq!(m.get(&1), Some(&3));
        assert_eq!(m.get(&30), Some(&120));
        assert_eq!(m.len(), 3);
    }

    #[test]
    fn loads_real_tables() {
        let Some(dir) = csv_dir() else {
            eprintln!("跳过：未设置 DDJ_CSV_DIR");
            return;
        };
        let tpl = dir.parent().map_or_else(|| dir.clone(), Path::to_path_buf);
        let t = CsvCounterTemplates::load(&tpl).expect("加载计数器表");
        assert_eq!(t.len(), 92, "实测 92 行");
        assert_eq!(t.vip_levels(), 18, "VIP 0..17");

        // 2 号：宠物精力，type=1, cd=1800 → cdTime=1800000, everyNum=1, isLimit=0（受限）
        let c2 = t.get(2).expect("2 号模板");
        assert_eq!(c2.r#type, 1);
        assert_eq!(c2.cd_time, 1_800_000);
        assert_eq!(c2.every_num, 1);
        assert!(c2.limit(), "isLimit=0 → 受限");

        // 30 号：宝玉体力，isLimit=1 → 不受限
        let c30 = t.get(30).expect("30 号模板");
        assert!(!c30.limit(), "isLimit=1 → 不受限");

        // 4 号：商战挑战，type=2（跨天补满），cd 列为空 → cdTime=0
        let c4 = t.get(4).expect("4 号模板");
        assert_eq!(c4.r#type, 2);
        assert_eq!(c4.cd_time, 0, "cd 列为空 → 0");

        // VIP 0 级：2 号上限 3
        assert_eq!(t.counter_max(0, 2), 3);
        assert_eq!(t.counter_max(1, 2), 4, "VIP1 是 4");
        // 查不到的模板 → 0
        assert_eq!(t.counter_max(0, 999_999), 0);
    }
}
