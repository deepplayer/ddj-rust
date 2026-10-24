//! 加载与校验

use crate::raw::{parse_header_cells, TableSchema};
use std::path::Path;

/// 加载结果。
#[derive(Debug, Default)]
pub struct LoadReport {
    /// 成功表数。
    pub tables: usize,
    /// 总数据行。
    pub rows: u64,
    /// 警告（`sting` 笔误之类）。
    pub warnings: Vec<String>,
    /// 致命错误。
    pub errors: Vec<String>,
}

/// 模板加载器。
pub struct TemplateLoader {
    dir: std::path::PathBuf,
    strict: bool,
}

impl TemplateLoader {
    /// 构造。
    #[must_use]
    pub fn new(dir: impl AsRef<Path>, strict: bool) -> Self {
        Self {
            dir: dir.as_ref().to_path_buf(),
            strict,
        }
    }

    /// 扫描目录并解析全部表头（骨架阶段只校验 schema，不构建 struct）。
    ///
    /// # Errors
    /// 目录不存在。
    pub fn scan(&self) -> Result<LoadReport, String> {
        if !self.dir.exists() {
            return Err(format!("模板目录不存在: {}", self.dir.display()));
        }
        let mut rep = LoadReport::default();
        let mut stack = vec![self.dir.clone()];

        while let Some(d) = stack.pop() {
            let entries = std::fs::read_dir(&d).map_err(|e| format!("{}: {e}", d.display()))?;
            for e in entries {
                let p = e.map_err(|e| e.to_string())?.path();
                if p.is_dir() {
                    stack.push(p);
                    continue;
                }
                if p.extension().is_none_or(|x| x != "csv") {
                    continue;
                }
                match self.check_one(&p) {
                    Ok(rows) => {
                        rep.tables += 1;
                        rep.rows += rows;
                    }
                    Err(msg) => {
                        if self.strict {
                            rep.errors.push(msg);
                        } else {
                            rep.warnings.push(msg);
                        }
                    }
                }
            }
        }
        Ok(rep)
    }

    /// 校验单个 CSV。
    ///
    /// 用 `csv` crate 而不是 `str::lines()`——真表里存在**带引号的多行单元格**
    /// （`VasiMap.roomClick` 的注释列就有跨行内容），按行切会切碎。
    /// 表头与数据行统一走同一个 reader，保证引号语义一致。
    fn check_one(&self, p: &Path) -> Result<u64, String> {
        let mut rdr = csv::ReaderBuilder::new()
            .has_headers(false)
            .flexible(true)
            .comment(None)
            .from_path(p)
            .map_err(|e| format!("{}: {e}", p.display()))?;

        // 直接拿单元格数组，不要 join(",") 再切——`list;,=` 里就有逗号。
        let mut head: Vec<Vec<String>> = Vec::with_capacity(6);
        for rec in rdr.records().take(6) {
            let rec = rec.map_err(|e| format!("{}: {e}", p.display()))?;
            head.push(rec.iter().map(ToString::to_string).collect());
        }
        let schema: TableSchema =
            parse_header_cells(&head).map_err(|e| format!("{}: {e}", p.display()))?;

        let mut rows = 0u64;
        for rec in rdr.records() {
            let rec = rec.map_err(|e| format!("{}: {e}", p.display()))?;
            rows += 1;
            // 去掉尾部空单元格后再比列数（数据行常见尾随逗号）
            let mut n = rec.len();
            while n > 0 && rec.get(n - 1).is_some_and(|s| s.trim().is_empty()) {
                n -= 1;
            }
            if rows == 1 && n > schema.fields.len() {
                return Err(format!(
                    "{}: {} 列数 {} 多于表头 {}",
                    p.display(),
                    schema.entity,
                    n,
                    schema.fields.len()
                ));
            }
        }
        Ok(rows)
    }
}
