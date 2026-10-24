//! # GM 充值/邮件（原 `data/web/gm/gmPay.php`，152 行）
//!
//! ## 原版流程
//! ```text
//! if ($_SERVER['REQUEST_METHOD'] !== 'POST')  → 405 {"status":"error","message":"Method not allowed"}
//! $userId = $_POST['userid']
//! if (!queryUser($userId))                    → {"status":"error","message":"用户不存在！请确认","data":null}
//! switch ($_POST['request_type']) {
//!   'daijin': queryCdk ? POST 127.0.0.1:1544/gs/pushPlayerReplaceMoneyNum2 : 没有权限
//!   'mail':   queryCdk ? POST 127.0.0.1:1544/gs/sendMail                    : 没有权限
//!   'tingfu': 空实现（只赋了 $url 但没调）→ 空响应体
//!   default:  → 400 {"status":"error","message":"未知请求类型"}
//! }
//! ```
//!
//! ## 必须原样保留的三处「不合理」
//! 1. **`daijin` 无权限时 `status` 仍是 `success`**，错误信息塞在 `error` 键里
//!    （`mail` 无权限则是 `status=error` + `message` 键）。前端两种键名都得认。
//! 2. **`daijin` 完全不看下游返回值** —— `sendPostRequest` 的结果被丢弃，即使 jetty
//!    报错也照回「充值成功！」。
//! 3. **`tingfu` 什么都不发** —— 200 且响应体为空，不是 JSON。
//!
//! ## 与原版唯一的有意偏离
//! 原版把 `$_POST['userid']` 直接拼进 SQL（`SELECT * FROM player WHERE id = $userId`），
//! 是注入点。这里改用绑定参数，只保留「玩家是否存在」这一语义，其余行为不变。

use serde_json::{json, Value};

/// `request_type` 的三种取值。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GmAction {
    /// 发代金 → `POST /gs/pushPlayerReplaceMoneyNum2`
    Daijin,
    /// 发道具邮件 → `POST /gs/sendMail`
    Mail,
    /// 停服跑马灯（原版空实现）
    Tingfu,
}

impl GmAction {
    /// 下游 `jetty 1544` 的路径；`tingfu` 没有下游，返回 `None`。
    #[must_use]
    pub const fn push_path(self) -> Option<&'static str> {
        match self {
            Self::Daijin => Some("/gs/pushPlayerReplaceMoneyNum2"),
            Self::Mail => Some("/gs/sendMail"),
            Self::Tingfu => None,
        }
    }
}

/// `switch ($_POST['request_type'])` 的解析结果。
#[must_use]
pub fn parse_action(request_type: &str) -> Option<GmAction> {
    match request_type {
        "daijin" => Some(GmAction::Daijin),
        "mail" => Some(GmAction::Mail),
        "tingfu" => Some(GmAction::Tingfu),
        _ => None,
    }
}

/// 判定结果，与 HTTP 响应一一对应。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GmDecision {
    /// 玩家不存在 → 200，`{"status":"error","message":"用户不存在！请确认","data":null}`
    UserNotFound,
    /// 没有 CDK 权限且动作是 `daijin` → 200，`status` 仍为 `success`
    NoPermDaijin,
    /// 没有 CDK 权限且动作是 `mail` → 200，`status=error`
    NoPermMail,
    /// 有权限，转发到下游
    Push(GmAction),
    /// `tingfu`：200 空响应
    Tingfu,
    /// 未知 `request_type` → 400
    UnknownType,
}

/// 复刻 `gmPay.php` 的判定顺序：**先查玩家、再查权限、最后看动作**。
///
/// `player_exists` 来自 `SELECT * FROM player WHERE id = $userid`；
/// `has_cdk` 来自 `SELECT * FROM user_cdk WHERE userid = '$userid'`。
#[must_use]
pub fn decide(action: Option<GmAction>, player_exists: bool, has_cdk: bool) -> GmDecision {
    if !player_exists {
        return GmDecision::UserNotFound;
    }
    match action {
        None => GmDecision::UnknownType,
        Some(GmAction::Tingfu) => GmDecision::Tingfu,
        Some(GmAction::Daijin) => {
            if has_cdk {
                GmDecision::Push(GmAction::Daijin)
            } else {
                GmDecision::NoPermDaijin
            }
        }
        Some(GmAction::Mail) => {
            if has_cdk {
                GmDecision::Push(GmAction::Mail)
            } else {
                GmDecision::NoPermMail
            }
        }
    }
}

/// 判定结果的 HTTP 状态码。
#[must_use]
pub const fn status_of(d: GmDecision) -> u16 {
    match d {
        GmDecision::UnknownType => 400,
        _ => 200,
    }
}

/// 判定结果的响应体；`None` 表示空 body（仅 `tingfu`）。
#[must_use]
pub fn body_of(d: GmDecision) -> Option<Value> {
    match d {
        // 注意：`message` 键，不是 `error`。
        GmDecision::UserNotFound => Some(json!({
            "status": "error",
            "message": "用户不存在！请确认",
            "data": null,
        })),
        // 注意：`status` 是 success —— 原版 bug，照抄。
        GmDecision::NoPermDaijin => Some(json!({
            "status": "success",
            "error": "没有权限！",
            "data": null,
        })),
        GmDecision::NoPermMail => Some(json!({
            "status": "error",
            "message": "没有权限！",
            "data": null,
        })),
        // 下游结果被丢弃，恒回成功。
        GmDecision::Push(_) => Some(json!({
            "status": "success",
            "message": "充值成功！",
            "data": null,
        })),
        GmDecision::Tingfu => None,
        GmDecision::UnknownType => Some(json!({
            "status": "error",
            "message": "未知请求类型",
        })),
    }
}

/// 非 POST 请求的响应（原版 405 + JSON）。
#[must_use]
pub fn method_not_allowed() -> (u16, Value) {
    (
        405,
        json!({ "status": "error", "message": "Method not allowed" }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_only_the_three_known_types() {
        assert_eq!(parse_action("daijin"), Some(GmAction::Daijin));
        assert_eq!(parse_action("mail"), Some(GmAction::Mail));
        assert_eq!(parse_action("tingfu"), Some(GmAction::Tingfu));
        assert_eq!(parse_action("Daijin"), None, "区分大小写");
        assert_eq!(parse_action(""), None);
    }

    #[test]
    fn push_paths_match_php() {
        assert_eq!(
            GmAction::Daijin.push_path(),
            Some("/gs/pushPlayerReplaceMoneyNum2")
        );
        assert_eq!(GmAction::Mail.push_path(), Some("/gs/sendMail"));
        assert_eq!(GmAction::Tingfu.push_path(), None);
    }

    #[test]
    fn missing_player_wins_over_everything() {
        // 连未知 request_type 也要先报玩家不存在。
        assert_eq!(
            decide(Some(GmAction::Daijin), false, true),
            GmDecision::UserNotFound
        );
        assert_eq!(decide(None, false, true), GmDecision::UserNotFound);
        assert_eq!(
            decide(Some(GmAction::Tingfu), false, false),
            GmDecision::UserNotFound
        );
    }

    #[test]
    fn unknown_type_is_400_after_player_check() {
        assert_eq!(decide(None, true, true), GmDecision::UnknownType);
        assert_eq!(status_of(GmDecision::UnknownType), 400);
    }

    #[test]
    fn daijin_without_cdk_reports_success_status() {
        let d = decide(Some(GmAction::Daijin), true, false);
        assert_eq!(d, GmDecision::NoPermDaijin);
        let body = body_of(d).unwrap();
        assert_eq!(body["status"], "success", "原版 bug：无权限也算 success");
        assert_eq!(body["error"], "没有权限！");
        assert!(body.get("message").is_none(), "daijin 用 error 键");
    }

    #[test]
    fn mail_without_cdk_reports_error_status() {
        let d = decide(Some(GmAction::Mail), true, false);
        assert_eq!(d, GmDecision::NoPermMail);
        let body = body_of(d).unwrap();
        assert_eq!(body["status"], "error");
        assert_eq!(body["message"], "没有权限！");
        assert!(body.get("error").is_none(), "mail 用 message 键");
    }

    #[test]
    fn both_actions_authorized_push_downstream() {
        assert_eq!(
            decide(Some(GmAction::Daijin), true, true),
            GmDecision::Push(GmAction::Daijin)
        );
        assert_eq!(
            decide(Some(GmAction::Mail), true, true),
            GmDecision::Push(GmAction::Mail)
        );
    }

    #[test]
    fn push_response_ignores_downstream_result() {
        let body = body_of(GmDecision::Push(GmAction::Daijin)).unwrap();
        assert_eq!(body["status"], "success");
        assert_eq!(body["message"], "充值成功！");
        assert_eq!(body["data"], Value::Null);
    }

    #[test]
    fn tingfu_returns_empty_body_not_json() {
        let d = decide(Some(GmAction::Tingfu), true, false);
        assert_eq!(d, GmDecision::Tingfu);
        assert_eq!(status_of(d), 200);
        assert!(body_of(d).is_none(), "tingfu 是空响应体");
    }

    #[test]
    fn user_not_found_body_uses_message_key() {
        let body = body_of(GmDecision::UserNotFound).unwrap();
        assert_eq!(body["status"], "error");
        assert_eq!(body["message"], "用户不存在！请确认");
        assert_eq!(body["data"], Value::Null);
    }

    #[test]
    fn method_not_allowed_is_405() {
        let (code, body) = method_not_allowed();
        assert_eq!(code, 405);
        assert_eq!(body["message"], "Method not allowed");
    }

    #[test]
    fn every_non_error_decision_is_200() {
        for d in [
            GmDecision::UserNotFound,
            GmDecision::NoPermDaijin,
            GmDecision::NoPermMail,
            GmDecision::Push(GmAction::Daijin),
            GmDecision::Push(GmAction::Mail),
            GmDecision::Tingfu,
        ] {
            assert_eq!(status_of(d), 200, "{d:?} 应为 200");
        }
    }
}
