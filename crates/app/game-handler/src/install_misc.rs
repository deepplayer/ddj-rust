//! 角色周边 4 条小功能的接线（1009 / 1010 / 1015 / 1024）。
//!
//! ## 过渡期的保守默认
//! 这 4 条都依赖外部资源，业务函数已审计，入参先用默认值顶住：
//!
//! - 1009 `GsGiftManager` 查不到任何 key → 走 `PLAYER_ERROR_10(110)`；
//! - 1010 `getAddressInfo` 视为有地址、两种扣费都视为充足；
//! - 1015 没有 `SystemOpenDB` 模板 → `ret=2`（原版 NPE 被 catch）；
//! - 1024 改性别扣费视为成功。

use std::sync::Arc;

use ddj_core::Clock;
use ddj_pb::gen::player_message::{
    MsgUpdateSexReq1024, MsgUpdateSexResp1024, PlayerUseGiftKeyReq1009, PlayerUseGiftKeyResp1009,
    SetLocationReq1010, SetLocationResp1010, SystemOpenReq1015, SystemOpenResp1015,
};
use ddj_registry::HandlerTable;
use ddj_registry::Reply;
use prost::Message;

use crate::player_misc::{
    self, GiftKeyOutcome, LocationCost, LocationCtx, SetLocationOutcome, SystemOpenOutcome,
    UpdateSexOutcome,
};
use crate::runtime::{decode_err, FnHandler, MemWorld};

/// 过渡期扣费：一律视为资源充足。
struct MemLocationCost;

impl LocationCost for MemLocationCost {
    fn charge_positioning(&mut self) -> bool {
        true
    }

    fn charge_item_54(&mut self) -> bool {
        true
    }
}

/// 注册 4 条角色周边消息，返回注册条数。
pub fn install(table: &mut HandlerTable, world: Arc<MemWorld>, clock: Arc<dyn Clock>) -> usize {
    let mut n = 0;

    // 1009 使用礼包码。
    {
        table.insert(Box::new(FnHandler::new(1009, move |ctx, body| {
            let req = PlayerUseGiftKeyReq1009::decode(body).map_err(decode_err)?;
            ctx.require_player()?;
            let key = req.gift_key.clone().unwrap_or_default();
            // 过渡期 GsGiftManager 为空 → exists 恒 false。
            let out = player_misc::use_gift_key(&key, false);
            let ret = match out {
                GiftKeyOutcome::Reject(code) => code.as_i32(),
                GiftKeyOutcome::Accept => 0,
            };
            Ok(Some(Reply::same(
                PlayerUseGiftKeyResp1009 {
                    ret,
                    ..Default::default()
                }.encode_to_vec(),
            )))
        })));
        n += 1;
    }

    // 1010 改定位。
    {
        let w = world.clone();
        table.insert(Box::new(FnHandler::new(1010, move |ctx, body| {
            let req = SetLocationReq1010::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let city = req.city_id;
            let kind = req.r#type.unwrap_or(0);
            let (reset_times, _old_city) = w.peek(p, |b| (b.reset_address_times, b.city_id));
            let cx = LocationCtx {
                city_id: city,
                reset_times,
                address_exists: true,
            };
            let out = player_misc::set_location(city, kind, &cx, &mut MemLocationCost);
            let ret = match &out {
                SetLocationOutcome::Reject(code) => code.as_i32(),
                SetLocationOutcome::Accept { .. } => {
                    if let SetLocationOutcome::Accept {
                        new_city_id,
                        reset_times,
                    } = out
                    {
                        w.with(p, |b| {
                            b.city_id = new_city_id;
                            b.reset_address_times = reset_times;
                        });
                    }
                    0
                }
            };
            Ok(Some(Reply::same(SetLocationResp1010 { ret }.encode_to_vec())))
        })));
        n += 1;
    }

    // 1015 系统开启。
    {
        table.insert(Box::new(FnHandler::new(1015, move |ctx, body| {
            let req = SystemOpenReq1015::decode(body).map_err(decode_err)?;
            ctx.require_player()?;
            let system_id = req.system_id.unwrap_or(0);
            // 过渡期没有 SystemOpenDB 模板。
            let out = player_misc::system_open(system_id, None);
            let ret = match out {
                SystemOpenOutcome::Accept { .. } => 0,
                SystemOpenOutcome::Reject(code) => code.as_i32(),
            };
            Ok(Some(Reply::same(SystemOpenResp1015 { ret }.encode_to_vec())))
        })));
        n += 1;
    }

    // 1024 改性别。
    {
        let w = world.clone();
        let _ = &clock;
        table.insert(Box::new(FnHandler::new(1024, move |ctx, body| {
            let req = MsgUpdateSexReq1024::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let sex = req.sex.unwrap_or(0);
            let mut cost = || true;
            let out = player_misc::update_sex(sex, &mut cost);
            let ret = match &out {
                UpdateSexOutcome::Reject(code) => code.as_i32(),
                UpdateSexOutcome::Accept { sex } => {
                    let value = *sex;
                    w.with(p, |b| b.sex = value);
                    0
                }
            };
            Ok(Some(Reply::same(
                MsgUpdateSexResp1024 { ret }.encode_to_vec(),
            )))
        })));
        n += 1;
    }

    n
}
