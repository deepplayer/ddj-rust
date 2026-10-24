//! 消息号常量
//!
//! 全部来自 proto message 名后缀：`ItemUseReq_11002` → `11002`。
//! 由 `cargo xt gen-cmds` 生成；这里手写高频命令，其余在生成时补齐。

/// 登录相关（原 `LoginCmdId`）。
pub mod login {
    /// 版本检查。
    pub const VERSION: i32 = 0;
    /// 获取区服列表。
    pub const SERVER_LIST: i32 = 3;
    /// 登录（**唯一加密的消息**，XXTEA + Base64）。
    pub const LOGIN: i32 = 4;
    /// 心跳。
    pub const HEARTBEAT: i32 = 5;
    /// 断线重连。
    pub const RECONNECT: i32 = 6;
    /// 登出。
    pub const LOGOUT: i32 = 7;
    /// 公告。
    pub const NOTICE: i32 = 11;
    /// 服务器时间。
    pub const SERVER_TIME: i32 = 98;
}

/// 道具相关。
pub mod item {
    /// 使用道具 —— 本次逆向的起点。
    pub const USE_REQ: i32 = 11002;
}
