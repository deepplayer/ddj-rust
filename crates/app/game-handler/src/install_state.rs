//! 1100 客户端缓存回写的接线。
//!
//! 原版 `playerSaveNewPlayerDataReq` 全文只有一句
//! `player.setNewPlayerData(req.getPlayerCacheTempList())`，`catch(Exception)` 只写日志：
//! **零校验、零回包**。所以这里成功后也返回 `Ok(None)`，不是漏写。

use std::sync::Arc;

use ddj_core::Clock;
use ddj_pb::gen::player_message::PlayerSaveNewPlayerDataReq1100;
use ddj_registry::HandlerTable;
use prost::Message;

use crate::player::CacheTemp;
use crate::runtime::{decode_err, FnHandler, MemWorld};

/// 注册 1 条存档消息，返回注册条数。
pub fn install(table: &mut HandlerTable, world: Arc<MemWorld>, _clock: Arc<dyn Clock>) -> usize {
    table.insert(Box::new(FnHandler::new(1100, move |ctx, body| {
        let req = PlayerSaveNewPlayerDataReq1100::decode(body).map_err(decode_err)?;
        let p = ctx.require_player()?;
        let temps: Vec<CacheTemp> = req
            .player_cache_temp
            .iter()
            .map(|t| CacheTemp {
                key: t.key.unwrap_or(0),
                value: t.value.clone().unwrap_or_default(),
            })
            .collect();
        world.with(p, |b| b.new_player_data.apply(&temps));
        // 原版不回包。
        Ok(None)
    })));
    1
}
