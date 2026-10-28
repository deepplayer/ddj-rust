//! 用真实 CSV 实现游历事件表 + 游历配置。
//!
//! 数据源（实测 `data/apps/game_7010544/resource/template/csv/`）：
//! | 表 | 文件 | 行数 | 关键列 |
//! |---|---|---|---|
//! | `TravelEventsDB` | `Y-游历事件-TravelEvents.csv` | **84** | `type` / `place` / `unlockType` / `targetId` / `progressValue` / `extractWeight` / `weightScale` / `intimacy` / `reward` |
//! | `Config` | `B-变量表-Config.csv` | — | `PhysicalStrength` / `FixedPointTravel` / `TravelFirstEventGirl` / `TravelFirstEventOffspring` / `PartnerInviteTime` / `PartnerChangeCost` |
//!
//! ## 列格式
//! - `intimacy`：类型 `list=,`，值 `410=1` → `(410, 1)`。空 → `None`。
//! - `reward`：类型 `"list;,="`，值 `20=1;21=2` → `[(20,1),(21,2)]`。空串 → `[]`。
//! - `weightScale`：`double`，值 `0.8` / `1.2` / `1.3`。
//! - `place` 分布：`12×12`、`1/4/5/6/7/10×7`、`2/3/8/9/11×6` —— **没有 `place=0`**，
//!   所以「固定事件」分支（`place == 0`）在真实数据下永远抽不到表里的行，
//!   只能靠 `TravelFirstEventGirl` 命中。这是原版数据状态，照抄。
//! - `type` 分布：`2 珍兽 ×36`、`1 门客 ×18`、`3 奖励 ×18`、`4 美人 ×12`。
//! - `progressValue == 0` 的 38 行；`reward` 为空的 48 行。

use std::path::Path;

use ddj_game_handler::travel::{TravelConfig, TravelEvent};
use ddj_template::raw_table::load_raw;

const EVENTS_CSV: &str = "Y-游历事件-TravelEvents.csv";
const CONFIG_CSV: &str = "B-变量表-Config.csv";

/// 按 `;` 切成非空段。
fn list_of(s: &str) -> Vec<&str> {
    s.split(';').map(str::trim).filter(|x| !x.is_empty()).collect()
}

/// `"list;,="` → `[(i32, i64)]`（`20=1;21=2`）。
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

/// `ResourcesJson.createResourcesJson(List<String>)`：**段数必须正好 2**，否则 `null`。
///
/// `intimacy` 列类型是 `list=,`，实测全是 `410=1` 两段；
/// 段数不是 2 时返回 `None`（对应原版 `null` → `getIntimacy()` 回 0）。
fn one_res(s: &str) -> Option<(i32, i64)> {
    let mut it = s.trim().split('=');
    let a = it.next()?.trim().parse().ok()?;
    let b = it.next()?.trim().parse().ok()?;
    Some((a, b))
}

/// 从 CSV 读出游历事件表。
///
/// `template_dir` 是 `.../resource/template`，CSV 在它下面的 `csv/` 子目录。
pub fn load_events(template_dir: &Path) -> Result<Vec<TravelEvent>, String> {
    let t = load_raw(&template_dir.join("csv").join(EVENTS_CSV))?;
    let mut out = Vec::with_capacity(t.rows.len());
    for row in &t.rows {
        let Some(id) = t.int(row, "id") else { continue };
        let intimacy_text = t.text(row, "intimacy").unwrap_or("");
        out.push(TravelEvent {
            id,
            event_type: t.int(row, "type").unwrap_or(0),
            place: t.int(row, "place").unwrap_or(0),
            place_name: t.text(row, "placeName").unwrap_or("").to_string(),
            unlock_type: t.int(row, "unlockType").unwrap_or(0),
            unlock_value: t.int(row, "unlockValue").unwrap_or(0),
            target_id: t.int(row, "targetId").unwrap_or(0),
            progress_value: t.int(row, "progressValue").unwrap_or(0),
            extract_weight: t.int(row, "extractWeight").unwrap_or(0),
            weight_scale: t
                .text(row, "weightScale")
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(1.0),
            intimacy: if intimacy_text.trim().is_empty() {
                None
            } else {
                one_res(intimacy_text)
            },
            rewards: t
                .text(row, "reward")
                .map(res_pairs)
                .unwrap_or_default(),
        });
    }
    Ok(out)
}

/// 从 `B-变量表-Config.csv` 读游历配置。
///
/// 原版这些是 `ConfigDBCache` 的静态字段：
/// - `PhysicalStrength` → 每次随机游历的体力
/// - `FixedPointTravel` → 桃花岛每次的印章消耗（`ResourcesJson`）
/// - `TravelFirstEventGirl` → `[次数, 事件id]` 的固定事件表（`ResourcesJson` 的 `getId/getNum`）
/// - `TravelFirstEventOffspring` → 首个子嗣的事件次数门槛
/// - `PartnerInviteTime` → 十连叠加的邀请次数
/// - `PartnerChangeCost` → 15009 变性消耗
pub fn load_config(template_dir: &Path) -> TravelConfig {
    let mut cfg = TravelConfig::default();
    let Ok(t) = load_raw(&template_dir.join("csv").join(CONFIG_CSV)) else {
        return cfg;
    };
    for row in &t.rows {
        let Some(name) = t.text(row, "name") else {
            continue;
        };
        let value = t.text(row, "value").unwrap_or("");
        match name {
            "PhysicalStrength" => {
                if let Ok(v) = value.trim().parse::<i64>() {
                    cfg.physical_strength = v;
                }
            }
            "FixedPointTravel" => {
                if let Some(p) = one_res(value) {
                    cfg.fixed_point_travel = p;
                }
            }
            "TravelFirstEventGirl" => {
                cfg.first_event_girl = res_pairs(value)
                    .into_iter()
                    .map(|(a, b)| (i64::from(a), i32::try_from(b).unwrap_or(0)))
                    .collect();
            }
            "TravelFirstEventOffspring" => {
                if let Ok(v) = value.trim().parse::<i64>() {
                    cfg.first_event_offspring = v;
                }
            }
            "PartnerInviteTime" => {
                if let Ok(v) = value.trim().parse::<i32>() {
                    cfg.partner_invite_time = v;
                }
            }
            "PartnerChangeCost" => {
                if let Some(p) = one_res(value) {
                    cfg.partner_change_cost = p;
                }
            }
            _ => {}
        }
    }
    cfg
}

/// 装配游历事件表 + 配置；失败只记日志（15002/15004 抽不出事件 → 回 1005）。
pub fn install(template_dir: &Path) {
    let events = load_events(template_dir);
    let cfg = load_config(template_dir);
    apply(events, cfg);
}

/// 把已装载的结果注入 handler 层（单独抽出便于单测）。
fn apply(events: Result<Vec<TravelEvent>, String>, cfg: TravelConfig) {
    match events {
        Ok(list) => {
            tracing::info!(travel_events = list.len(), "游历事件表已装载");
            ddj_game_handler::install_travel::install_templates(list);
        }
        Err(e) => {
            tracing::warn!(error = %e, "游历事件表装载失败，15002/15004 将抽不到事件");
        }
    }
    tracing::info!(
        physical_strength = cfg.physical_strength,
        fixed_point_travel = ?cfg.fixed_point_travel,
        first_event_girl = cfg.first_event_girl.len(),
        partner_invite_time = cfg.partner_invite_time,
        partner_change_cost = ?cfg.partner_change_cost,
        "游历配置已装载"
    );
    ddj_game_handler::install_travel::install_config(cfg);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// `DDJ_CSV_DIR` 指向 `.../template/csv`，`load_*` 要的是 `.../template`。
    fn csv_dir() -> Option<PathBuf> {
        std::env::var("DDJ_CSV_DIR")
            .ok()
            .map(PathBuf::from)
            .and_then(|p| p.parent().map(Path::to_path_buf))
    }

    #[test]
    fn one_res_needs_exactly_two_parts() {
        assert_eq!(one_res("410=1"), Some((410, 1)));
        assert_eq!(one_res("410"), None);
        assert_eq!(one_res("410=1=2"), Some((410, 1)));
    }

    #[test]
    fn res_pairs_parses_list() {
        assert_eq!(res_pairs("20=1;21=2"), vec![(20, 1), (21, 2)]);
        assert_eq!(res_pairs(""), Vec::new());
    }

    /// ⚠ 回归：`install` 曾把 `template_dir` 直接当 CSV 目录，
    /// 启动日志出现「游历事件表装载失败 …/template/Y-游历事件-TravelEvents.csv 不存在」，
    /// 同时配置里 `first_event_girl = 0`。这里锁住真实目录形态。
    #[test]
    fn template_dir_is_the_parent_of_csv() {
        let Some(dir) = csv_dir() else { return };
        assert!(dir.join("csv").join(EVENTS_CSV).exists());
        assert!(dir.join("csv").join(CONFIG_CSV).exists());
        assert!(!dir.join(EVENTS_CSV).exists(), "CSV 不在 template 根目录");
    }

    #[test]
    fn real_config_is_the_online_values() {
        let Some(dir) = csv_dir() else { return };
        let c = load_config(&dir);
        // 实测 `B-变量表-Config.csv`
        assert_eq!(c.physical_strength, 1);
        assert_eq!(c.fixed_point_travel, (24, 1));
        assert_eq!(c.partner_invite_time, 50);
        assert_eq!(c.partner_change_cost, (1, 500));
        assert_eq!(c.first_event_offspring, 4);
        assert_eq!(
            c.first_event_girl,
            vec![(1, 9101), (2, 12401), (3, 7301), (4, 12401), (5, 2101)]
        );
    }

    #[test]
    fn first_event_girl_ids_exist_in_event_table() {
        let Some(dir) = csv_dir() else { return };
        let events = load_events(&dir).expect("load");
        let cfg = load_config(&dir);
        for (_, id) in &cfg.first_event_girl {
            assert!(
                events.iter().any(|e| e.id == *id),
                "固定事件 {id} 不在事件表里（原版会 NPE）"
            );
        }
    }

    #[test]
    fn real_csv_loads_84_events() {
        let Some(dir) = csv_dir() else { return };
        let list = load_events(&dir).expect("load");
        assert_eq!(list.len(), 84, "实测 84 行");
        // 没有 place == 0 的行
        assert!(list.iter().all(|e| e.place != 0));
        // 四个类型的代表行
        let e = list.iter().find(|e| e.id == 1101).expect("1101");
        assert_eq!(e.event_type, 1);
        assert_eq!(e.target_id, 1003);
        assert_eq!(e.rewards, vec![(201, 1)]);
        let e = list.iter().find(|e| e.id == 1201).expect("1201");
        assert_eq!(e.event_type, 2);
        assert_eq!(e.intimacy, Some((407, 1)));
        assert!((e.weight_scale - 1.2).abs() < 1e-9);
        let e = list.iter().find(|e| e.id == 1301).expect("1301");
        assert_eq!(e.event_type, 3);
        assert_eq!(e.rewards, vec![(20, 1)]);
        let e = list.iter().find(|e| e.id == 12456).expect("12456");
        assert_eq!(e.event_type, 4);
        assert_eq!(e.place, 12);
        assert_eq!(e.intimacy, Some((410, 1)));
    }
}
