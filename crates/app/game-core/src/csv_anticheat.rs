//! 用真实 CSV 实现防骗手册奖励配置。
//!
//! 数据源：`B-变量表-Config.csv` 里 `name=AntiCheatReward` 的那一行（实测 id=568）：
//!
//! ```text
//! 568,AntiCheatReward,
//!   1=25;231=1;211=1;201=1|1=25;231=1;211=2;201=2|1=25;231=1;45=1;202=1|1=25;231=1;46=1;202=1,
//!   【防骗手册】分签奖励（格式：道具id=道具数量;……|……）
//! ```
//!
//! ## 解析口径（`bf/template/config/AntiCheatRewardJson.parse`）
//! ```java
//! this.reward = new ArrayList();
//! for (String s : param.split("\\|")) {
//!     this.reward.add(ResourcesJson.toResourcesJsons(s));
//! }
//! ```
//! `toResourcesJsons` 再按 `;` 切，每段按 `=` 切，`Integer.parseInt(id)` + 数量字符串。
//!
//! ## ⚠ 原版解析失败会让整个字段为 null
//! `ConfigDBCache.init` 的 `catch (Exception e2)` 只记日志，`AntiCheatReward` 保持 `null`；
//! 之后 17151/17152 首行 `ConfigDBCache.AntiCheatReward.canReward(...)` 直接 NPE。
//! Rust 侧没有 null，这里退化为「空配置」（17151 → ret=29，17152 → 静默）。
//! **只在畸形配置下与 Java 分叉**，已记入 `docs/AUDIT.md` A15。

use std::path::Path;

use ddj_game_handler::anticheat::{AntiCheatRewardConfig, RewardGroup};
use ddj_game_handler::install_anticheat::install_config;
use ddj_template::raw_table::{load_raw, RawTable};

const CONFIG_CSV: &str = "B-变量表-Config.csv";
const KEY: &str = "AntiCheatReward";

/// `ResourcesJson.toResourcesJsons`：`1=25;231=1` → `[(1,25),(231,1)]`。
fn parse_pairs(seg: &str) -> RewardGroup {
    // 原版 `if (script.equals("")) return list;` —— 整段为空才短路。
    if seg.is_empty() {
        return Vec::new();
    }
    seg.split(';')
        .filter_map(|p| {
            let (id, num) = p.split_once('=')?;
            Some((id.trim().parse().ok()?, num.trim().parse().ok()?))
        })
        .collect()
}

/// `AntiCheatRewardJson.parse`：先按 `|` 切组，每组再按 `;` 切条目。
#[must_use]
pub fn parse_value(v: &str) -> Vec<RewardGroup> {
    v.split('|').map(parse_pairs).collect()
}

/// 从变量表里取 `AntiCheatReward` 一行并解析。
///
/// # Errors
/// 文件缺失、表头不足 6 行、或表里没有 `AntiCheatReward` 行时返回错误文本。
pub fn load(template_dir: &Path) -> Result<AntiCheatRewardConfig, String> {
    let csv = template_dir.join("csv");
    let t: &RawTable = &load_raw(&csv.join(CONFIG_CSV))?;
    let mut found = None;
    for row in &t.rows {
        if t.text(row, "name") == Some(KEY) {
            found = t.text(row, "value").map(parse_value);
            break;
        }
    }
    let reward = found.ok_or_else(|| format!("{CONFIG_CSV}: 找不到 name={KEY} 的行"))?;
    Ok(AntiCheatRewardConfig { reward })
}

/// 启动时装配；失败只记日志（防骗手册退化为「全部越界」）。
pub fn install(template_dir: &Path) {
    match load(template_dir) {
        Ok(c) => {
            tracing::info!(groups = c.reward.len(), "防骗手册模板已装载");
            install_config(c);
        }
        Err(e) => {
            tracing::warn!(error = %e, "防骗手册模板装载失败，17151 将一律回 PARAM_ERROR");
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
    fn parses_single_group() {
        assert_eq!(parse_pairs("1=25;231=1"), vec![(1, 25), (231, 1)]);
        assert_eq!(parse_pairs(""), Vec::new());
    }

    #[test]
    fn parses_multi_groups() {
        let g = parse_value("1=25;231=1|1=25;231=1;211=2");
        assert_eq!(g.len(), 2);
        assert_eq!(g[0], vec![(1, 25), (231, 1)]);
        assert_eq!(g[1], vec![(1, 25), (231, 1), (211, 2)]);
    }

    #[test]
    fn loads_real_config_row() {
        let Some(dir) = csv_dir() else {
            eprintln!("跳过：未设置 DDJ_CSV_DIR");
            return;
        };
        let tpl = dir.parent().map_or_else(|| dir.clone(), Path::to_path_buf);
        let c = load(&tpl).expect("加载 AntiCheatReward");
        assert_eq!(c.reward.len(), 4, "实测 4 组");
        assert_eq!(c.reward[0], vec![(1, 25), (231, 1), (211, 1), (201, 1)]);
        assert!(c.can_reward(3));
        assert!(!c.can_reward(4));
    }
}
