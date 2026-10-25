//! 角色属性 2 条消息的接线（1002 改名 / 1003 升级）。
//!
//! ## 过渡期缺的输入
//! - 1002 的 `name_repeated` 需要全局名字表（原版 `DBPlayerModule.repeated`）；
//! - 1002 的 `has_card_55` / `has_change_name_price` 需要资源系统；
//! - 1003 的等级模板需要 `S-身份表-Status.csv`。
//!
//! 这三处都先用「保守默认」顶住，业务判定本身（[`player::rename_verdict`] /
//! [`player::level_up`]）仍是审计过的那一份，接上数据源后只换入参。

use std::sync::Arc;

use ddj_core::Clock;
use ddj_pb::gen::player_message::UpdateNameReq1002;
use ddj_registry::HandlerTable;
use ddj_registry::Reply;
use prost::Message;

use crate::player::{self, LevelLookup, LevelTemplate, LevelUpOutcome, RenameCtx, RenameOutcome};
use crate::runtime::{decode_err, FnHandler, MemWorld};

/// 身份最高等级（`S-身份表-Status.csv` 共 24 级）。
const MAX_LEVEL: i32 = 24;

/// 过渡期的升级经验门槛：1 级需要 100（真实值来自 `StatusDB.playerExp`）。
const EXP_PER_LEVEL: i64 = 100;

/// 过渡期等级表：只在 `1..=MAX_LEVEL` 内给出模板。
struct MemLevels;

impl LevelLookup for MemLevels {
    fn template(&self, level: i32) -> Option<LevelTemplate> {
        if (1..=MAX_LEVEL).contains(&level) {
            Some(LevelTemplate {
                id: level,
                player_exp: EXP_PER_LEVEL,
            })
        } else {
            None
        }
    }
}

/// 注册 2 条角色属性消息，返回注册条数。
pub fn install(table: &mut HandlerTable, world: Arc<MemWorld>, clock: Arc<dyn Clock>) -> usize {
    let mut n = 0;

    // 1002 改名。
    {
        let (w, c) = (world.clone(), clock.clone());
        table.insert(Box::new(FnHandler::new(1002, move |ctx, body| {
            let req = UpdateNameReq1002::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let new_name = req.name.clone().unwrap_or_default();
            let (old_name, used, forbid) = w.peek(p, |b| {
                (
                    b.name.clone(),
                    b.update_name_used,
                    b.forbid_reset_name_until,
                )
            });
            let cx = RenameCtx {
                now: c.now_millis(),
                forbid_reset_name_until: forbid,
                chat_off: false,
                is_vn_sdk: false,
                vn_name_matches: true,
                name_space_on: false,
                nick_name_length: player::NICK_NAME_LENGTH,
                dirty_word: false,
                name_repeated: false,
                update_name_used: used,
                old_had_dot_r: old_name.contains(".R"),
                has_card_55: false,
                has_change_name_price: false,
            };
            let out = player::rename_verdict(&new_name, &old_name, &cx);
            if matches!(&out, RenameOutcome::Accept(_)) {
                w.with(p, |b| {
                    b.name = new_name;
                    b.update_name_used = true;
                });
            }
            Ok(Some(Reply::same(
                player::build_rename_resp(out).encode_to_vec(),
            )))
        })));
        n += 1;
    }

    // 1003 主动升级。
    {
        let (w, c) = (world.clone(), clock.clone());
        table.insert(Box::new(FnHandler::new(1003, move |ctx, _body| {
            let p = ctx.require_player()?;
            let (lv, exp) = w.peek(p, |b| (b.level, b.exp));
            let out = player::level_up(lv, exp, c.now_millis(), &MemLevels);
            let new_level = match &out {
                LevelUpOutcome::Accept(plan) => Some(plan.new_level),
                _ => None,
            };
            let resp = player::build_level_up_resp(out);
            let Some(resp) = resp else {
                // 满级：原版 `return`，一个包都不发。
                return Ok(None);
            };
            if let Some(level) = new_level {
                w.with(p, |b| {
                    b.level = level;
                    b.exp = 0;
                });
            }
            Ok(Some(Reply::same(resp.encode_to_vec())))
        })));
        n += 1;
    }

    n
}
