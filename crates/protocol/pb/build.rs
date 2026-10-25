//! 构建脚本。
//!
//! build script 在编译期运行，失败就该让构建失败，
//! 因此这里的 `expect` / `panic` 是**预期行为**，不是生产代码缺陷。
#![allow(clippy::expect_used, clippy::panic)]

// 编译期生成 protobuf 消息。
//
// 两套源，用环境变量切：
//
// 1. `proto/jar/*.proto`（**默认**，102 个文件）——
//    从服务端 `jb3.game.protobuf.message.v1.jar` 里把 `FileDescriptorProto`
//    还原出来的**权威** schema。服务端跑的就是它，`_NNNNN` 消息 4984 个。
// 2. `proto/letsproto.proto`（`DDJ_PROTO_LEGACY=1`）——
//    早期从客户端 JS 反出来的 7774 message 版本。保留它是因为部分字段名
//    与 jar 版不同（客户端发布版改过名），排查差异时有用。
//
// 生成物 4.6 MB 级，rustc 峰值内存 ~900 MB。本项目约定「不在本机编译」，
// 远端 32 核 / 126 GB 可直接吃下；若确实需要在低内存机器上只编骨架，
// 可 `DDJ_PB_STUB=1 cargo check`。

use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-env-changed=DDJ_PB_STUB");
    println!("cargo:rerun-if-env-changed=DDJ_PROTO_LEGACY");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=proto");

    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let stub = out.join("gen.rs");

    if std::env::var("DDJ_PB_STUB").as_deref() == Ok("1") {
        // 注意：`include!` 展开处已经在 `pub mod gen { }` 内部，
        // 不能再出现 `//!` 内部文档注释（会触发 E0753），只能用普通注释 + 一个占位项。
        std::fs::write(
            &stub,
            "// 占位：未启用真实 proto 生成。\n\
             // 跳过生成：DDJ_PB_STUB=1 cargo check\n\
             pub const PB_PLACEHOLDER: () = ();\n",
        )
        .expect("write placeholder");
        return;
    }

    // 用 vendored protoc，不依赖系统安装
    let protoc = protoc_bin_vendored::protoc_bin_path().expect("vendored protoc");
    let mut cfg = prost_build::Config::new();
    cfg.protoc_executable(protoc);

    let legacy = std::env::var("DDJ_PROTO_LEGACY").as_deref() == Ok("1");
    let inc = PathBuf::from("proto");

    if legacy {
        let src = inc.join("letsproto.proto");
        println!("cargo:rerun-if-changed={}", src.display());
        cfg.compile_protos(&[src], &[inc]).expect("prost-build");
        // 老版本只有一个包 `pb` → prost 产出 `pb.rs`；转成 gen.rs 让 lib.rs 不必分支。
        let pb = out.join("pb.rs");
        let body = std::fs::read_to_string(&pb).expect("读 pb.rs");
        std::fs::write(&stub, body).expect("write legacy gen.rs");
        return;
    }

    // ---- 默认：jar 还原的多文件 schema ----
    let jar_dir = inc.join("jar");
    let mut protos: Vec<PathBuf> = std::fs::read_dir(&jar_dir)
        .expect("读取 proto/jar")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "proto"))
        .collect();
    protos.sort();
    assert!(!protos.is_empty(), "proto/jar 下没有 .proto");

    cfg.compile_protos(&protos, &[&jar_dir])
        .expect("prost-build");

    // prost 的产物文件名由 **package** 决定（`bf.messages.game.EngineMessage.rs`）。
    // 这里扫一遍 OUT_DIR，按实际文件名拼出嵌套模块，避免手写 102 行。
    let mut generated: Vec<String> = std::fs::read_dir(&out)
        .expect("读取 OUT_DIR")
        .filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().into_owned()))
        .filter(|n| n.ends_with(".rs") && n != "gen.rs" && n != "pb.rs")
        .collect();
    generated.sort();
    assert!(
        !generated.is_empty(),
        "prost-build 没有产出任何 .rs（检查 proto 是否被正确 include）"
    );

    // `bf.messages.game.EngineMessage.rs` → 树 `bf::messages::game::engine_message`
    let mut tree = BTree::default();
    let mut aliases: Vec<String> = Vec::new();
    for name in &generated {
        let stem = name.trim_end_matches(".rs");
        let segs: Vec<String> = stem.split('.').map(snake).collect();
        tree.insert(&segs, name);
        // 兼容旧代码路径 `ddj_pb::gen::engine_message::X`
        if let Some(last) = segs.last() {
            let path = segs.join("::");
            aliases.push(format!("pub use self::{path} as {last};"));
        }
    }

    let mut src = String::from("// 由 build.rs 生成：嵌套模块 + 旧路径别名。\n");
    src.push_str(&tree.render());
    src.push('\n');
    for a in &aliases {
        src.push_str(a);
        src.push('\n');
    }
    std::fs::write(out.join("gen.rs"), src).expect("write gen.rs");
}

fn snake(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for (i, ch) in s.chars().enumerate() {
        if ch.is_uppercase() {
            if i != 0 && !out.ends_with('_') {
                out.push('_');
            }
            out.extend(ch.to_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

/// 极简前缀树：只为把「扁平的文件名列表」渲染成嵌套 `pub mod`。
#[derive(Default)]
struct BTree {
    children: std::collections::BTreeMap<String, (BTree, Option<String>)>,
}

impl BTree {
    fn insert(&mut self, segs: &[String], file: &str) {
        let Some((head, rest)) = segs.split_first() else {
            return;
        };
        let slot = self.children.entry(head.clone()).or_default();
        if rest.is_empty() {
            slot.1 = Some(file.to_owned());
        } else {
            slot.0.insert(rest, file);
        }
    }

    fn render(&self) -> String {
        let mut s = String::new();
        self.render_into(&mut s, 0);
        s
    }

    fn render_into(&self, s: &mut String, depth: usize) {
        let pad = "    ".repeat(depth);
        for (name, (sub, file)) in &self.children {
            s.push_str(&format!("{pad}pub mod {name} {{\n"));
            if let Some(f) = file {
                s.push_str(&format!(
                    "{pad}    include!(concat!(env!(\"OUT_DIR\"), \"/{f}\"));\n"
                ));
            }
            sub.render_into(s, depth + 1);
            s.push_str(&format!("{pad}}}\n"));
        }
    }
}

#[allow(dead_code)]
fn _assert_path_exists(p: &Path) {
    let _ = p;
}
