//! 用真实 CSV 实现身份（等级）表 `S-身份表-Status.csv`。
//!
//! 原版 `StatusDBCache.getPlayerLvTemplate(level)` 只用到 `id` + `playerExp`：
//! `playerExp` 是「升到**下一级**所需经验」。实测 24 行，满级行 `playerExp = 0`：
//!
//! ```text
//! 1,100  2,1000  3,2200  4,4000  5,7000  6,11000  7,15000  8,20000
//! 9,25000  10,32000  11,40000  12,50000  13,70000  14,100000  15,150000
//! 16,200000  17,250000  18,350000  19,500000  20,700000  21,1000000
//! 22,1500000  23,2000000  24,0
//! ```

use std::path::Path;

use ddj_game_handler::player::LevelTemplate;
use ddj_template::raw_table::load_raw;

/// `S-身份表-Status.csv`。
const STATUS_CSV: &str = "S-身份表-Status.csv";

/// 从 CSV 读出全部等级模板（按 `id` 升序）。
///
/// # Errors
/// 文件缺失或表头不足 6 行时返回错误文本。
pub fn load(template_dir: &Path) -> Result<Vec<LevelTemplate>, String> {
    let csv = template_dir.join("csv");
    let t = load_raw(&csv.join(STATUS_CSV))?;
    let mut out = Vec::with_capacity(t.rows.len());
    for row in &t.rows {
        let Some(id) = t.int(row, "id") else { continue };
        out.push(LevelTemplate {
            id,
            player_exp: t.int(row, "playerExp").map_or(0, i64::from),
        });
    }
    out.sort_by_key(|t| t.id);
    Ok(out)
}

/// 装载等级表；失败只记日志（1003 升级会一直回 `ret=2`）。
pub fn install(template_dir: &Path) {
    match load(template_dir) {
        Ok(list) => {
            let max = list.last().map_or(0, |t| t.id);
            tracing::info!(status_levels = list.len(), max_level = max, "身份等级表已装载");
            ddj_game_handler::install_player::install_levels(list);
        }
        Err(e) => {
            tracing::warn!(error = %e, "身份等级表装载失败，1003 升级将一直回 ret=2");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// `DDJ_CSV_DIR` 指向 `template/csv` 本身，而 [`load`] 会再拼 `csv/`，
    /// 所以这里回退到父目录 `template`。
    fn csv_dir() -> Option<PathBuf> {
        std::env::var("DDJ_CSV_DIR")
            .ok()
            .map(|p| {
                let p = PathBuf::from(p);
                if p.join(STATUS_CSV).exists() {
                    p.parent().map_or(p.clone(), Path::to_path_buf)
                } else {
                    p
                }
            })
    }

    #[test]
    fn real_csv_loads_24_levels() {
        let Some(dir) = csv_dir() else { return };
        let list = load(&dir).expect("load");
        assert_eq!(list.len(), 24);
        assert_eq!(list[0], LevelTemplate { id: 1, player_exp: 100 });
        assert_eq!(list[1], LevelTemplate { id: 2, player_exp: 1000 });
        assert_eq!(list[22], LevelTemplate { id: 23, player_exp: 2_000_000 });
        // 满级 playerExp = 0
        assert_eq!(list[23], LevelTemplate { id: 24, player_exp: 0 });
        // 查不到的等级返回 None（原版 null）
        // 查不到的等级返回 None（原版 null）：靠 install_levels 装进去后由
        // `install_player::levels()` 的 `[LevelTemplate]` 实现兜住。
        assert!(list.iter().all(|t| t.id != 0));
        assert!(list.iter().all(|t| t.id != 25));
        assert_eq!(
            list.iter().find(|t| t.id == 3).map(|t| t.player_exp),
            Some(2200)
        );
    }
}
