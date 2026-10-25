//! 1008 看他人 / 1020 他人详情的接线。
//!
//! ## 与原版的差别（跨服部分）
//! 原版跨服走 `RemoteClient.sendRemoteMsg` 同步调用对端 game 服。`ddj-rpc` 的
//! game↔game 通道还没铺，所以跨服分支**一律 `COMMON_SERVER_BUSY(2)`** 并打 warn ——
//! 这与原版「对端不可达/超时」的表现一致（原版 catch 后同样是 ret=2）。
//!
//! 本服分支真实可用：查 PG 的 `player` 表。
//!
//! ## 判定顺序（字节码）
//! 1008：`targetServerId` → 同服？→ `getPlayerById` → 不存在则 NPE → ret=2；
//! 存在则 `friendScope = true`（硬编码 `iconst_1`，偏移 52）。
//! 1020：同构，但**没有** `friendScope` 这层改写。

use std::sync::Arc;

use ddj_core::Clock;
use ddj_pb::gen::player_message::{
    MsgOtherPlayer, MsgOtherPlayerDetail, PlayerViewOtherDetailReq1020,
    PlayerViewOtherDetailResp1020, PlayerViewOtherReq1008, PlayerViewOtherResp1008,
};
use ddj_registry::{HandlerTable, Reply};
use prost::Message;

use crate::player::{self, ViewLookup, ViewPlan};
use crate::runtime::{decode_err, AsyncHandler};

/// 本服角色查询。
#[async_trait::async_trait]
pub trait ViewRepo: Send + Sync {
    /// 按 playerId 查，返回 `(name, level, vip)`。
    async fn find(&self, id: i64) -> Option<(String, i64, i32)>;
}

struct Lookup {
    my_server_id: i64,
    id_range: i64,
    exists: bool,
}

impl ViewLookup for Lookup {
    fn target_server_id(&self, player_id: &str) -> i64 {
        player_id.parse::<i64>().map_or(-1, |v| v / self.id_range)
    }

    fn local_player_exists(&self, _player_id: &str) -> bool {
        self.exists
    }

    fn is_cross_player_id(&self, _player_id: &str) -> bool {
        false
    }
}

fn busy_1008() -> Reply {
    Reply::same(
        PlayerViewOtherResp1008 {
            ret: ddj_pb::cmd::RetCode::ServerBusy.as_i32(),
            msg_other_player: None,
        }
        .encode_to_vec(),
    )
}

fn busy_1020() -> Reply {
    Reply::same(
        PlayerViewOtherDetailResp1020 {
            ret: ddj_pb::cmd::RetCode::ServerBusy.as_i32(),
            msg_other_player: None,
        }
        .encode_to_vec(),
    )
}

/// 注册 2 条消息。
pub fn install(
    table: &mut HandlerTable,
    repo: Arc<dyn ViewRepo>,
    my_server_id: i64,
    id_range: i64,
    _clock: Arc<dyn Clock>,
) -> usize {
    let mut n = 0;

    // 1008
    {
        let r = repo.clone();
        table.insert(Box::new(AsyncHandler::new(1008, move |ctx, body| {
            let r = r.clone();
            async move {
                let req = PlayerViewOtherReq1008::decode(body).map_err(decode_err)?;
                let _me = ctx.require_player()?;
                let pid = req.player_id.clone().unwrap_or_default();
                let row = match pid.parse::<i64>() {
                    Ok(v) => r.find(v).await,
                    Err(_) => None,
                };
                let lk = Lookup {
                    my_server_id,
                    id_range,
                    exists: row.is_some(),
                };
                match player::view_other_plan(&pid, my_server_id, &lk) {
                    ViewPlan::Local { friend_scope } => {
                        let msg = row.map(|(name, lv, vip)| MsgOtherPlayer {
                            player_id: Some(pid.clone()),
                            name: Some(name),
                            lv: Some(lv),
                            vip_lv: Some(vip),
                            sex: Some(1),
                            ..Default::default()
                        });
                        let mut resp = PlayerViewOtherResp1008 {
                            ret: 0,
                            msg_other_player: msg,
                        };
                        let _ = friend_scope; // 原版硬编码 true，协议层暂无该字段
                        if resp.msg_other_player.is_none() {
                            resp.ret = ddj_pb::cmd::RetCode::ServerBusy.as_i32();
                        }
                        Ok(Some(Reply::same(resp.encode_to_vec())))
                    }
                    ViewPlan::Remote { target_server, .. } => {
                        tracing::warn!(target_server, "跨服看他人尚未接通，按原版超时路径回 ret=2");
                        Ok(Some(busy_1008()))
                    }
                    ViewPlan::Busy => Ok(Some(busy_1008())),
                }
            }
        })));
        n += 1;
    }

    // 1020
    {
        let r = repo.clone();
        table.insert(Box::new(AsyncHandler::new(1020, move |ctx, body| {
            let r = r.clone();
            async move {
                let req = PlayerViewOtherDetailReq1020::decode(body).map_err(decode_err)?;
                let _me = ctx.require_player()?;
                let pid = req.player_id.clone().unwrap_or_default();
                let row = match pid.parse::<i64>() {
                    Ok(v) => r.find(v).await,
                    Err(_) => None,
                };
                let lk = Lookup {
                    my_server_id,
                    id_range,
                    exists: row.is_some(),
                };
                match player::view_other_detail_plan(&pid, my_server_id, &lk) {
                    ViewPlan::Local { .. } => {
                        let msg = row.map(|(name, lv, vip)| MsgOtherPlayerDetail {
                            player_data: Some(MsgOtherPlayer {
                                player_id: Some(pid.clone()),
                                name: Some(name),
                                lv: Some(lv),
                                vip_lv: Some(vip),
                                sex: Some(1),
                                ..Default::default()
                            }),
                            ..Default::default()
                        });
                        let mut resp = PlayerViewOtherDetailResp1020 {
                            ret: 0,
                            msg_other_player: msg,
                        };
                        if resp.msg_other_player.is_none() {
                            resp.ret = ddj_pb::cmd::RetCode::ServerBusy.as_i32();
                        }
                        Ok(Some(Reply::same(resp.encode_to_vec())))
                    }
                    ViewPlan::Remote { target_server, .. } => {
                        tracing::warn!(target_server, "跨服看他人详情尚未接通，回 ret=2");
                        Ok(Some(busy_1020()))
                    }
                    ViewPlan::Busy => Ok(Some(busy_1020())),
                }
            }
        })));
        n += 1;
    }

    n
}
