//! 渠道好评 17101 / 17102 / 17103 的接线。
//!
//! 纯逻辑在 [`crate::favourable`]，本文件只做解码 → 调纯函数 → 改状态 → 编码回包。
//!
//! ## ⚠ 三条都不走 `ErrorResp_1`
//! 原版三个方法的签名都**没有** `throws HandlerException`，方法体内也没有
//! `throw`。失败一律塞进各自的 Resp：
//!
//! | cmd | 失败 | 帧号 |
//! |---|---|---|
//! | 17101 | 无失败分支 | 17101 |
//! | 17102 | `ret=0`（渠道不符）/ `2754`（模板缺失） | 17102 |
//! | 17103 | `3941`（渠道不符）/ `3942`（已评价） | 17103 |
//!
//! ## ⚠ 17102 渠道不符时回 `ret=0`
//! ```java
//! if (!FAVOURABLE_CHANNELS.contains(channelId)) return builder.setRet(0).build();
//! ```
//! 不是错误码 —— 客户端会当成"没发生过"。照抄。
//!
//! ## `CommentDB` 在打包数据里不存在
//! 445 张 CSV 里搜不到 `CommentDB` 实体，所以 `CommentDBCache.get(id)` 恒 `null`
//! → 17102 恒回 `2754`。这与原版线上一致（原版的 `CommentDB` 表也没打包）。

use std::sync::{Arc, OnceLock};

use ddj_core::Clock;
use ddj_pb::gen::sys_activity_message::{
    CloseFavourableWindowReq17102, CloseFavourableWindowResp17102, FeedBackReq17103,
    FeedBackResp17103, GetFavourableReq17101, GetFavourableResp17101, MsgFavourable,
};
use ddj_registry::HandlerTable;
use prost::Message;

use crate::favourable::{
    self, CommentTemplate, FavourableState, FeedbackOutcome, PlatformUrls,
};
use crate::runtime::{decode_err, FnHandler, MemWorld};

/// `PlatformServerModule` 的三个静态配置（`bf_config.configId` 19/20/21）。
static URLS: OnceLock<PlatformUrls> = OnceLock::new();

/// 注入平台配置（只第一次生效）。
pub fn install_urls(u: PlatformUrls) {
    let _ = URLS.set(u);
}

fn urls() -> &'static PlatformUrls {
    URLS.get_or_init(PlatformUrls::default)
}

/// `CommentDBCache`（`id → CommentTemplate`）。
static COMMENTS: OnceLock<Vec<CommentTemplate>> = OnceLock::new();

/// 注入好评模板（只第一次生效）。
pub fn install_comments(c: Vec<CommentTemplate>) {
    let _ = COMMENTS.set(c);
}

fn comment(id: i32) -> Option<CommentTemplate> {
    COMMENTS
        .get()
        .and_then(|v| v.iter().find(|c| c.id == id).copied())
}

/// `ConfigDBCache.CommentReward` 的注入点（`B-变量表-Config.csv:456`，id=456）。
static REWARD: OnceLock<Vec<(i32, i64)>> = OnceLock::new();

/// 注入好评奖励（只第一次生效）。
pub fn install_reward(r: Vec<(i32, i64)>) {
    let _ = REWARD.set(r);
}

fn reward() -> Vec<(i32, i64)> {
    REWARD.get().cloned().unwrap_or_default()
}

fn to_msg(list: &[favourable::FavourableMsg]) -> Vec<MsgFavourable> {
    list.iter()
        .map(|m| MsgFavourable {
            id: Some(m.id),
            count: Some(m.count),
        })
        .collect()
}

/// 注册 3 条消息，返回注册条数。
pub fn install(table: &mut HandlerTable, world: Arc<MemWorld>, clock: Arc<dyn Clock>) -> usize {
    let mut n = 0;

    // 17101 查询（无失败分支）
    {
        let w = world.clone();
        table.insert(Box::new(FnHandler::new(17101, move |ctx, body| {
            let _req = GetFavourableReq17101::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let state = w.peek(p, |b| b.favourable.clone());
            let u = urls();
            // `if (!isFeedBack) { ... }` —— 已评价过就只回 ret=0
            let (last_close_time, list, cafe, store) = if state.is_feedback {
                (None, Vec::new(), String::new(), String::new())
            } else {
                (
                    Some(state.last_close_time),
                    favourable::to_msg_list(&state.open_cnt),
                    u.cafe_url.clone(),
                    u.one_store_url.clone(),
                )
            };
            Ok(Some(
                GetFavourableResp17101 {
                    ret: 0,
                    last_close_time,
                    msg_favourable: to_msg(&list),
                    cafe_url: if cafe.is_empty() { None } else { Some(cafe) },
                    one_store_url: if store.is_empty() { None } else { Some(store) },
                }
                .encode_to_vec()
                .into(),
            ))
        })));
        n += 1;
    }

    // 17102 关窗（失败也在 Resp 里，帧号 17102）
    {
        let (w, c) = (world.clone(), clock.clone());
        table.insert(Box::new(FnHandler::new(17102, move |ctx, body| {
            let req = CloseFavourableWindowReq17102::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let id = req.id;
            let now = c.now_millis();
            let (state, channel_id) = w.peek(p, |b| (b.favourable.clone(), b.channel_id));
            let out = favourable::close_verdict(
                &state,
                &urls().channels,
                channel_id,
                comment(id),
                id,
                now,
            );
            if out.dirty {
                w.with(p, |b| {
                    b.favourable.last_close_time = out.last_close_time;
                    b.favourable.open_cnt = out.open_cnt.clone();
                });
            }
            let list = if out.ret.as_i32() == 0 {
                favourable::to_msg_list(&out.open_cnt)
            } else {
                Vec::new()
            };
            Ok(Some(
                CloseFavourableWindowResp17102 {
                    ret: out.ret.as_i32(),
                    last_close_time: Some(out.last_close_time),
                    msg_favourable: to_msg(&list),
                }
                .encode_to_vec()
                .into(),
            ))
        })));
        n += 1;
    }

    // 17103 评价（失败也在 Resp 里，帧号 17103）
    {
        let w = world.clone();
        table.insert(Box::new(FnHandler::new(17103, move |ctx, body| {
            let _req = FeedBackReq17103::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let (state, channel_id) = w.peek(p, |b| (b.favourable.clone(), b.channel_id));
            match favourable::feedback_verdict(&state, &urls().channels, channel_id, &reward()) {
                FeedbackOutcome::Reject(code) => Ok(Some(
                    FeedBackResp17103 {
                        ret: code.as_i32(),
                        msg_resources_gain: Vec::new(),
                    }
                    .encode_to_vec()
                    .into(),
                )),
                FeedbackOutcome::Accept { rewards } => {
                    w.with(p, |b| {
                        b.favourable.is_feedback = true;
                        for (id, num) in &rewards {
                            *b.bag.entry(*id).or_insert(0) += num;
                        }
                    });
                    Ok(Some(
                        FeedBackResp17103 {
                            ret: 0,
                            msg_resources_gain: crate::runtime::resources_to_msg(&rewards),
                        }
                        .encode_to_vec()
                        .into(),
                    ))
                }
            }
        })));
        n += 1;
    }

    let _ = FavourableState::default;
    n
}
