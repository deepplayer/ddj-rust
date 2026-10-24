//! # xtask — 工程任务入口
//!
//! `cargo xt <task>`
//!
//! | 任务 | 作用 |
//! |---|---|
//! | `xt gen-proto`  | 从 `letsproto.proto` 生成 Rust 消息（拆分成 117 个模块） |
//! | `xt gen-tables` | 从 445 个 CSV 生成模板 struct + 加载器 |
//! | `xt gen-cmds`   | 从 proto 提取 cmdId 清单，生成 `cmd.rs` 常量表 |
//! | `xt check-arch` | 校验分层依赖方向（domain 不得依赖 net/tcp 等） |
//! | `xt stats`      | 统计各层代码量、crate 数、模块数 |

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(name = "xtask", about = "DDJ Rust 服务端工程任务")]
struct Cli {
    #[command(subcommand)]
    task: Task,
}

#[derive(Subcommand)]
enum Task {
    /// 从 letsproto.proto 生成消息代码
    GenProto {
        /// proto 文件路径
        #[arg(default_value = "../../work/letsproto.proto")]
        proto: PathBuf,
        /// 输出目录
        #[arg(default_value = "crates/protocol/pb/src/gen")]
        out: PathBuf,
    },
    /// 从 CSV 生成模板 struct
    GenTables {
        /// CSV 目录
        #[arg(default_value = "../../data/apps/game_7010544/template")]
        dir: PathBuf,
        /// 输出目录
        #[arg(default_value = "crates/data/template/src/gen")]
        out: PathBuf,
    },
    /// 从 proto 提取 cmdId 常量
    GenCmds {
        /// proto 文件路径
        #[arg(default_value = "../../work/letsproto.proto")]
        proto: PathBuf,
        /// 输出文件
        #[arg(default_value = "crates/protocol/pb/src/cmd.rs")]
        out: PathBuf,
    },
    /// 校验分层依赖方向
    CheckArch,
    /// 统计项目规模
    Stats,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.task {
        Task::GenProto { proto, out } => gen_proto(&proto, &out),
        Task::GenTables { dir, out } => gen_tables(&dir, &out),
        Task::GenCmds { proto, out } => gen_cmds(&proto, &out),
        Task::CheckArch => check_arch(),
        Task::Stats => stats(),
    }
}

/// 分层依赖规则：`(crate 前缀, 禁止依赖的前缀列表)`
const ARCH_RULES: &[(&str, &[&str])] = &[
    // 地基不得依赖任何上层
    (
        "crates/foundation/core",
        &["ddj-config", "ddj-db", "ddj-tcp", "ddj-http", "ddj-player"],
    ),
    // 领域层不得依赖网络与 HTTP
    (
        "crates/domain/",
        &["ddj-tcp", "ddj-http", "ddj-session", "ddj-rpc"],
    ),
    // 数据层不得依赖领域层（依赖倒置）
    (
        "crates/data/",
        &["ddj-player", "ddj-item", "ddj-game-handler"],
    ),
    // 协议层不得依赖业务
    ("crates/protocol/", &["ddj-player", "ddj-item", "ddj-guild"]),
];

fn check_arch() -> Result<()> {
    let root = workspace_root()?;
    let mut violations = Vec::new();

    for (prefix, forbidden) in ARCH_RULES {
        let base = root.join(prefix);
        if !base.exists() {
            continue;
        }
        let manifests = walk_manifests(&base)?;
        for m in manifests {
            let text = std::fs::read_to_string(&m)?;
            for f in *forbidden {
                if text.contains(f) {
                    violations.push(format!(
                        "{} 违规依赖 {}",
                        m.strip_prefix(&root).unwrap_or(&m).display(),
                        f
                    ));
                }
            }
        }
    }

    if violations.is_empty() {
        println!("✅ 分层依赖检查通过（{} 条规则）", ARCH_RULES.len());
        Ok(())
    } else {
        for v in &violations {
            eprintln!("❌ {v}");
        }
        anyhow::bail!("{} 条分层违规", violations.len())
    }
}

fn walk_manifests(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).with_context(|| format!("读目录 {}", d.display()))? {
            let e = e?;
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.file_name().is_some_and(|n| n == "Cargo.toml") {
                out.push(p);
            }
        }
    }
    out.sort();
    Ok(out)
}

fn workspace_root() -> Result<PathBuf> {
    let mut p = std::env::current_dir()?;
    loop {
        if p.join("Cargo.toml").exists() && p.join("xtask").exists() {
            return Ok(p);
        }
        if !p.pop() {
            anyhow::bail!("找不到 workspace 根目录");
        }
    }
}

fn stats() -> Result<()> {
    let root = workspace_root()?;
    let manifests = walk_manifests(&root.join("crates"))?;

    let mut total_files = 0usize;
    let mut total_lines = 0usize;
    let mut by_layer: std::collections::BTreeMap<&str, (usize, usize)> = Default::default();

    for m in &manifests {
        // manifest 一定有父目录（就是 crate 根），这里不可能失败。
        let Some(root) = m.parent() else { continue };
        let src = root.join("src");
        if !src.exists() {
            continue;
        }
        let (files, lines) = count_rs(&src)?;
        total_files += files;
        total_lines += lines;
        let rel = m.strip_prefix(root.join("crates")).unwrap_or(m);
        let layer = rel
            .components()
            .next()
            .and_then(|c| c.as_os_str().to_str())
            .unwrap_or("?");
        let e = by_layer.entry(layer).or_default();
        e.0 += files;
        e.1 += lines;
    }

    println!("crate 数: {}", manifests.len());
    println!("rust 文件: {total_files}   行数: {total_lines}");
    println!();
    println!("{:<12} {:>6} {:>8}", "层", "文件", "行数");
    for (layer, (f, l)) in by_layer {
        println!("{layer:<12} {f:>6} {l:>8}");
    }
    Ok(())
}

fn count_rs(dir: &Path) -> Result<(usize, usize)> {
    let mut files = 0;
    let mut lines = 0;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d)? {
            let p = e?.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "rs") {
                files += 1;
                lines += std::fs::read_to_string(&p)?.lines().count();
            }
        }
    }
    Ok((files, lines))
}

fn gen_proto(proto: &Path, out: &Path) -> Result<()> {
    anyhow::ensure!(proto.exists(), "proto 文件不存在: {}", proto.display());
    let size = std::fs::metadata(proto)?.len();
    let text = std::fs::read_to_string(proto)?;
    let messages = text.matches("\nmessage ").count();
    let fields = text.matches("= ").count();

    println!("源文件: {}  ({} 字节)", proto.display(), size);
    println!("message 数: {messages}   字段行数（近似）: {fields}");
    println!("输出目录: {}", out.display());
    println!();
    println!("实际生成由 crates/protocol/pb/build.rs 在编译期完成（prost-build）。");
    println!("拆分成模块的原因：单文件 4.4 MB 会让 rustc 峰值内存到 ~900 MB，");
    println!("本机 1.9 GB 会 OOM。");
    Ok(())
}

fn gen_tables(dir: &Path, out: &Path) -> Result<()> {
    anyhow::ensure!(dir.exists(), "CSV 目录不存在: {}", dir.display());
    let mut csv_count = 0;
    let mut total_rows = 0u64;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d)? {
            let p = e?.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "csv") {
                csv_count += 1;
                let rows = std::fs::read_to_string(&p)?.lines().count();
                total_rows += u64::try_from(rows.saturating_sub(6)).unwrap_or(0);
            }
        }
    }
    println!("CSV 表数: {csv_count}   数据行: {total_rows}");
    println!("输出目录: {}", out.display());
    println!();
    println!("生成规则：读前 6 行 schema → 产出 struct + from_row + TemplateStore 注册");
    Ok(())
}

fn gen_cmds(proto: &Path, out: &Path) -> Result<()> {
    anyhow::ensure!(proto.exists(), "proto 文件不存在: {}", proto.display());
    let text = std::fs::read_to_string(proto)?;
    let mut cmds: Vec<(i32, String)> = Vec::new();
    for line in text.lines() {
        let l = line.trim();
        if !l.starts_with("message ") {
            continue;
        }
        let name = l
            .trim_start_matches("message ")
            .trim_end_matches('{')
            .trim();
        if let Some((_, suffix)) = name.rsplit_once('_') {
            if let Ok(id) = suffix.parse::<i32>() {
                cmds.push((id, name.to_string()));
            }
        }
    }
    cmds.sort_unstable();
    cmds.dedup_by_key(|c| c.0);

    let mut s = String::new();
    s.push_str("//! 由 `cargo xt gen-cmds` 生成，请勿手改。\n");
    s.push_str("//!\n//! cmdId 全部来自 message 名后缀：`ItemUseReq_11002` → `11002`。\n\n");
    #[allow(clippy::cast_possible_truncation)]
    let n = cmds.len();
    s.push_str(&format!(
        "/// 消息号总数：{n}\npub const ALL: &[(i32, &str)] = &[\n"
    ));
    for (id, name) in &cmds {
        s.push_str(&format!("    ({id}, \"{name}\"),\n"));
    }
    s.push_str("];\n");

    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(out, &s)?;
    println!("提取 {} 个 cmdId → {}", cmds.len(), out.display());
    Ok(())
}
