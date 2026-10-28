//! 把已移植的处理链路注册进 `HandlerTable`。
//!
//! ## 为什么单独一层
//! 各业务模块（[`crate::login`]、[`crate::item`]、[`crate::sign`] …）都只暴露
//! **无 IO 的纯函数**，依赖通过 trait 注入，这样单测能覆盖全部分支。
//! 但真正的服务端需要把这些纯函数挂到 `cmdId → 处理器` 的表上，这一层负责：
//!
//! 1. 持有运行期依赖（[`login::ServerState`] 等）；
//! 2. 实现 [`ErasedHandler`]，把裸字节翻译成业务函数的入参；
//! 3. 统一把 `AppError` 交给管道层处理（原版对应 `HandlerException` 的出口）。
//!
//! ## 登录为什么不用 `MessageHandler`
//! `LoginReq_4` 的 body 是 **XXTEA + Base64** 密文（见 [`crate::login`]），
//! 直接 `prost::decode` 必定失败。所以登录处理器自己解析 body，
//! 走 [`ErasedHandler`] 而不是类型化的 [`ddj_registry::MessageHandler`]。

use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use ddj_core::{Clock, Result};
use ddj_registry::{ErasedHandler, HandlerCtx, HandlerTable, Reply};
use prost::Message;

use crate::handshake::{
    self, HandshakeStore, HandshakeVerdict, CMD_GET_ITEM, CMD_GET_MISSION, CMD_GET_PET,
};
use crate::login::{self, AccountLookup, ServerState};
use crate::player::{self, CreateCtx, CreateOutcome, CreatePlayerRepo, NewPlayerRow};
use crate::player_misc;
use crate::player_state::PlayerStateStore;
use crate::runtime::MemWorld;

/// 登录消息号（`LoginMessage.LoginReq_4`）。
pub const CMD_LOGIN: i32 = ddj_pb::cmd::login::LOGIN;

/// 创角消息号（`LoginMessage.CreatePlayerReq_6`）。
pub const CMD_CREATE_PLAYER: i32 = 6;

/// 登录（`cv = 4`）。
///
/// 原版：`LoginMessageHandler` → `WorkTaskPlayerSignIn`。
/// 本文档见 [`crate::login`] 的分支表。
pub struct LoginHandler {
    state: ServerState,
    lookup: Option<Arc<dyn AccountLookup>>,
    handshake: Option<Arc<dyn HandshakeStore>>,
    /// 过渡期内存态：登录成功后把 `channelId` / `cid` 回填进去。
    /// 未注册 `MemWorld` 时为 `None`（单测与纯逻辑路径）。
    world: Option<Arc<MemWorld>>,
}

impl LoginHandler {
    /// 用给定的服务器状态构造。
    #[must_use]
    pub fn new(state: ServerState) -> Self {
        Self {
            state,
            lookup: None,
            handshake: None,
            world: None,
        }
    }

    /// 挂上账号查询（服务端启动时用这个）。
    #[must_use]
    pub fn with_lookup(state: ServerState, lookup: Arc<dyn AccountLookup>) -> Self {
        Self {
            state,
            lookup: Some(lookup),
            handshake: None,
            world: None,
        }
    }

    /// 再挂上握手状态（服务端启动时用这个）。
    #[must_use]
    pub fn with_handshake(mut self, hs: Arc<dyn HandshakeStore>) -> Self {
        self.handshake = Some(hs);
        self
    }

    /// 挂上内存态（登录成功后回填 `channelId` / `cid`）。
    #[must_use]
    pub fn with_world(mut self, w: Arc<MemWorld>) -> Self {
        self.world = Some(w);
        self
    }
}

#[async_trait]
impl ErasedHandler for LoginHandler {
    fn cmd_id(&self) -> i32 {
        CMD_LOGIN
    }

    /// 登录包必须能在未登录状态下处理 —— 它就是用来建立登录态的。
    fn requires_login(&self) -> bool {
        false
    }

    async fn handle_raw(&self, ctx: &HandlerCtx, body: Bytes) -> Result<Option<Reply>> {
        let now = chrono::Utc::now().timestamp_millis();
        // 原版 `UtilsTools.random(1, 10000)`。这里用高精度时间戳取模，
        // 在同一毫秒内多次登录会重复，但原版 prng 也不是密码学强度。
        let loginvar = i32::try_from((now.unsigned_abs() % 10_000) + 1).unwrap_or(1);
        let (resp, player) = login::handle_with(
            ctx,
            &body,
            &self.state,
            self.lookup.as_deref(),
            now,
            loginvar,
            self.handshake.as_deref(),
        )
        .await?;
        // 登录成功 → 把角色绑进会话。之后 `player_id` 才有值，
        // 需要登录的消息（握手回执 16/17/18、背包等）才会放行。
        if let (Some((p, channel_id, cid, money, total_money)), Some(sess)) =
            (player, ctx.session.as_ref())
        {
            sess.bind_player(p);
            // `Player.channelId` / `Player.cid` 随会话落进 `MemWorld`，
            // 供 17301/17101/17081 等按渠道判定的消息读回（见 0006 迁移）。
            // `money` / `totalMoney` 同理：原版 `Player` 整行从库里加载，
            // 登录完就有值；移植版在这里补上（见 0007 迁移）。
            if let Some(w) = self.world.as_ref() {
                w.with(p, |b| {
                    b.channel_id = channel_id;
                    b.cid = cid;
                    b.money.clone_from(&money);
                    b.total_money.clone_from(&total_money);
                });
            }
        }
        Ok(Some(Bytes::from(resp.encode_to_vec()).into()))
    }
}

/// 把全部已移植的处理器挂到表上。
///
/// 返回实际注册的条数，便于启动日志核对「有多少条链路真正可达」。
///
/// # Errors
/// 目前不会失败；保留 `Result` 是为了后续需要读模板表时不改签名。
/// 1008/1020 的依赖（发号范围与所在服）。
pub struct ViewDeps {
    /// 本服角色查询；`None` 表示不注册看他人。
    pub repo: Option<Arc<dyn crate::install_view::ViewRepo>>,
    /// 当前 game 服 id。
    pub my_server_id: i64,
    /// `getPlayerIDRange()`。
    pub id_range: i64,
}

/// 注册全部业务处理器，返回注册条数。
pub fn register(
    table: &mut HandlerTable,
    state: ServerState,
    lookup: Option<Arc<dyn AccountLookup>>,
    create: Option<(Arc<dyn CreatePlayerRepo>, Arc<dyn player::FashionLookup>)>,
    handshake: Option<Arc<dyn HandshakeStore>>,
    stores: Option<(Arc<dyn PlayerStateStore>, Arc<dyn Clock>)>,
    runtime: Option<(Arc<MemWorld>, Arc<dyn Clock>)>,
    view: ViewDeps,
) -> Result<usize> {
    let mut n = 0;
    let server_id = state.server_id;
    let h = match lookup {
        Some(l) => {
            let h = LoginHandler::with_lookup(state, l);
            let h = match handshake.clone() {
                Some(hs) => h.with_handshake(hs),
                None => h,
            };
            match runtime.as_ref() {
                Some((w, _)) => h.with_world(w.clone()),
                None => h,
            }
        }
        None => LoginHandler::new(state),
    };
    table.insert(Box::new(h));
    n += 1;
    if let Some(hs) = handshake.clone() {
        for cmd in [CMD_GET_ITEM, CMD_GET_PET, CMD_GET_MISSION] {
            table.insert(Box::new(HandshakeHandler::new(hs.clone(), cmd)));
            n += 1;
        }
    }
    if let Some((repo, fashion)) = create {
        table.insert(Box::new(CreatePlayerHandler::new(repo, server_id, fashion)));
        n += 1;
    }
    if let Some((st, clock)) = stores {
        table.insert(Box::new(GuideHandler::new(st.clone(), clock)));
        n += 1;
        table.insert(Box::new(CacheSaveHandler::new(st.clone())));
        n += 1;
        table.insert(Box::new(CacheGetHandler::new(st)));
        n += 1;
    }
    // 背包 / 签到 / 邮件 走过渡期内存态（见 crate::runtime 的模块文档）。
    if let Some((world, clock)) = runtime {
        n += crate::install_amber::install(table, world.clone(), clock.clone());
        n += crate::install_anticheat::install(table, world.clone(), clock.clone());
        n += crate::install_bind::install(table, world.clone(), clock.clone());
        n += crate::install_cornucopia::install(table, world.clone(), clock.clone());
        n += crate::install_counter::install(table, world.clone(), clock.clone());
        n += crate::install_declare::install(table, world.clone(), clock.clone());
        n += crate::install_engine::install(table, world.clone(), clock.clone());
        n += crate::install_favourable::install(table, world.clone(), clock.clone());
        n += crate::install_pet::install(table, world.clone(), clock.clone());
        n += crate::install_platform::install(table, world.clone(), clock.clone());
        n += crate::install_pray::install(table, world.clone(), clock.clone());
        n += crate::install_vip::install(table, world.clone(), clock.clone());
        n += crate::install_item::install(table, world.clone(), clock.clone());
        n += crate::install_lantern_riddles::install(table, world.clone(), clock.clone());
        n += crate::install_log::install(table, world.clone(), clock.clone());
        n += crate::install_misc::install(table, world.clone(), clock.clone());
        n += crate::install_player::install(table, world.clone(), clock.clone());
        n += crate::install_ocean_trade::install(table, world.clone(), clock.clone());
        n += crate::install_second_psw::install(table, world.clone(), clock.clone());
        n += crate::install_shortcut::install(table, world.clone(), clock.clone());
        n += crate::install_sign::install(table, world.clone(), clock.clone());
        n += crate::install_special_msg::install(table, world.clone(), clock.clone());
        n += crate::install_trade::install(table, world.clone(), clock.clone());
        n += crate::install_travel::install(table, world.clone(), clock.clone());
        n += crate::install_state::install(table, world.clone(), clock.clone());
        if let Some(v) = view.repo {
            n += crate::install_view::install(
                table,
                v,
                view.my_server_id,
                view.id_range,
                clock.clone(),
            );
        }
        n += crate::install_mail::install(table, world, clock);
    }
    Ok(n)
}

/// 供 `game-core` 用的便捷包装（内部 `Arc`，避免每个处理器各存一份状态）。
#[must_use]
pub fn login_handler(state: ServerState) -> Arc<dyn ErasedHandler> {
    Arc::new(LoginHandler::new(state))
}

/// 创角（`cv = 6`）。
///
/// 原版 `WorkTaskCreatePlayer.run()` 的 17 个分支见 [`crate::player`]。
/// 这里只做三件事：查必要的外部数据 → 交给纯函数判定 → 落库并回包。
///
/// ## 为什么回包类型不固定
/// 分支 1/5/6 原版回的是 `LoginResp_4`（不是 `CreatePlayerResp_6`），
/// 所以本处理器直接实现 [`ErasedHandler`] 而不是 `MessageHandler`。
pub struct CreatePlayerHandler {
    repo: Arc<dyn CreatePlayerRepo>,
    server_id: i32,
    /// 一个只读的默认扮装表（启动时注入；空表 = 全部 `faceId` 走默认补齐）。
    fashion: Arc<dyn player::FashionLookup>,
}

impl CreatePlayerHandler {
    /// 构造。
    #[must_use]
    pub fn new(
        repo: Arc<dyn CreatePlayerRepo>,
        server_id: i32,
        fashion: Arc<dyn player::FashionLookup>,
    ) -> Self {
        Self {
            repo,
            server_id,
            fashion,
        }
    }
}

#[async_trait]
impl ErasedHandler for CreatePlayerHandler {
    fn cmd_id(&self) -> i32 {
        CMD_CREATE_PLAYER
    }

    fn requires_login(&self) -> bool {
        false
    }

    async fn handle_raw(&self, ctx: &HandlerCtx, body: Bytes) -> Result<Option<Reply>> {
        let req = ddj_pb::gen::engine_message::CreatePlayerReq6::decode(body)
            .map_err(|e| ddj_core::AppError::BadRequest(format!("创角包解码失败: {e}")))?;
        let name = req.name.clone().unwrap_or_default();

        // 原版分支 14 才查重名。这里提前查是为了把 `name_repeated` 作为入参传进
        // 纯函数 —— 输出完全一致，代价是非法请求多一次查询。
        let name_repeated = self.repo.name_exists(&name).await;
        let mut c = CreateCtx::healthy(self.server_id);
        c.name_repeated = name_repeated;
        c.register_num = self.repo.register_count().await;

        match player::evaluate(&req, &c, self.fashion.as_ref())? {
            CreateOutcome::Reject { ret, as_login_resp } => {
                let bytes = if as_login_resp {
                    login::build_resp(login::LoginOutcome::Reject(ret)).encode_to_vec()
                } else {
                    player::build_resp(ret).encode_to_vec()
                };
                tracing::info!(peer = %ctx.peer_ip, ret = ret.as_i32(), as_login = as_login_resp, "创角被拒");
                Ok(Some(Bytes::from(bytes).into()))
            }
            CreateOutcome::Accept { face_ids } => {
                let open_id = req.open_id.clone().unwrap_or_default();
                let now = chrono::Utc::now().timestamp_millis();
                // 发号 + 插入可能并发撞名，重试一次即可收敛
                // （原版靠 `synchronized (Player.class)` 串行化，这里靠唯一键）。
                for attempt in 0..2 {
                    let player_id = self.repo.next_player_id(self.server_id).await;
                    let row = NewPlayerRow {
                        player_id,
                        open_id: open_id.clone(),
                        server_id: self.server_id,
                        name: name.clone(),
                        ico: req.ico.clone().unwrap_or_default(),
                        sex: req.sex.unwrap_or_default().to_string(),
                        now_ms: now,
                        channel_id: req.channel_id.unwrap_or(0),
                        cid: req.cid.clone().unwrap_or_default(),
                    };
                    if self.repo.create(&row).await {
                        tracing::info!(
                            peer = %ctx.peer_ip,
                            player_id,
                            face_ids = ?face_ids,
                            attempt,
                            "创角成功"
                        );
                        return Ok(Some(
                            Bytes::from(
                                player::build_resp(ddj_pb::cmd::RetCode::Ok).encode_to_vec(),
                            )
                            .into(),
                        ));
                    }
                }
                // 两次都撞 → 原版 `createPlayer` 返回 null → PLAYER_ERROR_3(103)
                tracing::warn!(peer = %ctx.peer_ip, name = %name, "创角落库连续冲突");
                Ok(Some(
                    Bytes::from(
                        player::build_resp(ddj_pb::cmd::RetCode::NameDuplicate).encode_to_vec(),
                    )
                    .into(),
                ))
            }
        }
    }
}

/// 握手回执：cmd 16 / 17 / 18 三合一。
///
/// 三者在原版是三个方法、三份逐指令相同的代码，且都 `return void`
/// （不发包）。所以一个结构体挂三个消息号即可。
pub struct HandshakeHandler {
    store: Arc<dyn HandshakeStore>,
    cmd: i32,
}

impl HandshakeHandler {
    /// 构造。
    #[must_use]
    pub fn new(store: Arc<dyn HandshakeStore>, cmd: i32) -> Self {
        Self { store, cmd }
    }
}

#[async_trait]
impl ErasedHandler for HandshakeHandler {
    fn cmd_id(&self) -> i32 {
        self.cmd
    }

    async fn handle_raw(&self, ctx: &HandlerCtx, body: Bytes) -> Result<Option<Reply>> {
        // 三个 Req 的第一个字段都是 `optional string itemId = 1`，
        // 用最小的解码器取它，避免为一个字段依赖三种生成类型。
        let item_id = decode_first_string(body.as_ref())
            .ok_or_else(|| ddj_core::AppError::BadRequest("握手包缺少 itemId".to_owned()))?;
        let Some(player) = ctx.player_id else {
            // 原版靠 `getChannelPlayerByPlayerId(p.getId())` 拿会话，
            // 没有会话就是 NPE → 外层捕获。这里直接报错。
            return Err(ddj_core::AppError::Unauthorized);
        };
        self.store.set_client_password(player, &item_id);
        let verdict = handshake::verify(self.store.as_ref(), player);
        // 原版只打 log.debug("客户端发过来的密码：" + itemId)；
        // 这里把比对结果也带上，便于定位「算法错一位」这类问题。
        tracing::debug!(
            player = player.get(),
            cmd = self.cmd,
            password = %item_id,
            ?verdict,
            "客户端发过来的密码"
        );
        if verdict == HandshakeVerdict::Mismatch {
            tracing::warn!(
                player = player.get(),
                cmd = self.cmd,
                "握手密码与服务端留底不一致（原版静默，此处仅告警）"
            );
        }
        Ok(None)
    }
}

/// 取 protobuf 第一个字段（若为 string）的值。
fn decode_first_string(buf: &[u8]) -> Option<String> {
    // tag = (field 1, wire type 2) = 0x0A；长度是 varint。
    if buf.first() != Some(&0x0A) {
        return None;
    }
    let mut i = 1_usize;
    let mut len = 0_usize;
    let mut shift = 0_u32;
    loop {
        let b = *buf.get(i)?;
        i += 1;
        len |= usize::from(b & 0x7F) << shift;
        if b & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift > 28 {
            return None;
        }
    }
    let end = i.checked_add(len)?;
    std::str::from_utf8(buf.get(i..end)?)
        .ok()
        .map(str::to_owned)
}

/// 1103 引导存档。
pub const CMD_SAVE_GUIDE: i32 = 1103;

/// 1111 存客户端缓存。
pub const CMD_SAVE_CACHE: i32 = 1111;

/// 1112 读客户端缓存。
pub const CMD_GET_CACHE: i32 = 1112;

/// 1103：`playerSaveGuideIdReq` —— **不回包**，异常只打日志。
pub struct GuideHandler {
    stores: Arc<dyn PlayerStateStore>,
    clock: Arc<dyn Clock>,
}

impl GuideHandler {
    /// 构造。
    #[must_use]
    pub fn new(stores: Arc<dyn PlayerStateStore>, clock: Arc<dyn Clock>) -> Self {
        Self { stores, clock }
    }
}

#[async_trait]
impl ErasedHandler for GuideHandler {
    fn cmd_id(&self) -> i32 {
        CMD_SAVE_GUIDE
    }

    async fn handle_raw(&self, ctx: &HandlerCtx, body: Bytes) -> Result<Option<Reply>> {
        let player = ctx.require_player()?;
        let req = ddj_pb::gen::player_message::ClientSaveGuideIdReq1103::decode(body)
            .map_err(|e| ddj_core::AppError::BadRequest(format!("引导存档包解码失败: {e}")))?;
        let saved = player_misc::save_guide_info(
            req.guide_id.unwrap_or_default(),
            req.next_id.unwrap_or_default(),
            req.is_pastt.unwrap_or_default(),
            // 原版存的是 `player.dbId`；本项目里 `player.player_id` 就是库里那行。
            player.get(),
            self.clock.now_millis(),
        );
        // 原版：`EventLogSendRecord.guide(player, isPast, guideId, nextId)`
        tracing::info!(
            target: "eventlog",
            event = "guide",
            player = %player,
            is_past = saved.event.is_past,
            guide_id = saved.event.guide_id,
            next_id = saved.event.next_id,
            "引导节点存档"
        );
        self.stores.save_guide(player, saved.info).await?;
        Ok(None)
    }
}

/// 1111：`PlayerSaveCacheDataReq` —— 逐键合并，**不回包**。
pub struct CacheSaveHandler {
    stores: Arc<dyn PlayerStateStore>,
}

impl CacheSaveHandler {
    /// 构造。
    #[must_use]
    pub fn new(stores: Arc<dyn PlayerStateStore>) -> Self {
        Self { stores }
    }
}

#[async_trait]
impl ErasedHandler for CacheSaveHandler {
    fn cmd_id(&self) -> i32 {
        CMD_SAVE_CACHE
    }

    async fn handle_raw(&self, ctx: &HandlerCtx, body: Bytes) -> Result<Option<Reply>> {
        let player = ctx.require_player()?;
        let req = ddj_pb::gen::player_message::PlayerSaveCacheDataReq1111::decode(body)
            .map_err(|e| ddj_core::AppError::BadRequest(format!("缓存存档包解码失败: {e}")))?;
        let items: Vec<(i32, String)> = req
            .player_cache_temp
            .iter()
            .map(|t| {
                (
                    t.key.unwrap_or_default(),
                    t.value.clone().unwrap_or_default(),
                )
            })
            .collect();
        let n = items.len();
        self.stores.cache_put(player, items).await?;
        tracing::debug!(player = %player, count = n, "客户端缓存合并");
        Ok(None)
    }
}

/// 1112：`GetPlayerCacheDataReq` —— 有回包。
pub struct CacheGetHandler {
    stores: Arc<dyn PlayerStateStore>,
}

impl CacheGetHandler {
    /// 构造。
    #[must_use]
    pub fn new(stores: Arc<dyn PlayerStateStore>) -> Self {
        Self { stores }
    }
}

#[async_trait]
impl ErasedHandler for CacheGetHandler {
    fn cmd_id(&self) -> i32 {
        CMD_GET_CACHE
    }

    async fn handle_raw(&self, ctx: &HandlerCtx, body: Bytes) -> Result<Option<Reply>> {
        let player = ctx.require_player()?;
        let req = ddj_pb::gen::player_message::GetPlayerCacheDataReq1112::decode(body)
            .map_err(|e| ddj_core::AppError::BadRequest(format!("读缓存包解码失败: {e}")))?;
        let got = self.stores.cache_get(player, req.key.clone()).await?;
        let resp = ddj_pb::gen::player_message::GetPlayerCacheDataResp1112 {
            ret: 0,
            player_cache_temp: got
                .into_iter()
                .map(|(k, v)| ddj_pb::gen::player_message::PlayerCacheTemp {
                    key: Some(k),
                    value: Some(v),
                })
                .collect(),
        };
        Ok(Some(Bytes::from(resp.encode_to_vec()).into()))
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use super::*;

    fn ctx() -> HandlerCtx {
        HandlerCtx {
            session_id: 1,
            player_id: None,
            peer_ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
            cmd_id: CMD_LOGIN,
            session: None,
        }
    }

    #[test]
    fn login_handler_reports_cmd_and_free_login() {
        let h = LoginHandler::new(ServerState::healthy(1));
        assert_eq!(h.cmd_id(), CMD_LOGIN);
        assert!(!h.requires_login(), "登录包必须免登录");
    }

    #[test]
    fn register_counts_login() {
        let mut t = HandlerTable::empty();
        let n = register(
            &mut t,
            ServerState::healthy(1),
            None,
            None,
            None,
            None,
            None,
            ViewDeps {
                repo: None,
                my_server_id: 1,
                id_range: 1_000_000,
            },
        )
        .expect("注册");
        assert_eq!(n, 1);
        assert!(t.get(CMD_LOGIN).is_some(), "登录应已入表");
        assert!(t.get(11002).is_none(), "未移植的链路不应凭空出现");
    }

    /// 装配点守卫。
    ///
    /// 曾经出过一次事故：重构 `handlers.rs` 时把 `install_travel` 那一行整行删掉，
    /// 而 `install_travel.rs` 本身仍编过、单测仍绿 —— 只有 e2e 冒烟能发现。
    /// 这里把「每个已接线模块都必须真的被 register 调用」钉成断言，
    /// 让遗忘装配在 `cargo test` 阶段就红。
    #[tokio::test]
    async fn register_wires_every_ported_module() {
        let mut t = HandlerTable::empty();
        let world = MemWorld::new();
        let clock: Arc<dyn Clock> = Arc::new(TestClock(1_700_000_000_000));
        let n = register(
            &mut t,
            ServerState::healthy(1),
            None,
            None,
            None,
            None,
            Some((world, clock)),
            ViewDeps {
                repo: None,
                my_server_id: 1,
                id_range: 1_000_000,
            },
        )
        .expect("注册");
        // 已移植且必须入表的消息（每接一批就往这里加）。
        let must = [
            15002, 15004, 15009, // 游历
            37603, 37604, 37605, 37607, // 猜灯谜
            41001, 41002, 41003, // 远洋贸易
            17151, 17152, 17153, // 防骗手册
            8001, 8002, // 计数器恢复
            17071, 17072, // 焚香祈愿
            17101, 17102, 17103, // 渠道好评
            17301, 17302, 17303, // 保存桌面
            19002, 19003, 19004, 19005, // 通商
            1109,  // 客户端特约
        ];
        for cmd in must {
            assert!(
                t.get(cmd).is_some(),
                "cmd {cmd} 未入表 —— 忘了在 register 里装配"
            );
        }
        assert!(
            n >= must.len(),
            "register 只接了 {n} 个，不到 {must_len}",
            must_len = must.len()
        );
    }

    /// 固定时钟，用于装配测试。
    #[derive(Debug)]
    struct TestClock(i64);

    impl Clock for TestClock {
        fn now(&self) -> chrono::DateTime<chrono::Utc> {
            chrono::DateTime::from_timestamp_millis(self.0).expect("合法毫秒")
        }
    }

    #[tokio::test]
    async fn login_handler_rejects_garbage_body() {
        let h = LoginHandler::new(ServerState::healthy(1));
        let err = h
            .handle_raw(&ctx(), Bytes::from_static(b"not-a-login-packet"))
            .await;
        assert!(err.is_err(), "非法密文必须报错而不是回默认响应");
    }
}
