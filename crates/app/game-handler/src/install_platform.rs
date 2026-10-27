//! 1022 / 1107 / 1110 的接线（平台侧只读信息）。
//!
//! 纯逻辑在 [`crate::platform_info`]，本文件只做解码 → 调纯函数 → 编码回包。
//!
//! ## 过渡期数据源
//! 三条都依赖平台侧配置（`bf_word_config` / `bf_wechat_official_account` /
//! `PlatformServerModule.switchPayment`），这些表在 `0002_platform.sql` 里已平移，
//! 但仓储层还没实现。这里用一份**进程内静态快照**顶住：
//!
//! - [`install_platform`] 由 `game-core` 在启动时调用，注入真实配置；
//! - 未注入时 [`PlatformSnapshot::default()`] 生效，行为与「配置表全空」一致 ——
//!   1022 回一份空字段的 `MsgVipWordConfig`（原版 `toMsg()` 对空配置就是这么回），
//!   1107 回 `ret=29`（渠道不在表里），1110 回 `state=0`（`switchPayment` 为空）。
//!
//! 接上仓储后只需替换 `PlatformSnapshot` 的来源，闭包与回包逻辑不用动。

use std::sync::{Arc, OnceLock};

use ddj_core::Clock;
use ddj_pb::gen::player_message::{
    GetSwitchPaymentReq1110, GetSwitchPaymentResp1110, GetWechatOfficialAccountInfoReq1107,
    MsgQueryVipWordConfigReq1022, MsgQueryVipWordConfigResp1022,
};
use ddj_registry::HandlerTable;
use prost::Message;

use crate::platform_info::{
    self, WechatChannelConfig, WechatOfficialAccount, WechatInfoOutcome, WordConfig,
};
use crate::runtime::{decode_err, FnHandler, MemWorld};

/// 平台侧配置快照。
#[derive(Debug, Clone, Default)]
pub struct PlatformSnapshot {
    /// `PlatformServerModule.getWordConfig()`（`bf_word_config`）。
    pub word_config: WordConfig,
    /// 玩家所在渠道的 `wechatConfig`（`PlatformServerModule.wechatOfficialAccount`）。
    pub wechat_channel: Option<WechatChannelConfig>,
    /// 按 `wechatId` 查到的公众号（`bf_wechat_official_account`）。
    pub wechat_account: Option<WechatOfficialAccount>,
    /// `PlatformServerModule.switchPayment`（`List<Integer>`）。
    pub switch_payment: Vec<i32>,
    /// 玩家 cid（原版 `player.getCid()`）。
    pub cid: String,
}

static SNAPSHOT: OnceLock<Arc<PlatformSnapshot>> = OnceLock::new();

/// 注入平台配置快照（只第一次生效）。
pub fn install_platform(s: PlatformSnapshot) {
    let _ = SNAPSHOT.set(Arc::new(s));
}

fn snapshot() -> Option<&'static Arc<PlatformSnapshot>> {
    SNAPSHOT.get()
}

/// 注册 3 条平台信息消息，返回注册条数。
pub fn install(table: &mut HandlerTable, world: Arc<MemWorld>, _clock: Arc<dyn Clock>) -> usize {
    let mut n = 0;

    // 1022 查 VIP 客服文案。
    {
        table.insert(Box::new(FnHandler::new(1022, move |_ctx, body| {
            let _req = MsgQueryVipWordConfigReq1022::decode(body).map_err(decode_err)?;
            // 原版无分支：`ret=0` + `getWordConfig().toMsg()`
            let info = snapshot().map(|s| platform_info::word_config_to_msg(&s.word_config));
            Ok(Some(
                MsgQueryVipWordConfigResp1022 { ret: 0, info }
                    .encode_to_vec()
                    .into(),
            ))
        })));
        n += 1;
    }

    // 1107 查公众号信息。
    {
        table.insert(Box::new(FnHandler::new(1107, move |_ctx, body| {
            let _req = GetWechatOfficialAccountInfoReq1107::decode(body).map_err(decode_err)?;
            let out = match snapshot() {
                Some(s) => platform_info::wechat_info_outcome(
                    s.wechat_channel.as_ref(),
                    &s.cid,
                    s.wechat_account.as_ref(),
                ),
                // 无配置 → 渠道不在表里 → 参数错误
                None => WechatInfoOutcome::ParamError,
            };
            Ok(Some(
                platform_info::build_wechat_resp(&out)
                    .encode_to_vec()
                    .into(),
            ))
        })));
        n += 1;
    }

    // 1110 查切换支付状态。
    {
        let w = world.clone();
        table.insert(Box::new(FnHandler::new(1110, move |ctx, body| {
            let _req = GetSwitchPaymentReq1110::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            // 原版 `player.getLv()` 与 `getStageByPlayerId(id).getCheckpointId()`。
            // 过渡期 MemWorld 有 level，但还没有关卡进度（`Stage` 模块未接），
            // 关卡按初始值 101 处理（`Stage.java:93` 的默认值）。
            let lv = w.peek(p, |b| b.level);
            let checkpoint_id = CHECKPOINT_INITIAL;
            let state = snapshot().map_or(0, |s| {
                platform_info::switch_payment_state(&s.switch_payment, lv, checkpoint_id)
            });
            Ok(Some(
                GetSwitchPaymentResp1110 {
                    ret: 0,
                    state: Some(state),
                }
                .encode_to_vec()
                .into(),
            ))
        })));
        n += 1;
    }

    n
}

/// `Stage.checkpointId` 的初始值（`Stage.java:93`）。
pub const CHECKPOINT_INITIAL: i32 = 101;
