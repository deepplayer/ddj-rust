//! 1018 出站宣言 / 1019 看他人宣言 的接线。
//!
//! 对应 Java：`PlayerMessageHandler.MsgSetMammonDeclareReq_1018` / `MsgGetPlayerDeclareReq_1019`。
//! 纯逻辑在 [`crate::declare`]，本文件只做解码 → 调纯函数 → 编码回包。
//!
//! ## 两处照抄的原版 bug
//! 1. **1018 的 catch 回 `UpPlayerLvResp_1003`**（不是 1018 自己的 Resp）。
//! 2. **1019 本服分支一条请求发两个包**。
//!
//! 详见 [`crate::declare`] 模块文档。

use std::sync::Arc;

use ddj_core::Clock;
use ddj_pb::gen::player_message::{
    MsgGetPlayerDeclareReq1019, MsgGetPlayerDeclareResp1019, MsgSetMammonDeclareReq1018,
    MsgSetMammonDeclareResp1018, UpPlayerLvResp1003,
};
use ddj_registry::HandlerTable;
use prost::Message;

use crate::declare::{self, DeclareCtx, DeclareOutcome, GetDeclareOutcome};
use crate::runtime::{decode_err, FnHandler, MemWorld};

/// 注册 2 条宣言消息，返回注册条数。
pub fn install(table: &mut HandlerTable, world: Arc<MemWorld>, _clock: Arc<dyn Clock>) -> usize {
    let mut n = 0;

    // 1018 设置宣言。
    {
        let w = world.clone();
        table.insert(Box::new(FnHandler::new(1018, move |ctx, body| {
            let req = MsgSetMammonDeclareReq1018::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let dec = req.dec.clone().unwrap_or_default();

            // 过渡期：`FashionModel.isActivateFashionByType(3)` 与
            // `WxMsgSecCheck.checkDirtyWord` 都还没有数据源。
            // 按「时装已解锁、无敏感词」放行，让长度分支可以被打到；
            // 接上时装/敏感词服务后只替换这两个字段的取值。
            let c = DeclareCtx {
                fashion_unlocked: true,
                dirty_word: false,
                ..DeclareCtx::default()
            };
            let out = declare::set_declare_verdict(&dec, &c);
            match out {
                DeclareOutcome::Accept => {
                    // `Player.setManifesto(dec)`：赋值 + 刷离线数据 + save()
                    w.with(p, |b| b.manifesto = declare::apply_set_manifesto(&dec).unwrap_or_default());
                    Ok(Some(
                        MsgSetMammonDeclareResp1018 { ret: 0 }.encode_to_vec().into(),
                    ))
                }
                DeclareOutcome::Resp(code) => Ok(Some(
                    MsgSetMammonDeclareResp1018 { ret: code.as_i32() }
                        .encode_to_vec()
                        .into(),
                )),
                // 原版 catch(Exception) 回的是 `UpPlayerLvResp_1003`
                DeclareOutcome::UpPlayerLvBusy => Ok(Some(
                    UpPlayerLvResp1003 {
                        ret: ddj_pb::cmd::RetCode::ServerBusy.as_i32(),
                    }
                    .encode_to_vec()
                    .into(),
                )),
            }
        })));
        n += 1;
    }

    // 1019 查宣言（看他人）。
    {
        let w = world.clone();
        table.insert(Box::new(FnHandler::new(1019, move |ctx, body| {
            let req = MsgGetPlayerDeclareReq1019::decode(body).map_err(decode_err)?;
            let me = ctx.require_player()?;
            let target = req.player_id.clone().unwrap_or_default();

            // 原版 `LogicUtils.getTargetGsServerIdOfPlayer(playerId)` + 与本服比较。
            // 过渡期 MemWorld 只有本服玩家：目标就是自己时算本服，否则走跨服分支
            // （跨服异常 → dec 空、ret=0，与原版 catch 后 setRet(0) 的表现一致）。
            let self_id = me.get().to_string();
            let same_server = target == self_id;

            let manifesto: Option<String> = if same_server {
                Some(w.peek(me, |b| b.manifesto.clone()))
            } else {
                // 跨服通道（`ddj-rpc` game↔game）未铺，取不到 → None。
                None
            };

            let GetDeclareOutcome { dec, send_twice } =
                declare::get_declare_outcome(same_server, manifesto.as_deref());

            let resp = MsgGetPlayerDeclareResp1019 {
                ret: 0,
                dec: Some(dec),
            };

            // 本服分支：原版先 `player.sendMessage(builder.build())` 一次，
            // 末尾再 `player.sendMessage(builder.build())` 一次 —— 两个包内容相同。
            // 第一份走主动推送（与请求回包并列），第二份是返回值。
            if send_twice {
                if let Some(sess) = ctx.session.as_ref() {
                    sess.push(1019, resp.encode_to_vec().into());
                }
            }
            Ok(Some(resp.encode_to_vec().into()))
        })));
        n += 1;
    }

    n
}
