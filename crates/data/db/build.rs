//! 让 `sqlx::migrate!` 的嵌入内容跟随 .sql 文件变化。
//!
//! `sqlx::migrate!("./migrations")` 在**宏展开时**把 SQL 读进二进制，但它自己
//! 不会告诉 cargo「这个 crate 还依赖 migrations 目录」。于是新增一个 .sql 而
//! 不改任何 .rs 时，cargo 复用上一次的编译产物 —— 迁移表里就永远看不到新版本。
//! 这里显式声明依赖，把这条隐性约束补上。

fn main() {
    println!("cargo:rerun-if-changed=migrations");
    println!("cargo:rerun-if-changed=build.rs");
}
