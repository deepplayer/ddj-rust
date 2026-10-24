//! 模块注册表
//!
//! 原系统靠 XML 列模块名 + 反射实例化。这里改为**编译期 inventory**：
//! 每个模块 `inventory::submit!` 自己，`Lifecycle` 启动时收集。
//! 好处：模块删了链接器就报错，不会出现"配置里有、代码里没有"。

use crate::module::Module;

/// 模块工厂。
pub type ModuleFactory = fn() -> Box<dyn Module>;

/// 一条注册项。
pub struct ModuleEntry {
    /// 模块名。
    pub name: &'static str,
    /// 构造函数。
    pub factory: ModuleFactory,
}

inventory::collect!(ModuleEntry);

/// 收集所有注册模块（按注册顺序）。
#[must_use]
pub fn all_modules() -> Vec<ModuleEntry> {
    inventory::iter::<ModuleEntry>
        .into_iter()
        .map(|e| ModuleEntry {
            name: e.name,
            factory: e.factory,
        })
        .collect()
}

/// 按名字过滤出要启用的模块。
#[must_use]
pub fn select(enabled: &[&str]) -> Vec<ModuleEntry> {
    if enabled.is_empty() {
        return all_modules();
    }
    all_modules()
        .into_iter()
        .filter(|e| enabled.contains(&e.name))
        .collect()
}

/// 声明一个模块。
///
/// ```ignore
/// ddj_runtime::submit_module!("item", || Box::new(ItemModule::new()));
/// ```
#[macro_export]
macro_rules! submit_module {
    ($name:literal, $factory:expr) => {
        inventory::submit! {
            $crate::registry::ModuleEntry { name: $name, factory: $factory }
        }
    };
}
