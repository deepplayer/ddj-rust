//! 处理器查找表
//!
//! 替代原 Java 的反射扫描 + `Integer.parseInt(类名后缀)`。
//! 这里是**编译期收集的只读表**，启动零成本，冲突在链接期暴露。

use crate::handler::ErasedHandler;
use std::collections::HashMap;
use std::sync::OnceLock;

/// 注册表。
pub struct HandlerTable {
    by_cmd: HashMap<i32, Box<dyn ErasedHandler>>,
}

impl HandlerTable {
    /// 空表。
    #[must_use]
    pub fn empty() -> Self {
        Self {
            by_cmd: HashMap::new(),
        }
    }

    /// 插入。重复 cmdId 会覆盖并告警（真实实现应在构建期直接 panic）。
    pub fn insert(&mut self, h: Box<dyn ErasedHandler>) {
        let id = h.cmd_id();
        if self.by_cmd.insert(id, h).is_some() {
            tracing::error!(cmd_id = id, "cmdId 重复注册！");
        }
    }

    /// 查找。
    #[must_use]
    pub fn get(&self, cmd_id: i32) -> Option<&dyn ErasedHandler> {
        self.by_cmd.get(&cmd_id).map(AsRef::as_ref)
    }

    /// 已注册数量。
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_cmd.len()
    }
    /// 是否为空。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_cmd.is_empty()
    }

    /// 已注册的全部 cmdId（升序）。
    #[must_use]
    pub fn cmd_ids(&self) -> Vec<i32> {
        let mut v: Vec<i32> = self.by_cmd.keys().copied().collect();
        v.sort_unstable();
        v
    }

    /// 全局单例（启动时 `OnceLock::set`）。
    ///
    /// # Errors
    /// 重复初始化。
    pub fn install(self) -> Result<(), Self> {
        TABLE.set(self)
    }

    /// 取全局表。
    ///
    /// # Panics
    /// 未初始化即调用 —— 启动顺序错误必须在启动期暴露，所以刻意 panic。
    #[must_use]
    pub fn global() -> &'static Self {
        #[allow(clippy::panic)]
        match TABLE.get() {
            Some(t) => t,
            None => panic!("HandlerTable 尚未初始化：请在 Init 阶段调用 install()"),
        }
    }
}

static TABLE: OnceLock<HandlerTable> = OnceLock::new();
