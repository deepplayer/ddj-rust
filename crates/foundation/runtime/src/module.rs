//! 模块与阶段
//!
//! 对应 Java：`serverModuleListConfig.xml` 里的 `<module name=… moduleClass=…>`，
//! 以及 `XxxModule` 的 `init/start/stop` 钩子。

use async_trait::async_trait;
use ddj_core::Result;

/// 启动阶段。顺序严格，不可跳。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Phase {
    /// 建连接池、加载静态资源。
    PreInit = 0,
    /// 加载模板表、注册表、缓存预热。
    Init = 1,
    /// 开始 accept / 起定时器。
    Start = 2,
    /// 全部就绪，开始对外服务。
    Ready = 3,
}

/// 一个可启动模块。
///
/// 默认实现全部为空，模块只实现需要的那几个阶段。
#[async_trait]
pub trait Module: Send + Sync + 'static {
    /// 模块名（日志 / 拓扑排序用）。
    fn name(&self) -> &'static str;

    /// 依赖的其它模块名。用于拓扑排序，必须正交无环。
    fn depends_on(&self) -> &'static [&'static str] {
        &[]
    }

    /// 建连接、开文件。
    async fn pre_init(&mut self) -> Result<()> {
        Ok(())
    }
    /// 加载数据。
    async fn init(&mut self) -> Result<()> {
        Ok(())
    }
    /// 对外服务。
    async fn start(&mut self) -> Result<()> {
        Ok(())
    }
    /// 停机（反序调用，必须幂等）。
    async fn stop(&mut self) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_ordering() {
        assert!(Phase::PreInit < Phase::Init);
        assert!(Phase::Init < Phase::Start);
        assert!(Phase::Start < Phase::Ready);
    }
}
