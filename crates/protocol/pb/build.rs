//! 构建脚本。
//!
//! build script 在编译期运行，失败就该让构建失败，
//! 因此这里的 `expect` / `panic` 是**预期行为**，不是生产代码缺陷。
#![allow(clippy::expect_used, clippy::panic)]

// 编译期生成 protobuf 消息。
//
// 源：work/letsproto.proto（1.75 MB / 8856 message / proto2）
//
// **默认生成**（源文件已签入 `proto/letsproto.proto`）。
//
// 生成物 4.4 MB / 117 顶层模块，rustc 峰值内存 ~900 MB。
// 本项目约定「不在本机编译」，远端 32 核 / 126 GB 可直接吃下；
// 若确实需要在低内存机器上只编骨架，可 `DDJ_PB_STUB=1 cargo check`。

use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=DDJ_PB_STUB");
    println!("cargo:rerun-if-env-changed=DDJ_PROTO");
    println!("cargo:rerun-if-changed=build.rs");

    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let dest = out.join("pb.rs");

    if std::env::var("DDJ_PB_STUB").as_deref() == Ok("1") {
        // 注意：`include!` 展开处已经在 `pub mod gen { }` 内部，
        // 不能再出现 `//!` 内部文档注释（会触发 E0753），只能用普通注释 + 一个占位项。
        std::fs::write(
            &dest,
            "// 占位：未启用真实 proto 生成。\n\
             // 跳过生成：DDJ_PB_STUB=1 cargo check\n\
             // 源文件：work/letsproto.proto（8856 message / proto2）\n\
             pub const PB_PLACEHOLDER: () = ();\n",
        )
        .expect("write placeholder");
        return;
    }

    // crates/protocol/pb → rust/ 需要上跳三级
    let proto = std::env::var("DDJ_PROTO")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("proto/letsproto.proto"));

    if !proto.exists() {
        panic!(
            "找不到 proto 文件: {}（可用 DDJ_PROTO 指定）",
            proto.display()
        );
    }
    println!("cargo:rerun-if-changed={}", proto.display());

    // 先取出 include 目录（拥有所有权），再移动 `proto` 进 `compile_protos`
    let inc: PathBuf = proto
        .parent()
        .map_or_else(|| PathBuf::from("."), PathBuf::from);
    // 用 vendored protoc，不依赖系统安装
    let protoc = protoc_bin_vendored::protoc_bin_path().expect("vendored protoc");
    let mut cfg = prost_build::Config::new();
    cfg.protoc_executable(protoc);
    cfg.compile_protos(&[proto], &[inc]).expect("prost-build");
}
