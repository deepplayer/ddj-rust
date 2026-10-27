//! 3 心跳 / 5 取随机名 的接线。
//!
//! 纯逻辑在 [`crate::engine`]，本文件只做解码 → 调纯函数 → 编码回包。
//!
//! ## 两条都是免登录
//! 原版 `MessageHandlerExtensionImpl` 的白名单 `[0,3,4,5,6,7,11,98]` 包含 3 与 5，
//! 所以它们的 `requires_login` 必须是 `false` —— 用 [`FnHandler::free`]。
//!
//! ## 5 的名字池
//! 池子由 `W_玩家名字_PlayerName.csv` 的姓氏 × 名字构成，启动时构建一次
//! （[`install_name_pool`]）。未注入时池子为空 → 走兜底 `["玩家"]`，
//! 与原版「表里没有数据」一致。
//!
//! ## 5 的语言字段缺失
//! 原版 `LanguageDBCache.LanguageType.get(lan)` 对未知语言返回 null，
//! 随后 `map.get(null)` NPE → **不回包**（客户端超时）。
//! 这里照抄「不回包」：返回 `Ok(None)`。日志里打 warn 便于排查。

use std::sync::{Arc, OnceLock};

use ddj_core::Clock;
use ddj_pb::gen::engine_message::{
    GetPlayerNamesReq5, GetPlayerNamesResp5, GetQueueUpInfoReq7, MsgTestData, TestSengReq11,
    TestSengResp11,
};
use ddj_registry::HandlerTable;
use prost::Message;

use crate::engine::{self, NameLanguage};
use crate::runtime::{decode_err, FnHandler, MemWorld};

/// 名字池（按语言分组，对应 `PlayerNameMgr.map`）。
#[derive(Debug, Default)]
pub struct NamePools {
    /// `zh_CN` 的候选名池。
    pub zh_cn: Vec<String>,
    /// `en` 的候选名池。
    pub en: Vec<String>,
    /// `vi_VN` 的候选名池。
    pub vi_vn: Vec<String>,
}

impl NamePools {
    fn get(&self, lang: NameLanguage) -> &[String] {
        match lang {
            NameLanguage::ZhCn => &self.zh_cn,
            NameLanguage::En => &self.en,
            NameLanguage::ViVn => &self.vi_vn,
        }
    }
}

static POOLS: OnceLock<Arc<NamePools>> = OnceLock::new();

/// 注入名字池（只第一次生效）。
pub fn install_name_pool(p: NamePools) {
    let _ = POOLS.set(Arc::new(p));
}

fn pools() -> Option<&'static Arc<NamePools>> {
    POOLS.get()
}

/// 注册 2 条消息，返回注册条数。
pub fn install(table: &mut HandlerTable, world: Arc<MemWorld>, clock: Arc<dyn Clock>) -> usize {
    let mut n = 0;

    // 3 心跳
    {
        let c = clock.clone();
        table.insert(Box::new(
            FnHandler::new(3, move |_ctx, body| {
                let _req = ddj_pb::gen::engine_message::HeartbeatConnectReq3::decode(body)
                    .map_err(decode_err)?;
                // 原版无分支：ret=0 + serverTime
                Ok(Some(
                    engine::heartbeat_resp(c.now_millis())
                        .encode_to_vec()
                        .into(),
                ))
            })
            // 白名单 [0,3,4,5,6,7,11,98] 之一
            .free(),
        ));
        n += 1;
    }

    // 5 取随机名
    {
        // rng 与签到共用同一套「确定性随机」思路，但独立种子避免相互影响
        let rng = Arc::new(std::sync::Mutex::new(ddj_core::DeterministicRng::new(
            0x5EED_0005,
        )));
        let _ = &world; // 5 不需要玩家状态
        table.insert(Box::new(
            FnHandler::new(5, move |_ctx, body| {
                let req = GetPlayerNamesReq5::decode(body).map_err(decode_err)?;
                let lan = req.lan.clone().unwrap_or_default();
                let Some(lang) = NameLanguage::parse(&lan) else {
                    // 原版此处 NPE → 不回包（客户端超时）。
                    // 照抄：返回 Ok(None)，但打 warn 便于定位。
                    tracing::warn!(
                        lan = %lan,
                        "取随机名：未知语言，原版此处 NPE 不回包（照抄）"
                    );
                    return Ok(None);
                };
                let pool = pools().map_or(&[][..], |p| p.get(lang));
                let guard = rng
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let names = engine::random_names(pool, &*guard);
                drop(guard);
                Ok(Some(
                    GetPlayerNamesResp5 { ret: 0, names }.encode_to_vec().into(),
                ))
            })
            .free(),
        ));
        n += 1;
    }

    // 7 排队信息
    {
        table.insert(Box::new(
            FnHandler::new(7, move |_ctx, body| {
                let _req = GetQueueUpInfoReq7::decode(body).map_err(decode_err)?;
                // 过渡期没有排队机制（`QUEUEUP_PLAYER_LIST` 恒空），
                // 等价于原版「队列空 + 不在队列」→ nowIndex=1, maxIndex=1。
                // 接上真实排队后把 snapshot 换成队列查询结果即可。
                Ok(Some(
                    crate::queue::queue_resp(crate::queue::QueueSnapshot {
                        len: 0,
                        index: None,
                    })
                    .encode_to_vec()
                    .into(),
                ))
            })
            .free(),
        ));
        n += 1;
    }

    // 11 测试消息（三条包）
    {
        let c = clock.clone();
        table.insert(Box::new(
            FnHandler::new(11, move |ctx, body| {
                let _req = TestSengReq11::decode(body).map_err(decode_err)?;
                let now = c.now_millis();

                // 包 1：TestSengResp_11，含 5 条固定测试数据
                let data: Vec<MsgTestData> = engine::test_data_entries()
                    .into_iter()
                    .map(|(t, d)| MsgTestData {
                        r#type: Some(t),
                        data: Some(d),
                    })
                    .collect();
                let first = TestSengResp11 { data };

                // 包 2、3：原版连续发两次同样的 HeartbeatConnectResp_3
                if let Some(sess) = ctx.session.as_ref() {
                    for _ in 0..2 {
                        sess.push(3, engine::heartbeat_resp(now).encode_to_vec().into());
                    }
                }
                // 返回第一个包（帧号沿用请求的 11）
                Ok(Some(first.encode_to_vec().into()))
            })
            .free(),
        ));
        n += 1;
    }

    n
}
