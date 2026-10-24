//! 模板仓库

use std::collections::HashMap;
use std::sync::Arc;

/// 已加载的一张表。
#[derive(Debug)]
pub struct Table<T> {
    /// 表名（= 实体名）。
    pub name: &'static str,
    /// 全部行。
    pub rows: Vec<T>,
    /// 主键索引。
    pub by_id: HashMap<i32, usize>,
}

impl<T> Table<T> {
    /// 按主键取。
    #[must_use]
    pub fn get(&self, id: i32) -> Option<&T> {
        self.by_id.get(&id).map(|&i| &self.rows[i])
    }
    /// 行数。
    #[must_use]
    pub fn len(&self) -> usize {
        self.rows.len()
    }
    /// 是否为空。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// 全局模板仓库。
///
/// 原 Java 是 `TemplateFactory` 里一堆 `static HashMap`，
/// 这里统一成一个 `HashMap<&'static str, Arc<dyn Any>>` + 类型化取值。
#[derive(Default)]
pub struct TemplateStore {
    tables: HashMap<&'static str, Arc<dyn std::any::Any + Send + Sync>>,
}

impl TemplateStore {
    /// 空仓库。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 放入一张表。
    pub fn put<T: Send + Sync + 'static>(&mut self, t: Table<T>) {
        self.tables.insert(t.name, Arc::new(t));
    }

    /// 取一张表。
    ///
    /// # Errors
    /// 表不存在，或已加载的表类型与 `T` 不符。
    pub fn get<T: Send + Sync + 'static>(&self, name: &str) -> Result<Arc<Table<T>>, String> {
        let any = self
            .tables
            .get(name)
            .ok_or_else(|| format!("模板表 {name} 未加载"))?;
        Arc::downcast::<Table<T>>(any.clone()).map_err(|_| format!("模板表 {name} 类型不符"))
    }

    /// 是否存在该表。
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.tables.contains_key(name)
    }

    /// 已加载表数。
    #[must_use]
    pub fn loaded(&self) -> usize {
        self.tables.len()
    }
}
