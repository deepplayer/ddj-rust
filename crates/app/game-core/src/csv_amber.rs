//! 用真实 CSV 实现琥珀福利表。
//!
//! 数据源：`H-琥珀福利-AmberWelfare.csv`，列 `id,cid,type,day,payValue,reward`，实测 **31 行**：
//! - `type=1` **7 行**：每日签到（`day` 1..7），reward 如 `219=1`、`24=2`。
//! - `type=2` **1 行**：专属奖励，reward 是 6 条。
//! - `type=3` **23 行**：充值豪礼，`payValue` 是门槛（98/198/328/648…），reward 如 `1=50`。
//!
//! 注意 `cid` 列（实测恒为 1181）是活动实例 id，本实现暂不使用。

use std::path::Path;
use std::sync::Arc;

use ddj_game_handler::amber::AmberTemplate;
use ddj_game_handler::install_amber::{install_amber_tables, AmberTables};
use ddj_template::raw_table::{load_raw, RawTable};

const AMBER_CSV: &str = "H-琥珀福利-AmberWelfare.csv";

/// 解析 `1=50;22=1` 形式的全部条目。
fn parse_pairs(s: &str) -> Vec<(i32, i64)> {
    s.split(';')
        .filter_map(|p| {
            let (id, num) = p.split_once('=')?;
            Some((id.trim().parse().ok()?, num.trim().parse().ok()?))
        })
        .collect()
}

/// 从表里按 `type` 分组。
///
/// # Errors
/// 文件缺失或表头不足 6 行时返回错误文本。
pub fn load(template_dir: &Path) -> Result<AmberTables, String> {
    let csv = template_dir.join("csv");
    let t: &RawTable = &load_raw(&csv.join(AMBER_CSV))?;

    let mut out = AmberTables::default();
    for row in &t.rows {
        let Some(id) = t.int(row, "id") else { continue };
        let template_type = t.int(row, "type").unwrap_or(0);
        let day = t.int(row, "day").unwrap_or(0);
        let pay_value = t.int(row, "payValue").unwrap_or(0);
        let reward_json = t
            .text(row, "reward")
            .map(parse_pairs)
            .unwrap_or_default();
        let tpl = AmberTemplate {
            id,
            template_type,
            day,
            pay_value,
            reward_json,
        };
        match template_type {
            1 => out.sign_in.push(tpl),
            2 => out.exclusive = Some(tpl),
            3 => out.charges.push(tpl),
            _ => {}
        }
    }
    out.sign_in.sort_by_key(|t| t.day);
    Ok(out)
}

/// 启动时装配；失败只记日志（琥珀退化成「什么都不能领」）。
pub fn install(template_dir: &Path) {
    match load(template_dir) {
        Ok(t) => {
            tracing::info!(
                sign_in = t.sign_in.len(),
                charges = t.charges.len(),
                has_exclusive = t.exclusive.is_some(),
                "琥珀模板已装载"
            );
            install_amber_tables(Arc::new(t));
        }
        Err(e) => {
            tracing::warn!(error = %e, "琥珀模板装载失败，琥珀福利退化为不可用");
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
    fn parses_pairs() {
        assert_eq!(parse_pairs("219=1"), vec![(219, 1)]);
        assert_eq!(parse_pairs("11=1;33=1"), vec![(11, 1), (33, 1)]);
        assert_eq!(parse_pairs(""), Vec::new());
    }

    #[test]
    fn loads_real_amber_table() {
        let Some(dir) = csv_dir() else {
            eprintln!("跳过：未设置 DDJ_CSV_DIR");
            return;
        };
        let tpl = dir.parent().map_or_else(|| dir.clone(), Path::to_path_buf);
        let t = load(&tpl).expect("加载琥珀表");
        assert_eq!(t.sign_in.len(), 7, "签到 7 天");
        assert_eq!(t.sign_in[0].day, 1, "按 day 升序");
        assert_eq!(t.sign_in[0].reward_json, vec![(219, 1)]);
        assert!(t.exclusive.is_some(), "应有专属奖励行");
        assert_eq!(
            t.exclusive.as_ref().map_or(0, |e| e.reward_json.len()),
            6,
            "专属奖励 6 条"
        );
        assert_eq!(t.charges.len(), 23, "充值豪礼 23 档");
        // 门槛最小的一档：98 元 → 1=50
        let min = t.charges.iter().filter(|c| c.pay_value > 0).min_by_key(|c| c.pay_value).expect("有门槛");
        assert_eq!(min.pay_value, 98);
        assert_eq!(min.reward_json, vec![(1, 50)]);
    }
}
