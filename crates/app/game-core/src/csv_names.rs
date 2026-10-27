//! 用真实 CSV 构建名字池（`W_玩家名字_PlayerName.csv`）。
//!
//! ## 表结构（实测）
//! ```text
//! 第 4 行 类型   int,string,string,string,string
//! 第 5 行 列名   id,surname,surnameRemarks,name,nameRemarks
//! 第 6 行 注释   ID,姓,姓备注,名,名备注2
//! ```
//!
//! ## 两个关键事实
//! 1. **`surname` / `name` 列存的是多语言 key**（`change_word_41001` 这种），
//!    不是直接的中文。真实中文在 `surnameRemarks` / `nameRemarks` 列。
//!    原版走 `PlayerNameDB.getSurname(languageType)` 查多语言表拿译文；
//!    过渡期没有多语言表，直接用 `*Remarks` 列（就是 `zh_CN` 的文案）。
//! 2. **1224 行里只有 250 行填了 `surname`**，其余为空。原版 `NameData.init()`
//!    遇到空姓氏是 `break`（不是 `continue`），所以正好取到前 250 行。
//!    笛卡尔积 = 250 × 1224 = **306000**，与 `e2e/jsclient/DIFF.md` 记录的
//!    「实测 zh_CN 可用名 306000 个」完全吻合。
//!
//! ## 为什么用 `break` 而不是过滤空值
//! 如果过滤掉空行继续往下读，会把表里「预留但未填」的行也算进来，
//! 得到的池子比原版大。原版是 `break`，所以这里也必须是 `break`。

use std::path::Path;

use ddj_game_handler::engine::{build_name_pool, NameLanguage};
use ddj_game_handler::install_engine::{install_name_pool, NamePools};
use ddj_template::raw_table::{load_raw, RawTable};

const NAME_CSV: &str = "W_玩家名字_PlayerName.csv";

/// 从模板目录装载名字池。
///
/// # Errors
/// 文件缺失或表头不足 6 行时返回错误文本。
pub fn load(template_dir: &Path) -> Result<NamePools, String> {
    let csv = template_dir.join("csv");
    let table = load_raw(&csv.join(NAME_CSV))?;
    Ok(build_pools(&table))
}

/// 从已加载的表构建三个语言的池子。
///
/// 原版 `PlayerNameDB.getSurname(languageType)` 按语言查多语言表；
/// 这里三种语言共用同一份 `*Remarks`（即中文），差别只在拼接顺序
/// （`en` 是「名 姓」、`vi_VN` 是「姓 名」）。
fn build_pools(table: &RawTable) -> NamePools {
    let (surnames, names) = read_columns(table);
    // 原版 `init(hashSet)` 传入的是「已被占用的名字」；过渡期没有玩家库，
    // 传空切片等价于「一个都没被占用」。
    let taken: Vec<String> = Vec::new();
    NamePools {
        zh_cn: build_name_pool(&surnames, &names, &taken, NameLanguage::ZhCn),
        en: build_name_pool(&surnames, &names, &taken, NameLanguage::En),
        vi_vn: build_name_pool(&surnames, &names, &taken, NameLanguage::ViVn),
    }
}

/// 读 `surnameRemarks` / `nameRemarks` 两列，**两列各自独立截断**。
///
/// 原版 `NameData.init()` 是**两层嵌套遍历同一张表**：
/// ```java
/// for (PlayerNameDB a : list) {           // 外层：取姓氏
///     if (a.getSurname(lang) 空) break;   // 姓氏为空 → 结束外层
///     for (PlayerNameDB b : list) {       // 内层：取名字
///         if (b.getName(lang) 空) break;  // 名字为空 → 结束内层（下一个姓氏继续）
///         pool.add(a.surname + b.name);
///     }
/// }
/// ```
/// 所以两列的「有效长度」是**独立**的：姓氏取前 250 个，名字取前 1224 个，
/// 笛卡尔积 250 × 1224 = 306000。
///
/// 这里对应地分别扫描两列，各自遇到空值就停，互不影响。
fn read_columns(table: &RawTable) -> (Vec<String>, Vec<String>) {
    let mut surnames = Vec::new();
    let mut names = Vec::new();
    // 外层：姓氏，遇空即停
    for row in &table.rows {
        let s = table.text(row, "surnameRemarks").unwrap_or("").trim();
        if s.is_empty() {
            break;
        }
        surnames.push(s.to_owned());
    }
    // 内层：名字，遇空即停（与姓氏的截断点无关）
    for row in &table.rows {
        let n = table.text(row, "nameRemarks").unwrap_or("").trim();
        if n.is_empty() {
            break;
        }
        names.push(n.to_owned());
    }
    (surnames, names)
}

/// 启动时装配；失败只记日志不阻塞启动（名字池空 → 走兜底 `["玩家"]`）。
pub fn install(template_dir: &Path) {
    match load(template_dir) {
        Ok(p) => {
            tracing::info!(
                zh_cn = p.zh_cn.len(),
                en = p.en.len(),
                vi_vn = p.vi_vn.len(),
                "名字池已装载"
            );
            install_name_pool(p);
        }
        Err(e) => {
            tracing::warn!(
                error = %e,
                dir = %template_dir.display(),
                "名字池装载失败，取随机名将回退到 [\"玩家\"]"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn dir() -> Option<PathBuf> {
        std::env::var("DDJ_CSV_DIR").ok().map(PathBuf::from)
    }

    #[test]
    fn loads_real_table_with_306000_combinations() {
        let Some(dir) = dir() else {
            eprintln!("跳过：未设置 DDJ_CSV_DIR");
            return;
        };
        let tpl_dir = dir.parent().map_or_else(|| dir.clone(), Path::to_path_buf);
        let p = load(&tpl_dir).expect("加载名字表");
        // 250 姓氏 × 1224 名字 = 306000，与 DIFF.md 记录一致
        assert_eq!(p.zh_cn.len(), 306_000, "zh_CN 池子大小");
        assert_eq!(p.en.len(), 306_000);
        assert_eq!(p.vi_vn.len(), 306_000);
        // 抽查：第一条应是「赵 + 向邦」
        assert!(p.zh_cn.iter().any(|n| n == "赵向邦"), "应含 赵向邦");
    }

    #[test]
    fn en_and_vi_use_different_separator() {
        let Some(dir) = dir() else {
            eprintln!("跳过：未设置 DDJ_CSV_DIR");
            return;
        };
        let tpl_dir = dir.parent().map_or_else(|| dir.clone(), Path::to_path_buf);
        let p = load(&tpl_dir).expect("加载名字表");
        // en: "名 姓"；vi_VN: "姓 名"
        assert!(p.en.iter().any(|n| n == "向邦 赵"), "en 是 名+空格+姓");
        assert!(p.vi_vn.iter().any(|n| n == "赵 向邦"), "vi_VN 是 姓+空格+名");
    }
}
