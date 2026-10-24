//! # ddj-template — 配置表
//!
//! 对应 Java：`bf.template.db.*`（450 个 `XxxDB` class）+ `jery.ngsp.template.TemplateFactory`。
//!
//! ## CSV 结构（前 6 行是 schema，第 7 行起是数据）
//! ```text
//! 1  AmberWelfareDB                 ← 实体类名
//! 2  PK                             ← 主键标记
//! 3  3,3,3,3,3,3                    ← 列宽
//! 4  int,string,int,int,int,"list;,="  ← 类型
//! 5  id,cid,type,day,payValue,reward   ← 字段名
//! 6  ID,oppoCid,类型,签到天数,...       ← 中文注释
//! 7+ 1,1001,1,1,0,"1;2,3"           ← 数据
//! ```
//!
//! ## 类型 token 全集（实测 445 表只有 8 种）
//! `int 1752 / string 937 / list 681 / bool 6 / long 5 / double 1`
//! 外加 2 处手滑 `sting`（按 `string` 处理并告警）。
//!
//! ## `list;,=` 语义
//! 分隔符 `;`、键值符 `=`。原 `dbInit()` 字节码做两件事：
//! `reward → rewardJson`、`cid 按 ; 切分 → oppoCids`。
//!
//! ## 子模块
//! - [`raw`]      CSV 解析：schema 抽取 + 行迭代
//! - [`schema`]   类型 token → Rust 类型映射
//! - [`loader`]   全量加载 + 校验（主键唯一 / 外键存在 / 引用完整性）
//! - [`store`]    `TemplateStore`：按表名取 `Arc<Table<T>>`

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod loader;
pub mod raw;
pub mod schema;
pub mod store;

pub use loader::TemplateLoader;
pub use store::TemplateStore;
