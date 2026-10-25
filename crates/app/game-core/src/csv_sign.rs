//! 用真实 CSV 实现签到表。
//!
//! 数据源（实测）：
//! - `M-每日签到表-DailySign.csv`：列 `id,reward`，**21 行**，reward 形如 `1=50;22=1;101=1;102=1;103=1`（5 条）。
//! - `M-每日签到宝箱表-DailySignBox.csv`：列 `id,days,boxReward`，**4 行** `1,3` / `2,7` / `3,14` / `4,21`。
//!
//! 注意 `randTimes` **不是 CSV 列**——它在 `DailySignDB.<clinit>` 里硬编码为 3。

use std::path::Path;
use std::sync::Arc;

use ddj_game_handler::install_sign::{install_sign_templates, SignTemplates};
use ddj_template::raw_table::{load_raw, RawTable};

const DAY_CSV: &str = "M-每日签到表-DailySign.csv";
const BOX_CSV: &str = "M-每日签到宝箱表-DailySignBox.csv";

/// 解析 `1=50;22=1;...` 形式的全部条目。
fn parse_pairs(s: &str) -> Vec<(i32, i64)> {
    s.split(';')
        .filter_map(|p| {
            let (id, num) = p.split_once('=')?;
            Some((id.trim().parse().ok()?, num.trim().parse().ok()?))
        })
        .collect()
}

/// 签到天数表 + 宝箱表。
#[derive(Debug)]
pub struct CsvSignTemplates {
    day: RawTable,
    box_tbl: RawTable,
}

impl CsvSignTemplates {
    /// 从模板目录加载（该目录下应有 `csv/`）。
    ///
    /// # Errors
    /// 文件缺失或表头不足 6 行时返回错误文本。
    pub fn load(template_dir: &Path) -> Result<Self, String> {
        let csv = template_dir.join("csv");
        Ok(Self {
            day: load_raw(&csv.join(DAY_CSV))?,
            box_tbl: load_raw(&csv.join(BOX_CSV))?,
        })
    }

    /// 签到表行数（自检用）。
    #[must_use]
    pub fn day_rows(&self) -> usize {
        self.day.len()
    }

    /// 宝箱表行数（自检用）。
    #[must_use]
    pub fn box_rows(&self) -> usize {
        self.box_tbl.len()
    }
}

impl SignTemplates for CsvSignTemplates {
    fn rewards_of_day(&self, day: i32) -> Vec<(i32, i64)> {
        self.day
            .text_of(day, "reward")
            .map(parse_pairs)
            .unwrap_or_default()
    }

    fn box_days(&self, box_id: i32) -> Option<i32> {
        self.box_tbl.int_of(box_id, "days")
    }

    fn box_rewards(&self, box_id: i32) -> Vec<(i32, i64)> {
        self.box_tbl
            .text_of(box_id, "boxReward")
            .map(parse_pairs)
            .unwrap_or_default()
    }
}

/// 启动时装配；失败只记日志（签到会退化成「抽空奖励」）。
pub fn install(template_dir: &Path) {
    match CsvSignTemplates::load(template_dir) {
        Ok(t) => {
            tracing::info!(
                day_rows = t.day_rows(),
                box_rows = t.box_rows(),
                "签到模板已装载"
            );
            install_sign_templates(Arc::new(t));
        }
        Err(e) => {
            tracing::warn!(error = %e, "签到模板装载失败，签到奖励退化为空");
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
    fn parses_multi_pairs() {
        assert_eq!(parse_pairs("1=50;22=1"), vec![(1, 50), (22, 1)]);
        assert_eq!(parse_pairs(""), Vec::new());
        assert_eq!(parse_pairs("bad"), Vec::new());
    }

    #[test]
    fn loads_real_sign_tables() {
        let Some(dir) = csv_dir() else {
            eprintln!("跳过：未设置 DDJ_CSV_DIR");
            return;
        };
        let tpl = dir.parent().map_or_else(|| dir.clone(), Path::to_path_buf);
        let t = CsvSignTemplates::load(&tpl).expect("加载签到表");
        assert_eq!(t.day_rows(), 21, "签到天数应为 21");
        assert_eq!(t.box_rows(), 4, "宝箱应为 4 档");
        // 第 1 天：1=50;22=1;101=1;102=1;103=1
        let d1 = t.rewards_of_day(1);
        assert_eq!(d1.len(), 5, "第 1 天配 5 个奖励");
        assert_eq!(d1[0], (1, 50));
        // 宝箱档位
        assert_eq!(t.box_days(1), Some(3));
        assert_eq!(t.box_days(4), Some(21));
        assert_eq!(t.box_days(9), None);
        assert!(!t.box_rewards(1).is_empty(), "1 号宝箱应有奖励");
    }
}
