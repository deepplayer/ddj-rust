//! 通用行表：不生成 445 个结构体，按「列名 → 列下标」取值。
//!
//! ## 为什么加这一层
//! `Table<T>` 需要为每张表生成一个 Rust 结构体（445 个），在移植期是纯开销。
//! 真实 CSV 的表头已经给出全部元信息：
//!
//! | 行 | 内容 | 例（`D-道具表-Item.csv`） |
//! |---|---|---|
//! | 1 | 实体名 | `ItemDB` |
//! | 2 | 主键标记 | `PK` |
//! | 3 | 序列化标记 | `3,0,0,...` |
//! | 4 | 类型 | `int,string,string,...` |
//! | 5 | **列名** | `id,bf_desc_name,...,useLimit,...` |
//! | 6 | 中文注释 | `道具ID,名字（策划）,...` |
//! | 7+ | 数据 | |
//!
//! 所以按列名取值即可，不需要代码生成。

use std::collections::HashMap;
use std::path::Path;

/// 表头行数（实体名、PK、标记、类型、列名、注释）。
pub const HEADER_ROWS: usize = 6;

/// 一张按列名索引的原始表。
#[derive(Debug, Clone, Default)]
pub struct RawTable {
    /// 实体名（第 1 行第 1 列，如 `ItemDB`）。
    pub entity: String,
    /// 列名（第 5 行）。
    pub columns: Vec<String>,
    /// 类型（第 4 行），保留原始文本便于诊断。
    pub types: Vec<String>,
    /// 数据行。
    pub rows: Vec<Vec<String>>,
    /// 主键索引：`id` 列的值 → 行下标。
    by_id: HashMap<i32, usize>,
}

impl RawTable {
    /// 列名 → 列下标。
    #[must_use]
    pub fn col(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|c| c == name)
    }

    /// 按主键取一行。
    #[must_use]
    pub fn get(&self, id: i32) -> Option<&Vec<String>> {
        self.by_id.get(&id).map(|&i| &self.rows[i])
    }

    /// 行数。
    #[must_use]
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 是否为空。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// 取整数列，缺失或解析失败时返回 `None`（原版模板解析失败会抛异常，
    /// 这里交给调用方决定降级策略）。
    #[must_use]
    pub fn int(&self, row: &[String], name: &str) -> Option<i32> {
        let i = self.col(name)?;
        row.get(i)?.trim().parse().ok()
    }

    /// 取字符串列。
    #[must_use]
    pub fn text<'a>(&self, row: &'a [String], name: &str) -> Option<&'a str> {
        let i = self.col(name)?;
        row.get(i).map(|s| s.trim())
    }

    /// 按主键取整数列。
    #[must_use]
    pub fn int_of(&self, id: i32, name: &str) -> Option<i32> {
        self.int(self.get(id)?, name)
    }

    /// 按主键取字符串列。
    #[must_use]
    pub fn text_of<'a>(&'a self, id: i32, name: &str) -> Option<&'a str> {
        self.text(self.get(id)?, name)
    }
}

/// 读一张 CSV。
///
/// 表头固定 6 行，主键列名固定是 `id`（Java 侧所有 DB 表都是）。
pub fn load_raw(path: &Path) -> Result<RawTable, String> {
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_path(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;

    let mut recs = rdr.records();
    let mut head: Vec<Vec<String>> = Vec::with_capacity(HEADER_ROWS);
    for _ in 0..HEADER_ROWS {
        let Some(rec) = recs.next() else {
            return Err(format!("{}: 表头不足 {HEADER_ROWS} 行", path.display()));
        };
        head.push(
            rec.map_err(|e| format!("{}: {e}", path.display()))?
                .iter()
                .map(|s| s.trim().to_owned())
                .collect(),
        );
    }

    let entity = head[0].first().cloned().unwrap_or_default();
    let types = head[3].clone();
    let columns = head[4].clone();
    let id_col = columns
        .iter()
        .position(|c| c == "id")
        .ok_or_else(|| format!("{}: 第 5 行找不到 id 列", path.display()))?;

    let mut rows = Vec::new();
    let mut by_id = HashMap::new();
    for rec in recs {
        let rec = rec.map_err(|e| format!("{}: {e}", path.display()))?;
        let row: Vec<String> = rec.iter().map(|s| s.trim().to_owned()).collect();
        if row.iter().all(String::is_empty) {
            continue;
        }
        if let Some(id) = row.get(id_col).and_then(|s| s.parse::<i32>().ok()) {
            by_id.insert(id, rows.len());
        }
        rows.push(row);
    }

    Ok(RawTable {
        entity,
        columns,
        types,
        rows,
        by_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn csv_dir() -> Option<PathBuf> {
        std::env::var("DDJ_CSV_DIR").ok().map(PathBuf::from)
    }

    #[test]
    fn loads_real_item_table() {
        let Some(dir) = csv_dir() else {
            eprintln!("跳过：未设置 DDJ_CSV_DIR");
            return;
        };
        let t = load_raw(&dir.join("D-道具表-Item.csv")).expect("读 Item.csv");
        assert_eq!(t.entity, "ItemDB");
        assert!(t.len() > 1000, "道具表行数 {}", t.len());
        // 实测列：id=0、type=8、useLimit=18
        assert_eq!(t.col("id"), Some(0));
        assert_eq!(t.col("type"), Some(8));
        assert_eq!(t.col("useLimit"), Some(18));
        assert!(t.get(1).is_some(), "1 号道具应存在");
        // 大量行的 `useLimit` 是空值（策划只给可用次数有限的道具填），
        // 所以断言「至少有一行填了」而不是「1 号填了」。
        let with_limit = t
            .rows
            .iter()
            .filter(|r| t.int(r, "useLimit").is_some())
            .count();
        assert!(with_limit > 0, "应当有道具填了 useLimit");
    }

    #[test]
    fn loads_real_gift_table() {
        let Some(dir) = csv_dir() else {
            eprintln!("跳过：未设置 DDJ_CSV_DIR");
            return;
        };
        let t = load_raw(&dir.join("D-道具使用效果-Gift.csv")).expect("读 Gift.csv");
        assert_eq!(t.entity, "GiftDB");
        assert!(t.col("id").is_some());
        // 实测列名：id,name,type,reward,useLimit,useCost,noticeType,showEffect,bf_desc1
        // 注意是 `useCost` 不是 `useCostJson`（Java 字段名 useCostJson 对应列 useCost）。
        assert!(t.col("useCost").is_some(), "列名: {:?}", t.columns);
    }
}
