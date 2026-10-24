//! schema → Rust 代码生成
//!
//! 450 个 `bf.template.db.XxxDB` class 一个都不读：它们的字段就是 CSV 第 5 行，
//! `dbInit()` 做的事（`reward → rewardJson`、`cid 按 ; 切分 → oppoCids`）
//! 是全局统一规则，写在生成器里比写在 450 个类里更可靠。

use crate::raw::{FieldType, TableSchema};

/// 生成一个 struct。
#[must_use]
pub fn gen_struct(s: &TableSchema) -> String {
    let mut out = String::new();
    out.push_str("/// 由 `cargo xt gen-tables` 生成，请勿手改。\n");
    out.push_str("#[derive(Debug, Clone, serde::Deserialize)]\n");
    out.push_str(&format!("pub struct {} {{\n", s.entity));
    for (f, t) in s.fields.iter().zip(&s.types) {
        out.push_str(&format!("    /// {}\n", "（见 CSV 第 6 行注释）"));
        out.push_str(&format!("    pub {}: {},\n", sanitize(f), t.rust_type()));
    }
    out.push_str("}\n\n");
    out
}

/// 字段名清洗：CSV 里的名字可能撞 Rust 关键字。
#[must_use]
pub fn sanitize(name: &str) -> String {
    const KW: &[&str] = &[
        "type", "match", "move", "ref", "self", "super", "crate", "fn", "let", "use", "mod", "pub",
        "impl", "where", "loop", "box", "try", "async", "await", "dyn",
    ];
    let n = name.trim().replace([' ', '-'], "_");
    if KW.contains(&n.as_str()) {
        format!("r#{n}")
    } else {
        n
    }
}

/// 生成一行 CSV 的解析代码。
#[must_use]
pub fn gen_row_parser(s: &TableSchema) -> String {
    let mut out = String::from("    fn parse(cells: &[String]) -> Result<Self, String> {\n");
    out.push_str(&format!(
        "        if cells.len() < {} {{ return Err(format!(\"{}: 列数不足 {{}}\", cells.len())); }}\n",
        s.fields.len(),
        s.entity
    ));
    out.push_str("        Ok(Self {\n");
    for (i, (f, t)) in s.fields.iter().zip(&s.types).enumerate() {
        let name = sanitize(f);
        let expr = match t {
            FieldType::Int => format!("cells[{i}].trim().parse::<i32>().unwrap_or_default()"),
            FieldType::Long => format!("cells[{i}].trim().parse::<i64>().unwrap_or_default()"),
            FieldType::Double => format!("cells[{i}].trim().parse::<f64>().unwrap_or_default()"),
            FieldType::Bool => {
                format!("cells[{i}].trim() == \"1\" || cells[{i}].eq_ignore_ascii_case(\"true\")")
            }
            FieldType::Str => format!("cells[{i}].clone()"),
            FieldType::List { sep, .. } => format!(
                "cells[{i}].split('{sep}').filter(|s| !s.is_empty()).map(str::to_string).collect()"
            ),
        };
        out.push_str(&format!("            {name}: {expr},\n"));
    }
    out.push_str("        })\n    }\n");
    out
}
