//! 启动前校验
//!
//! 原系统配置错了要等运行时才炸（比如端口写 0、超时写负数）。
//! 这里在 `Init` 阶段一次性全部检查完，问题早暴露。

use crate::schema::Config;

/// 可校验的配置。
pub trait Validate {
    /// 校验失败返回第一条错误说明。
    fn validate(&self) -> Result<(), String>;
}

impl Validate for Config {
    fn validate(&self) -> Result<(), String> {
        if self.service.server_id < 0 {
            return Err("service.server_id 不能为负".into());
        }
        if self.net.max_conn_total == 0 {
            return Err("net.max_conn_total 不能为 0".into());
        }
        if self.net.idle_timeout_secs == 0 {
            return Err("net.idle_timeout_secs 不能为 0".into());
        }
        if self.db.max_connections == 0 {
            return Err("db.max_connections 不能为 0".into());
        }
        if self.gameplay.item_use_limit == 0 {
            return Err("gameplay.item_use_limit 不能为 0".into());
        }
        if !["json", "pretty"].contains(&self.telemetry.log_format.as_str()) {
            return Err(format!(
                "telemetry.log_format 只能是 json / pretty，当前为 {}",
                self.telemetry.log_format
            ));
        }
        Ok(())
    }
}
