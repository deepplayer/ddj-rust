//! 用真实 CSV 实现 `ItemTemplates`。
//!
//! 数据源：`{template.dir}/csv/D-道具表-Item.csv` 与 `D-道具使用效果-Gift.csv`。
//!
//! 关键实测事实（见 `docs/AUDIT.md` A1'）：
//! - `ItemDB.useLimit`：2674 行里只有 1178 行填了值，其余为空 → 原版得 0 → 这类道具不能用。
//! - `GiftDB` 的消耗列名是 **`useCost`**（Java 字段叫 `useCostJson`），格式 `资源id=数量`。
//! - 1066 行礼包配置里只有 10 行填了 `useCost`。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ddj_game_handler::install_item::{install_templates, ItemTemplates};
use ddj_template::raw_table::{load_raw, RawTable};

const ITEM_CSV: &str = "D-道具表-Item.csv";
const GIFT_CSV: &str = "D-道具使用效果-Gift.csv";

/// 道具/礼包两张表。
#[derive(Debug)]
pub struct CsvItemTemplates {
    item: RawTable,
    gift: RawTable,
}

impl CsvItemTemplates {
    /// 从模板目录加载（该目录下应有 `csv/`）。
    ///
    /// # Errors
    /// 文件缺失或表头不足 6 行时返回错误文本。
    pub fn load(template_dir: &Path) -> Result<Self, String> {
        let csv = template_dir.join("csv");
        Ok(Self {
            item: load_raw(&csv.join(ITEM_CSV))?,
            gift: load_raw(&csv.join(GIFT_CSV))?,
        })
    }

    /// 道具表行数（自检用）。
    #[must_use]
    pub fn item_rows(&self) -> usize {
        self.item.len()
    }

    /// 礼包表行数（自检用）。
    #[must_use]
    pub fn gift_rows(&self) -> usize {
        self.gift.len()
    }
}

/// 解析 `5088=1` 形式（取第一条）。
fn parse_cost(s: &str) -> Option<(i32, i64)> {
    let first = s.split(';').find(|p| !p.trim().is_empty())?;
    let (id, num) = first.split_once('=')?;
    Some((id.trim().parse().ok()?, num.trim().parse().ok()?))
}

impl ItemTemplates for CsvItemTemplates {
    fn use_limit(&self, template_id: i32) -> Option<i32> {
        self.item.int_of(template_id, "useLimit")
    }

    fn item_type(&self, template_id: i32) -> Option<i32> {
        self.item.int_of(template_id, "type")
    }

    fn use_cost(&self, template_id: i32) -> Option<(i32, i64)> {
        parse_cost(self.gift.text_of(template_id, "useCost")?)
    }

    /// `compoundItem` 列的类型是 `list;`，单元格形如 `12=10`。
    ///
    /// 原版 `ItemDB.getCompoundItem()` 返回 `List<Integer>`；解析器把 `12=10`
    /// 展平成 `[12, 10]`，所以 `get(0)=12`（被消耗道具）、`get(1)=10`（每次数量）。
    /// 表里 2674 行只有 37 行非 0，其余是 `0` 或空 —— 两者都得到 `None`。
    fn compound_item(&self, template_id: i32) -> Option<(i32, i32)> {
        parse_compound(self.item.text_of(template_id, "compoundItem")?)
    }
}

/// 解析 `12=10`（`list;` 的单元格）。`0` / 空 / 缺 `=` 都返回 `None`。
fn parse_compound(s: &str) -> Option<(i32, i32)> {
    let (a, b) = s.trim().split_once('=')?;
    let id: i32 = a.trim().parse().ok()?;
    let num: i32 = b.trim().parse().ok()?;
    Some((id, num))
}

/// 启动时装配；失败只记日志不阻塞启动（模板缺失时业务走过渡默认值）。
pub fn install(template_dir: &Path) {
    match CsvItemTemplates::load(template_dir) {
        Ok(t) => {
            tracing::info!(
                item_rows = t.item_rows(),
                gift_rows = t.gift_rows(),
                "道具模板已装载"
            );
            install_templates(Arc::new(t));
        }
        Err(e) => {
            tracing::warn!(error = %e, dir = %PathBuf::from(template_dir).display(), "道具模板装载失败，业务回退到过渡默认值");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> Option<PathBuf> {
        std::env::var("DDJ_CSV_DIR").ok().map(PathBuf::from)
    }

    #[test]
    fn parses_cost_pairs() {
        assert_eq!(parse_cost("5088=1"), Some((5088, 1)));
        assert_eq!(parse_cost("5088=3;5089=2"), Some((5088, 3)));
        assert_eq!(parse_cost(""), None);
        assert_eq!(parse_cost("5088"), None);
    }

    #[test]
    fn loads_real_tables_and_reads_fields() {
        let Some(dir) = dir() else {
            eprintln!("跳过：未设置 DDJ_CSV_DIR");
            return;
        };
        // DDJ_CSV_DIR 直接指到 csv 目录时，父目录就是模板目录
        let tpl_dir = dir.parent().map_or_else(|| dir.clone(), Path::to_path_buf);
        let t = CsvItemTemplates::load(&tpl_dir).expect("加载道具/礼包表");
        assert!(t.item_rows() > 2000, "道具行数 {}", t.item_rows());
        assert!(t.gift_rows() > 900, "礼包行数 {}", t.gift_rows());
        // 5086 号礼包 `useCost = 5088=1`
        assert_eq!(t.use_cost(5086), Some((5088, 1)));
        // 绝大多数礼包无消耗
        assert_eq!(t.use_cost(1), None);
        // 道具 type 一定存在；useLimit 允许为空
        assert!(t.item_type(1).is_some());
    }
}
