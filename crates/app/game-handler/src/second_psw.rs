//! # 二级密码（17351-17355）
//!
//! 对应 Java：
//! - `SysActivityMessageHandler`（行 307-350，5 条全部只做转发）
//! - `bf/game/server/module/sysActivity/model/SecondPsw.java`（210 行，全部逻辑）
//!
//! ## 五条消息
//! | cmd | 方法 | 作用 |
//! |---|---|---|
//! | 17351 | `setPsw` | 设置/修改密码 |
//! | 17352 | `verifyPsw` | 校验密码 |
//! | 17353 | `setEnabled` | 开关二级密码 |
//! | 17354 | `applyResetPsw` / `cancelResetPsw` | 按 `type` 分：1 申请重置、2 取消重置 |
//! | 17355 | `toMsg` | 查询当前状态 |
//!
//! ## 状态机
//! ```text
//! psw: Option<String>        // None 表示从未设置
//! enabled: bool              // 是否启用
//! inputErrorNum: i32         // 连续输错次数
//! applyResetEndTime: i64     // 重置生效时间（0 表示没在重置中）
//! isVerifySuccess: bool      // 本次会话是否已验证过（**不持久化**）
//! ```
//!
//! ## ⚠ 三处容易做错的地方
//!
//! ### 1. 17352 的异常处理与其他四条相反
//! 其他四条 `throws HandlerException`（失败走 `ErrorResp_1`），
//! 但 `verifyPsw` **自己 catch 住 HandlerException**，把它转成
//! `MsgVerifyPswResp_17352{ret=code}` 返回。所以 17352 失败时帧号仍是 **17352**，
//! 不是 1。
//!
//! ### 2. `setPsw` 的 try 范围覆盖了所有 HandlerException
//! ```java
//! try {
//!     Integer.parseInt(psw);
//!     if (isApplyResetState()) throw COMMON_SERVER_STAGE_ERROR;
//!     if (psw != null && !isVerifySuccess) throw SECOND_PSW_NO_VERIFY;
//!     ...
//! } catch (Throwable th) {
//!     throw new HandlerException(SECOND_PSW_FORMAT_ERROR);   // 9502
//! }
//! ```
//! 内层抛的 30 / 9503 全被外层 `catch (Throwable)` 吞掉，统一变成 **9502**。
//! 也就是说：**正在重置中、或未验证就改密码，客户端收到的都是「只能是数字」**。
//! 这是原版缺陷，照抄。
//!
//! ### 3. `applyResetPsw` 的时长单位
//! `ConfigDBCache.SecondpwResetTime * 3600000` —— 配置项的单位是**小时**，
//! 乘 3600000 得到毫秒。

use ddj_pb::RetCode;

/// `ConfigDBCache.SecondpwLimit`：密码长度（必须精确等于）。
pub const DEFAULT_PSW_LIMIT: usize = 6;

/// `ConfigDBCache.SecondpwAttempt`：允许的连续错误次数。
pub const DEFAULT_PSW_ATTEMPT: i32 = 5;

/// `ConfigDBCache.SecondpwResetTime`：重置等待时长（**小时**）。
pub const DEFAULT_RESET_HOURS: i64 = 24;

/// 二级密码状态（`SecondPsw` 的持久化字段 + 会话字段）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SecondPswState {
    /// `psw`：`None` 表示从未设置。
    pub psw: Option<String>,
    /// `enabled`：是否启用。
    pub enabled: bool,
    /// `inputErrorNum`：连续输错次数。
    pub input_error_num: i32,
    /// `applyResetEndTime`：重置生效时间（0 表示没在重置中）。
    pub apply_reset_end_time: i64,
    /// `isVerifySuccess`：本次会话是否已验证（**不持久化**，重连即失效）。
    pub is_verify_success: bool,
}

impl SecondPswState {
    /// `isApplyResetState()`：`applyResetEndTime > now` 表示正在重置等待中。
    #[must_use]
    pub fn is_apply_reset(&self, now: i64) -> bool {
        self.apply_reset_end_time > now
    }
}

/// 配置项。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SecondPswConfig {
    /// 密码长度。
    pub limit: usize,
    /// 允许错误次数。
    pub attempt: i32,
    /// 重置等待小时数。
    pub reset_hours: i64,
}

impl Default for SecondPswConfig {
    fn default() -> Self {
        Self {
            limit: DEFAULT_PSW_LIMIT,
            attempt: DEFAULT_PSW_ATTEMPT,
            reset_hours: DEFAULT_RESET_HOURS,
        }
    }
}

/// 判定结论。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PswOutcome {
    /// 走 `ErrorResp_1`（17351/17353/17354 的失败路径）。
    Reject(RetCode),
    /// 走 Resp 里的 `ret`（**仅 17352**）。
    RespRet(RetCode),
    /// 通过，附带要写回的状态。
    Accept(PswEffect),
}

/// 通过后要执行的动作。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PswEffect {
    /// 17351：密码已设置/修改。`first_set` 表示是首次设置（会把 `enabled` 置 true）。
    SetPsw {
        /// 新密码。
        psw: String,
        /// 是否首次设置。
        first_set: bool,
    },
    /// 17352：校验通过，清空错误计数。
    VerifyOk,
    /// 17353：开关已变更。
    SetEnabled {
        /// 目标状态。
        enabled: bool,
    },
    /// 17354 申请重置：写 `apply_reset_end_time`。
    ApplyReset {
        /// 重置生效时间。
        end_time: i64,
    },
    /// 17354 取消重置：`apply_reset_end_time = 0`。
    CancelReset,
}

impl PswOutcome {
    /// 协议上的 `ret`（仅 17352 用）。
    #[must_use]
    pub const fn resolve_ret(&self) -> Option<RetCode> {
        match self {
            Self::Reject(code) | Self::RespRet(code) => Some(*code),
            Self::Accept(_) => None,
        }
    }
}

/// 判断字符串是否为纯数字（Java `Integer.parseInt` 的接受范围）。
///
/// 原版用 `Integer.parseInt(psw)`，接受前导 `+`/`-`，且长度受 `i32` 限制。
/// 密码长度通常是 6，不会溢出；这里按「全 ASCII 数字」判，
/// 与原版对 6 位密码的实际行为一致。
fn is_all_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// 17351 `SecondPsw.setPsw` 的纯逻辑。
///
/// ⚠ 内层的 30 / 9503 会被外层 `catch (Throwable)` 吞成 9502（见模块文档）。
#[must_use]
pub fn set_psw_verdict(
    state: &SecondPswState,
    psw: Option<&str>,
    now: i64,
    cfg: &SecondPswConfig,
) -> PswOutcome {
    // `psw == null` → PARAM_ERROR（这一条在 try 之外，所以不被吞）
    let Some(psw) = psw else {
        return PswOutcome::Reject(RetCode::ParamError);
    };
    // 长度检查（也在 try 之外）
    if psw.chars().count() != cfg.limit {
        return PswOutcome::Reject(RetCode::SecondPswLengthError);
    }
    // ---- 以下在原版 try 内，任何异常都会被吞成 9502 ----
    if !is_all_digits(psw) {
        return PswOutcome::Reject(RetCode::SecondPswFormatError);
    }
    // `isApplyResetState()` → COMMON_SERVER_STAGE_ERROR(30) → 被吞成 9502
    if state.is_apply_reset(now) {
        return PswOutcome::Reject(RetCode::SecondPswFormatError);
    }
    // 已设过密码但本次未验证 → 9503 → 被吞成 9502
    if state.psw.is_some() && !state.is_verify_success {
        return PswOutcome::Reject(RetCode::SecondPswFormatError);
    }
    let first_set = state.psw.is_none();
    PswOutcome::Accept(PswEffect::SetPsw {
        psw: psw.to_owned(),
        first_set,
    })
}

/// 17352 `SecondPsw.verifyPsw` 的纯逻辑。
///
/// ⚠ 与其他四条不同：**失败也走 Resp 里的 ret**（帧号 17352）。
#[must_use]
pub fn verify_psw_verdict(
    state: &SecondPswState,
    psw: &str,
    cfg: &SecondPswConfig,
) -> PswOutcome {
    // 未设置密码 → 30
    if state.psw.is_none() {
        return PswOutcome::RespRet(RetCode::StageError);
    }
    // 错误次数用尽 → 9504
    if state.input_error_num >= cfg.attempt {
        return PswOutcome::RespRet(RetCode::SecondPswVerifyNumLimit);
    }
    // 非纯数字 → 9502
    if !is_all_digits(psw) {
        return PswOutcome::RespRet(RetCode::SecondPswFormatError);
    }
    // 密码不匹配 → 9505（且要 +1 错误次数）
    if state.psw.as_deref() != Some(psw) {
        return PswOutcome::RespRet(RetCode::SecondPswPswError);
    }
    PswOutcome::Accept(PswEffect::VerifyOk)
}

/// 17353 `SecondPsw.setEnabled` 的纯逻辑。
#[must_use]
pub fn set_enabled_verdict(state: &SecondPswState, enabled: bool) -> PswOutcome {
    // 未验证过 → 9503（这条抛 HandlerException，走 ErrorResp_1）
    if !state.is_verify_success {
        return PswOutcome::Reject(RetCode::SecondPswNoVerify);
    }
    PswOutcome::Accept(PswEffect::SetEnabled { enabled })
}

/// 17354 `SecondPsw.applyResetPsw` 的纯逻辑。
#[must_use]
pub fn apply_reset_verdict(
    state: &SecondPswState,
    now: i64,
    cfg: &SecondPswConfig,
) -> PswOutcome {
    // 已经在重置中 → 30
    if state.is_apply_reset(now) {
        return PswOutcome::Reject(RetCode::StageError);
    }
    // `SecondpwResetTime * 3600000`：配置单位是小时
    let end = now.saturating_add(cfg.reset_hours.saturating_mul(3_600_000));
    PswOutcome::Accept(PswEffect::ApplyReset { end_time: end })
}

/// 17354 `SecondPsw.cancelResetPsw` 的纯逻辑。
#[must_use]
pub fn cancel_reset_verdict(state: &SecondPswState, now: i64) -> PswOutcome {
    // 不在重置中 → 30
    if !state.is_apply_reset(now) {
        return PswOutcome::Reject(RetCode::StageError);
    }
    PswOutcome::Accept(PswEffect::CancelReset)
}

/// 17355 下发的状态快照（`SecondPsw.toMsg()` 的四个字段）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SecondPswSnapshot {
    /// `haveSetPsw`。
    pub have_set_psw: bool,
    /// `enabled`。
    pub enabled: bool,
    /// `inputErrorNum`。
    pub input_error_num: i32,
    /// `resetEndTime`。
    pub reset_end_time: i64,
}

impl SecondPswState {
    /// `SecondPsw.toMsg()`。
    #[must_use]
    pub fn to_snapshot(&self) -> SecondPswSnapshot {
        SecondPswSnapshot {
            have_set_psw: self.psw.is_some(),
            enabled: self.enabled,
            input_error_num: self.input_error_num,
            reset_end_time: self.apply_reset_end_time,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_700_000_000_000;

    fn cfg() -> SecondPswConfig {
        SecondPswConfig::default()
    }

    fn empty() -> SecondPswState {
        SecondPswState::default()
    }

    fn with_psw() -> SecondPswState {
        SecondPswState {
            psw: Some("123456".into()),
            enabled: true,
            ..empty()
        }
    }

    // ── 17351 setPsw ──

    #[test]
    fn set_psw_null_is_param_error() {
        assert_eq!(
            set_psw_verdict(&empty(), None, NOW, &cfg()),
            PswOutcome::Reject(RetCode::ParamError)
        );
    }

    #[test]
    fn set_psw_wrong_length_is_9501() {
        assert_eq!(
            set_psw_verdict(&empty(), Some("12345"), NOW, &cfg()),
            PswOutcome::Reject(RetCode::SecondPswLengthError)
        );
        assert_eq!(
            set_psw_verdict(&empty(), Some("1234567"), NOW, &cfg()),
            PswOutcome::Reject(RetCode::SecondPswLengthError)
        );
    }

    #[test]
    fn set_psw_non_digit_is_9502() {
        assert_eq!(
            set_psw_verdict(&empty(), Some("12345a"), NOW, &cfg()),
            PswOutcome::Reject(RetCode::SecondPswFormatError)
        );
    }

    #[test]
    fn set_psw_first_time_accepts() {
        match set_psw_verdict(&empty(), Some("123456"), NOW, &cfg()) {
            PswOutcome::Accept(PswEffect::SetPsw { first_set, psw }) => {
                assert!(first_set, "从未设置过 → first_set");
                assert_eq!(psw, "123456");
            }
            other => panic!("应通过，得到 {other:?}"),
        }
    }

    #[test]
    fn set_psw_modify_requires_verify_but_is_masked_to_9502() {
        // ⚠ 原版：未验证 → 9503，但被外层 catch(Throwable) 吞成 9502
        let s = with_psw(); // is_verify_success = false
        assert_eq!(
            set_psw_verdict(&s, Some("654321"), NOW, &cfg()),
            PswOutcome::Reject(RetCode::SecondPswFormatError),
            "9503 被吞成 9502 是原版行为"
        );
    }

    #[test]
    fn set_psw_modify_after_verify_accepts() {
        let s = SecondPswState {
            is_verify_success: true,
            ..with_psw()
        };
        match set_psw_verdict(&s, Some("654321"), NOW, &cfg()) {
            PswOutcome::Accept(PswEffect::SetPsw { first_set, .. }) => {
                assert!(!first_set, "已有密码 → 不是首次");
            }
            other => panic!("应通过，得到 {other:?}"),
        }
    }

    #[test]
    fn set_psw_during_reset_is_masked_to_9502() {
        // ⚠ 原版：重置中 → 30，但被吞成 9502
        let s = SecondPswState {
            apply_reset_end_time: NOW + 1000,
            ..empty()
        };
        assert_eq!(
            set_psw_verdict(&s, Some("123456"), NOW, &cfg()),
            PswOutcome::Reject(RetCode::SecondPswFormatError)
        );
    }

    #[test]
    fn set_psw_length_check_precedes_digit_check() {
        // 长度不对时先回 9501，不会走到数字校验
        assert_eq!(
            set_psw_verdict(&empty(), Some("abc"), NOW, &cfg()),
            PswOutcome::Reject(RetCode::SecondPswLengthError)
        );
    }

    // ── 17352 verifyPsw ──

    #[test]
    fn verify_not_set_is_30_via_resp() {
        let out = verify_psw_verdict(&empty(), "123456", &cfg());
        assert_eq!(out, PswOutcome::RespRet(RetCode::StageError));
        // 关键：走 RespRet 而不是 Reject
        assert_eq!(out.resolve_ret(), Some(RetCode::StageError));
    }

    #[test]
    fn verify_too_many_errors_is_9504() {
        let s = SecondPswState {
            input_error_num: 5,
            ..with_psw()
        };
        assert_eq!(
            verify_psw_verdict(&s, "123456", &cfg()),
            PswOutcome::RespRet(RetCode::SecondPswVerifyNumLimit)
        );
    }

    #[test]
    fn verify_wrong_psw_is_9505() {
        assert_eq!(
            verify_psw_verdict(&with_psw(), "000000", &cfg()),
            PswOutcome::RespRet(RetCode::SecondPswPswError)
        );
    }

    #[test]
    fn verify_non_digit_is_9502() {
        assert_eq!(
            verify_psw_verdict(&with_psw(), "12345a", &cfg()),
            PswOutcome::RespRet(RetCode::SecondPswFormatError)
        );
    }

    #[test]
    fn verify_correct_accepts() {
        assert_eq!(
            verify_psw_verdict(&with_psw(), "123456", &cfg()),
            PswOutcome::Accept(PswEffect::VerifyOk)
        );
    }

    #[test]
    fn verify_error_limit_boundary() {
        // 4 次错误仍可尝试
        let s = SecondPswState {
            input_error_num: 4,
            ..with_psw()
        };
        assert!(matches!(
            verify_psw_verdict(&s, "123456", &cfg()),
            PswOutcome::Accept(_)
        ));
        // 5 次（== attempt）被拒
        let s5 = SecondPswState {
            input_error_num: 5,
            ..with_psw()
        };
        assert_eq!(
            verify_psw_verdict(&s5, "123456", &cfg()),
            PswOutcome::RespRet(RetCode::SecondPswVerifyNumLimit)
        );
    }

    // ── 17353 setEnabled ──

    #[test]
    fn enabled_requires_verify() {
        assert_eq!(
            set_enabled_verdict(&with_psw(), false),
            PswOutcome::Reject(RetCode::SecondPswNoVerify)
        );
    }

    #[test]
    fn enabled_after_verify_accepts() {
        let s = SecondPswState {
            is_verify_success: true,
            ..with_psw()
        };
        assert_eq!(
            set_enabled_verdict(&s, false),
            PswOutcome::Accept(PswEffect::SetEnabled { enabled: false })
        );
    }

    // ── 17354 applyReset / cancelReset ──

    #[test]
    fn apply_reset_sets_end_time_in_hours() {
        match apply_reset_verdict(&with_psw(), NOW, &cfg()) {
            PswOutcome::Accept(PswEffect::ApplyReset { end_time }) => {
                // 24 小时 = 86_400_000 ms
                assert_eq!(end_time, NOW + 86_400_000);
            }
            other => panic!("应通过，得到 {other:?}"),
        }
    }

    #[test]
    fn apply_reset_twice_is_30() {
        let s = SecondPswState {
            apply_reset_end_time: NOW + 1000,
            ..with_psw()
        };
        assert_eq!(
            apply_reset_verdict(&s, NOW, &cfg()),
            PswOutcome::Reject(RetCode::StageError)
        );
    }

    #[test]
    fn cancel_reset_requires_pending() {
        assert_eq!(
            cancel_reset_verdict(&with_psw(), NOW),
            PswOutcome::Reject(RetCode::StageError)
        );
    }

    #[test]
    fn cancel_reset_accepts_when_pending() {
        let s = SecondPswState {
            apply_reset_end_time: NOW + 1000,
            ..with_psw()
        };
        assert_eq!(
            cancel_reset_verdict(&s, NOW),
            PswOutcome::Accept(PswEffect::CancelReset)
        );
    }

    // ── 17355 toMsg ──

    #[test]
    fn snapshot_reflects_state() {
        let s = SecondPswState {
            psw: Some("123456".into()),
            enabled: true,
            input_error_num: 2,
            apply_reset_end_time: 12_345,
            is_verify_success: false,
        };
        let snap = s.to_snapshot();
        assert!(snap.have_set_psw);
        assert!(snap.enabled);
        assert_eq!(snap.input_error_num, 2);
        assert_eq!(snap.reset_end_time, 12_345);
    }

    #[test]
    fn snapshot_of_empty_state() {
        let snap = empty().to_snapshot();
        assert!(!snap.have_set_psw);
        assert!(!snap.enabled);
        assert_eq!(snap.input_error_num, 0);
        assert_eq!(snap.reset_end_time, 0);
    }
}
