//! 用真实 CSV 验证解析器
//!
//! 数据源：`data/apps/game_7010544/resource/template/csv/`
//! 通过环境变量 `DDJ_CSV_DIR` 指定目录；未设置时跳过（CI 无数据）。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use ddj_template::loader::TemplateLoader;
use std::env;

#[test]
fn loads_all_real_tables() {
    let Ok(dir) = env::var("DDJ_CSV_DIR") else {
        eprintln!("跳过：未设置 DDJ_CSV_DIR");
        return;
    };
    let rep = TemplateLoader::new(&dir, false).scan().expect("扫描");

    println!("表数={} 行数={}", rep.tables, rep.rows);
    for w in rep.warnings.iter().take(10) {
        println!("警告: {w}");
    }

    assert!(rep.tables > 400, "只解析到 {} 张表", rep.tables);
    assert!(rep.rows > 10_000, "只解析到 {} 行", rep.rows);
    assert!(
        rep.errors.is_empty(),
        "严格模式下的错误: {:?}",
        rep.errors.first()
    );
}
