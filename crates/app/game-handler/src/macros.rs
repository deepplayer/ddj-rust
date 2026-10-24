//! 注册宏
//!
//! 原系统：`@LabelMessageMethod(cmdId = 11002)` 注解 + 反射扫描。
//! 这里：`#[[...]]` 不存在的自定义属性，所以用声明宏 + `inventory`。
//!
//! ## 用法
//! ```ignore
//! handle!(ItemUse, 11002, ItemUseReq11002, ItemUseResp11002, |ctx, req| {
//!     Ok(Some(ItemUseResp11002 { ret: 0, ..Default::default() }))
//! });
//! ```
//! 展开后生成一个 `ErasedHandler` 实现并 `inventory::submit!`，
//! cmdId 重复在启动时告警。

/// 声明一个处理器。
///
/// 参数：`名称, cmdId, 请求类型, 响应类型, 闭包体`
#[macro_export]
macro_rules! handle {
    ($name:ident, $cmd:expr, $req:ty, $resp:ty, $body:expr) => {
        /// 由 `handle!` 生成的处理器。
        pub struct $name;

        #[::async_trait::async_trait]
        impl $crate::macros::TypedHandler<$req, $resp> for $name {
            async fn handle(
                &self,
                ctx: &$crate::macros::Ctx,
                req: $req,
            ) -> $crate::macros::Res<Option<$resp>> {
                let f: fn(&$crate::macros::Ctx, $req) -> $crate::macros::Res<Option<$resp>> = $body;
                f(ctx, req)
            }
        }
    };
}

pub use crate::handle;

use ddj_core::Result;

/// 处理器上下文（重导出，宏里用短路径）。
pub use ddj_registry::HandlerCtx as Ctx;

/// 结果类型（重导出）。
pub type Res<T> = Result<T>;

/// 强类型处理器（宏生成时用）。
#[async_trait::async_trait]
pub trait TypedHandler<Req, Resp>: Send + Sync
where
    Req: Send + 'static,
    Resp: Send + 'static,
{
    /// 处理。
    async fn handle(&self, ctx: &Ctx, req: Req) -> Res<Option<Resp>>;
}
