//! # 客户端特约消息（`ClientSendSpecialMsgHandler`）
//!
//! 对应 Java：`bf/game/server/message/handler/ClientSendSpecialMsgHandler.java`（33 行）。
//!
//! ## 原版就是一个空壳
//! ```java
//! public String handlerMsg(String msgId, String data) {
//!     switch (msgId) {
//!         case "1":
//!             break;          // ← 只有 break，没有 return，也没有任何处理
//!     }
//!     return null;            // ← 唯一出口
//! }
//! ```
//! `switch` 里唯一的分支是 `case "1": break;`，`break` 之后直接落到
//! `return null`。所以 **`handlerMsg` 对任何 `msgId` 都返回 `null`**。
//!
//! 上层：
//! ```java
//! String respData = handlerMsg(req.getMsgId(), req.getData());
//! builder.setRet(0);
//! builder.setMsgId(req.getMsgId());
//! if (respData != null) builder.setData(respData);   // 永远不成立
//! ```
//!
//! 结论：**1109 永远回 `{ret=0, msgId=<回显>}`，`data` 字段永不设置**。
//!
//! ## 为什么照抄而不"实现"它
//! 这条消息显然是留给客户端和服务端做临时定制的口子（`data` 是 json）。
//! 原版没有实现任何子命令，我们也不臆造 —— 臆造出来的子命令在真实客户端
//! 上会表现成"服务端能回但客户端不认"，比回 `ret=0` 更难排查。
//! 等真实客户端联调时看它发了什么 `msgId`，再决定要不要补。

/// `handlerMsg(msgId, data)` 的返回值：原版恒为 `null`。
///
/// 保留这个函数是为了**显式记录**「原版没有实现任何子命令」这一事实，
/// 而不是在接线层写死 `None` 让人误以为是漏写。
#[must_use]
pub const fn handler_msg(_msg_id: &str, _data: &str) -> Option<String> {
    // 原版 `switch` 里唯一的 `case "1": break;` 之后仍然落到 `return null`，
    // 所以**没有**任何 `msgId` 会产生非 null 的返回值。
    // 这里不写 `match`：写成 `match { "1" => None, _ => None }` 会被
    // clippy 判成 `match_same_arms`，反而掩盖了「原版只有一个空分支」这一点。
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn msg_id_1_returns_none() {
        // 原版 `case "1": break;` → 落到 return null
        assert_eq!(handler_msg("1", "{}"), None);
    }

    #[test]
    fn unknown_msg_id_returns_none() {
        assert_eq!(handler_msg("999", "{}"), None);
        assert_eq!(handler_msg("", ""), None);
    }

    #[test]
    fn data_is_ignored() {
        // 原版 handlerMsg 的 `data` 形参从未被使用
        assert_eq!(handler_msg("1", "anything"), None);
        assert_eq!(handler_msg("1", ""), None);
    }
}
