//! 从 `bf_config` 读保存桌面的渠道白名单，从变量表读奖励。
//!
//! ## 两个数据源
//! | 用途 | 来源 | 原版位置 |
//! |---|---|---|
//! | 渠道白名单 | `bf_config.configId = 59`（**数据库行**，非 CSV） | `PlatformServerModule.parseConfig59`（`:1315`）|
//! | 奖励 | `B-变量表-Config.csv` 的 `name=SaveIconReward`（id=801） | `ConfigDBCache.SaveIconReward` |
//!
//! ## ⚠ 渠道白名单在数据库里，不在 CSV
//! `bf_config` 是 MySQL/PG 里的表（`data/sql/platform.sql:480`），
//! 由 `DBPConfigModule.getInstance().listAll()` 读进内存。
//! 实测 `configId=59` 的值是 **空串**（`platform.sql:562`），
//! 所以 `shortcutConfigMap` 为空、`isEligibleShortcut` 恒 false。
//!
//! 这里**直接读 CSV**（`B-变量表-Config.csv` 里没有 name=59 的行），
//! 所以只有在库里配了才有白名单。为了不引入启动期 DB 依赖，
//! 本模块只加载奖励；渠道白名单保留 `install_config` 接口，
//! 等 PC 配置表接进 PG 后再注入。已记入 `docs/AUDIT.md` A17。

use std::path::Path;

use ddj_game_handler::favourable::PlatformUrls;
use ddj_game_handler::install_favourable::{install_comments, install_reward, install_urls};
use ddj_game_handler::install_shortcut::{install_config, install_rewards};
use ddj_game_handler::shortcut::ShortcutConfig;
use ddj_template::raw_table::{load_raw, RawTable};

const CONFIG_CSV: &str = "B-变量表-Config.csv";
const SAVE_ICON_KEY: &str = "SaveIconReward";
const COMMENT_KEY: &str = "CommentReward";

/// 解析 `11=1;215=3` 形式的全部条目。
fn parse_pairs(s: &str) -> Vec<(i32, i64)> {
    s.split(';')
        .filter_map(|p| {
            let (id, num) = p.split_once('=')?;
            Some((id.trim().parse().ok()?, num.trim().parse().ok()?))
        })
        .collect()
}

/// 从变量表按 `name` 取一行并解析成 `(id, num)` 列表。
///
/// # Errors
/// 文件缺失、表头不足 6 行、或表里没有该行时返回错误文本。
pub fn load_named(template_dir: &Path, key: &str) -> Result<Vec<(i32, i64)>, String> {
    let csv = template_dir.join("csv");
    let t: &RawTable = &load_raw(&csv.join(CONFIG_CSV))?;
    for row in &t.rows {
        if t.text(row, "name") == Some(key) {
            return Ok(t.text(row, "value").map(parse_pairs).unwrap_or_default());
        }
    }
    Err(format!("{CONFIG_CSV}: 找不到 name={key} 的行"))
}

/// `SaveIconReward`（保存桌面奖励）。
///
/// # Errors
/// 同 [`load_named`]。
pub fn load_rewards(template_dir: &Path) -> Result<Vec<(i32, i64)>, String> {
    load_named(template_dir, SAVE_ICON_KEY)
}

/// `CommentReward`（渠道好评奖励）。
///
/// # Errors
/// 同 [`load_named`]。
pub fn load_comment_reward(template_dir: &Path) -> Result<Vec<(i32, i64)>, String> {
    load_named(template_dir, COMMENT_KEY)
}

/// 启动时装配；失败只记日志（17303 发不出奖励）。
pub fn install(template_dir: &Path) {
    match load_rewards(template_dir) {
        Ok(r) => {
            tracing::info!(count = r.len(), "保存桌面奖励已装载");
            install_rewards(r);
        }
        Err(e) => {
            tracing::warn!(error = %e, "保存桌面奖励装载失败，17303 将发空奖励");
        }
    }
    // 渠道白名单：库里没配之前保持空（原版线上也是空）。
    // 显式注入一个空表，让「已注入但为空」与「根本没注入」在代码里可区分
    // （前者 eligible 恒 false 且可复现，后者取决于 OnceLock 是否被别处填过）。
    install_config(ShortcutConfig::default());

    match load_comment_reward(template_dir) {
        Ok(r) => {
            tracing::info!(count = r.len(), "好评奖励已装载");
            install_reward(r);
        }
        Err(e) => {
            tracing::warn!(error = %e, "好评奖励装载失败，17103 将发空奖励");
        }
    }
    // 好评模板 `CommentDB` 在打包的 445 张 CSV 里不存在，所以保持空表
    // → 17102 恒回 ACTIVITY_PARAM_ERROR(2754)，与原版线上一致。
    install_comments(Vec::new());
    // `bf_config.configId` 19/20/21 实测都是 NULL → 空 URL、空渠道白名单
    // → 17101 的 cafeUrl/oneStoreUrl 为空、17103 恒回 FAVOURABLE_ERROR_1(3941)。
    install_urls(PlatformUrls::default());
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
        assert_eq!(parse_pairs("11=1;215=3"), vec![(11, 1), (215, 3)]);
        assert_eq!(parse_pairs(""), Vec::new());
    }

    #[test]
    fn loads_real_comment_reward_row() {
        let Some(dir) = csv_dir() else {
            eprintln!("跳过：未设置 DDJ_CSV_DIR");
            return;
        };
        let tpl = dir.parent().map_or_else(|| dir.clone(), Path::to_path_buf);
        let r = load_comment_reward(&tpl).expect("加载 CommentReward");
        // 实测 `1=100;231=2;211=1;202=1`
        assert_eq!(r.len(), 4, "实测 4 个奖励");
        assert_eq!(r[0], (1, 100));
    }

    #[test]
    fn loads_real_reward_row() {
        let Some(dir) = csv_dir() else {
            eprintln!("跳过：未设置 DDJ_CSV_DIR");
            return;
        };
        let tpl = dir.parent().map_or_else(|| dir.clone(), Path::to_path_buf);
        let r = load_rewards(&tpl).expect("加载 SaveIconReward");
        // 实测 `11=1;215=3;216=3;30=1;234=1`
        assert_eq!(r.len(), 5, "实测 5 个奖励");
        assert_eq!(r[0], (11, 1));
    }
}
