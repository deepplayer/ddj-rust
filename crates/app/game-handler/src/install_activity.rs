//! 活动模板（`ActivityDbTemplateCache`）的注入点。
//!
//! 纯逻辑在 [`crate::activity_template`]，本文件只持有进程级缓存。
//!
//! ## 为什么单独一个模块
//! 活动族的处理器（`CornucopiaMessageHandler` / `WonderlandMessageHandler` /
//! `BridgeMessageHandler` / `ActivityNianbeastMessageHandler` …）**全部**都先做
//! 同一件事：按 `(typeId, actId)` 查模板，然后 `isOpen()` / `isBetweenStartAndEndShowTime()`
//! / `isEndShowTime()` 之一。数据是同一份 `bf_activity`。
//!
//! 把它挂在任何一个具体活动模块下都会误导后来者（以为只服务那个活动），
//! 所以独立成 `install_activity`。
//!
//! ## 未注入的行为
//! 空表 → `get()` 恒 `None` → 所有活动处理器走 `ACTIVITY_NO_EXITS(2751)`。
//! 与原版「`ActivityDbTemplateCache` 没加载完就收到请求」一致：查不到 → 2751。

use std::sync::OnceLock;

use crate::activity_template::{ActivityCache, ActivityTemplate};

/// `ActivityDbTemplateCache` 的注入点。
static ACTIVITIES: OnceLock<ActivityCache> = OnceLock::new();

/// 注入活动模板表（只第一次生效）。
pub fn install(c: ActivityCache) {
    let _ = ACTIVITIES.set(c);
}

/// 当前缓存；未注入时是空表。
pub fn activities() -> &'static ActivityCache {
    ACTIVITIES.get_or_init(ActivityCache::default)
}

/// `ActivityDbTemplateCache.getActivityDbTemplate(actType, actId)`。
#[must_use]
pub fn template(act_type: i32, act_id: i32) -> Option<&'static ActivityTemplate> {
    activities().get(act_type, act_id)
}

/// 便利：查模板并在缺失时给 `ACTIVITY_NO_EXITS`。
///
/// 绝大多数处理器的第一段都是这个形状，抽成函数避免每处都写一遍。
///
/// # Errors
/// 模板不存在时返回 [`ddj_pb::RetCode::ActivityNoExits`]。
pub fn template_or_err(act_type: i32, act_id: i32) -> Result<&'static ActivityTemplate, ddj_pb::RetCode> {
    template(act_type, act_id).ok_or(ddj_pb::RetCode::ActivityNoExits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_cache_returns_none() {
        // 未注入 → 空表
        assert!(template(60, 100043242).is_none());
        assert_eq!(template_or_err(60, 1).unwrap_err(), ddj_pb::RetCode::ActivityNoExits);
    }
}
