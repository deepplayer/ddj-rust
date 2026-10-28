//! 登录处理器
//!
//! 对应 Java：`EngineMessageHandler.loginReq` → `WorkTaskPlayerSignIn.run()`。
//!
//! ## 原流程（反汇编 `WorkTaskPlayerSignIn.run()` 还原）
//! ```text
//! 1. serverId 与本服不符         → ret = COMMON_SERVER_BUSY(2)
//! 2. 服务器维护中                 → ret = COMMON_SERVER_WEIHU(1)
//! 3. 未开服                       → ret = COMMON_SERVER_NO_OPEN(23)
//! 4. 客户端版本 < 服务端要求       → ret = COMMON_SERVER_VERSION_NO(99)
//! 5. 白名单限制此区服             → ret = COMMON_LOGIN_FORBIDDEN(39)
//! 6. 账号被封                     → ret = COMMON_SERVER_NOT_SIGNIN(10)
//! 7. 该 channel 已登录过           → ret = PLAYER_ERROR_1(101)
//! 8. 人数已满                     → ret = COMMON_SERVER_MAX_PLAYER_NUM(4)
//! 9. 查不到角色（需创角）          → ret=0, ok=false, 附带可用名字
//! 10. 成功                         → ret=0, ok=true, 带 serverTime 等
//! ```
//! 每一步回包后**立即 return**，不做后续处理 —— 这里保持同样的短路结构。
//!
//! ## 加密
//! **只有 `msgId == 4` 走加密**：`XXeta.decrypt(body, SIGN_KEY)` 后再
//! `Base64.decode`。`SIGN_KEY` 是厂家写死在两端的同一串 `05:&()^&*abddzMQ`。
//! 客户端 proto 注释写得很直白：`sign`「为空，测试登录」。

use async_trait::async_trait;

use crate::handshake::HandshakeStore;
use ddj_codec::{unpack_login_body, LoginBodyError, SIGN_KEY};
use ddj_core::{AppError, PlayerId, Result};
use ddj_pb::gen::engine_message::{LoginReq4, LoginResp4};
use ddj_pb::RetCode;
use ddj_registry::HandlerCtx;

/// 登录请求解密后的明文 protobuf。
///
/// 单独封装的原因：加密是**传输层**的事，解完就该拿到一个和其它消息
/// 没有区别的 `LoginReq4`，后面的逻辑不该再关心密文。
pub struct LoginRequest {
    /// 解码后的请求体。
    pub req: LoginReq4,
}

impl LoginRequest {
    /// 从**密文 body** 解出登录请求。
    ///
    /// 这是全项目唯一需要特殊处理的消息：其余消息的 body 直接就是 protobuf。
    ///
    /// # Errors
    /// [`LoginBodyError`] 的三种情形，或 protobuf 格式错误。
    pub fn decrypt(body: &[u8]) -> Result<Self> {
        let plain = unpack_login_body(body, &SIGN_KEY).map_err(to_app_error)?;
        let req = <LoginReq4 as prost::Message>::decode(plain.as_slice())
            .map_err(|e| AppError::BadRequest(format!("LoginReq_4 解码失败: {e}")))?;
        Ok(Self { req })
    }
}

/// 登录校验的输入：服务端自己的状态，与请求无关。
///
/// 单独提出来是为了能单测——不起服务、不连数据库就能验证全部分支。
#[derive(Debug, Clone)]
pub struct ServerState {
    /// 本服 ID。
    pub server_id: i32,
    /// 是否维护中。
    pub maintaining: bool,
    /// 是否已开服。
    pub opened: bool,
    /// 服务端要求的最低客户端版本号。
    pub required_version: i32,
    /// 当前在线人数。
    pub online: u32,
    /// 人数上限。
    pub max_online: u32,
}

impl ServerState {
    /// 一个「正常运营中」的服，供测试与本地起服用。
    #[must_use]
    pub const fn healthy(server_id: i32) -> Self {
        Self {
            server_id,
            maintaining: false,
            opened: true,
            required_version: 0,
            online: 0,
            max_online: 10_000,
        }
    }
}

/// 一次登录的判定结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginOutcome {
    /// 拒绝，带错误码。
    Reject(RetCode),
    /// 无角色，需要创角（客户端接着走 `CreatePlayerReq_6`）。
    NeedCreate {
        /// 推荐名字（一次给多个，减少通讯）。
        names: Vec<String>,
    },
    /// 登录成功。
    Accepted {
        /// 角色 ID。
        player_id: PlayerId,
        /// 服务器当前毫秒时间。
        server_time: i64,
        /// 开服时间。
        open_server_time: i64,
        /// 角色创建时间。
        create_player_time: i64,
        /// 封禁截止（-1 永久封禁）。
        banned_time: i64,
        /// 本次握手的挑战值（`LoginResp_4.loginvar`，1..=10000）。
        loginvar: i32,
        /// `Player.channelId`（创角时落库，登录时读回）。
        ///
        /// 不发给客户端（`LoginResp_4` 没有这个字段），只用来回填 `MemWorld`，
        /// 供 17301/17101/17081 等按渠道判定的消息取用。
        channel_id: i32,
        /// `Player.cid`。
        cid: String,
        /// `Player.money`：银两（十进制字符串）。
        money: String,
        /// `Player.totalMoney`：总财力（十进制字符串）。
        total_money: String,
    },
}

/// 纯函数：给定请求与服务端状态，算出登录结果。
///
/// 刻意**不做 IO**：不查库、不写缓存、不发包。
/// 副作用全部留给调用方，这样每条分支都能单独测。
#[must_use]
pub fn evaluate(req: &LoginReq4, st: &ServerState) -> LoginOutcome {
    // 1. 区服不符
    if let Some(sid) = req.server_id {
        if sid != st.server_id {
            return LoginOutcome::Reject(RetCode::ServerBusy);
        }
    }
    // 2. 维护中
    if st.maintaining {
        return LoginOutcome::Reject(RetCode::ServerWeihu);
    }
    // 3. 未开服
    if !st.opened {
        return LoginOutcome::Reject(RetCode::NoOpen);
    }
    // 4. 版本不对
    let client_ver = req.version.unwrap_or(0);
    if client_ver < st.required_version {
        return LoginOutcome::Reject(RetCode::VersionNo);
    }
    // 5. 人数已满
    if st.online >= st.max_online {
        return LoginOutcome::Reject(RetCode::MaxPlayerNum);
    }
    // 到这里才需要外部数据（封禁 / 角色是否存在），由调用方补齐。
    LoginOutcome::NeedCreate { names: Vec::new() }
}

/// 账号侧数据：登录判定里唯一需要外部查询的部分。
///
/// 原版这层是 `PlayerServerModule.getPlayerByOpenId(openId, serverId)`，
/// 现在由仓储实现（见 `game-core` 的 `PgAccountLookup`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    /// 已有角色 ID。
    pub player_id: PlayerId,
    /// 角色创建时间（毫秒）。
    pub create_player_time: i64,
    /// 封禁截止（`-1` 永久封禁，`0` 未封禁）。
    pub banned_time: i64,
    /// `Player.channelId`（创角时写入，登录时读回）。
    pub channel_id: i32,
    /// `Player.cid`。
    pub cid: String,
    /// `Player.money`：银两（`Player.java:146`，`BigInteger` 的十进制表示）。
    ///
    /// 原版 `Player` 整行从库里加载，所以登录时银两一定已就位；
    /// 这里跟着 `Account` 一起带回来，登录成功后写进 `PlayerBox`。
    pub money: String,
    /// `Player.totalMoney`：总财力（`Player.java:266`）。
    pub total_money: String,
}

/// 登录成功后要回填进 `MemWorld` 的角色字段。
///
/// 原版 `Player` 是整行从库里加载的，登录完就等于 `MemWorld` 里有一份完整实体；
/// 移植版把「登录期读到的列」用这个元组带出来，由 `handlers.rs` 写进 `PlayerBox`。
///
/// 字段顺序：`player_id` / `channel_id` / `cid` / `money` / `total_money`。
pub type LoginPlayer = (PlayerId, i32, String, String, String);

/// 账号查询。
#[async_trait]
pub trait AccountLookup: Send + Sync {
    /// 按账号 + 服找角色。没有则 `None`（客户端会去走创角）。
    async fn find(&self, open_id: &str, server_id: i32) -> Option<Account>;
}

/// 把 [`evaluate`] 的「待补数据」补齐成最终结果。
///
/// 顺序与原版一致：先做服务端状态判定（维护 / 开服 / 版本 / 人数），
/// 这些都过了才去查账号 —— 所以 `Reject` 分支不会有任何 IO。
pub async fn resolve(
    out: LoginOutcome,
    open_id: &str,
    server_id: i32,
    now_ms: i64,
    loginvar: i32,
    lookup: Option<&dyn AccountLookup>,
    handshake: Option<&dyn HandshakeStore>,
) -> LoginOutcome {
    let LoginOutcome::NeedCreate { names } = out else {
        return out;
    };
    let Some(l) = lookup else {
        return LoginOutcome::NeedCreate { names };
    };
    match l.find(open_id, server_id).await {
        None => LoginOutcome::NeedCreate { names },
        Some(acc) => {
            // 原版 `channelPlayer.setLoginvar(v29)`：登录成功即把挑战种子留底，
            // 等客户端通过 cmd 16/17/18 回传密码。
            if let Some(hs) = handshake {
                hs.set_challenge(acc.player_id, loginvar);
            }
            LoginOutcome::Accepted {
                player_id: acc.player_id,
                server_time: now_ms,
                // 开服时间原版取自 `BaseServer.getOpenServerTime()`，
                // 骨架阶段还没有这层，先回 0（客户端只用来显示）。
                open_server_time: 0,
                create_player_time: acc.create_player_time,
                banned_time: acc.banned_time,
                loginvar,
                channel_id: acc.channel_id,
                cid: acc.cid,
                money: acc.money,
                total_money: acc.total_money,
            }
        }
    }
}

/// 带账号查询的登录入口（服务端实际走这条）。
///
/// # Errors
/// 解密失败或 protobuf 损坏。
pub async fn handle_with(
    ctx: &HandlerCtx,
    body: &[u8],
    st: &ServerState,
    lookup: Option<&dyn AccountLookup>,
    now_ms: i64,
    loginvar: i32,
    handshake: Option<&dyn HandshakeStore>,
) -> Result<(LoginResp4, Option<LoginPlayer>)> {
    let LoginRequest { req } = LoginRequest::decrypt(body)?;
    let open_id = req.open_id.clone().unwrap_or_default();
    let server_id = req.server_id.unwrap_or(st.server_id);
    let base = evaluate(&req, st);
    let outcome = resolve(
        base, &open_id, server_id, now_ms, loginvar, lookup, handshake,
    )
    .await;
    tracing::info!(
        peer = %ctx.peer_ip,
        open_id = %open_id,
        ok = matches!(outcome, LoginOutcome::Accepted { .. }),
        "登录判定完成"
    );
    let player = match &outcome {
        LoginOutcome::Accepted {
            player_id,
            channel_id,
            cid,
            money,
            total_money,
            ..
        } => Some((
            *player_id,
            *channel_id,
            cid.clone(),
            money.clone(),
            total_money.clone(),
        )),
        _ => None,
    };
    Ok((build_resp(outcome), player))
}

/// 把结果填进回包。
#[must_use]
pub fn build_resp(o: LoginOutcome) -> LoginResp4 {
    match o {
        LoginOutcome::Reject(code) => LoginResp4 {
            ret: code.as_i32(),
            ok: None,
            names: vec![],
            server_time: None,
            open_server_time: None,
            create_player_time: None,
            loginvar: None,
            banned_time: None,
            lan_config: vec![],
        },
        LoginOutcome::NeedCreate { names } => LoginResp4 {
            ret: RetCode::Ok.as_i32(),
            ok: Some(false),
            names,
            server_time: None,
            open_server_time: None,
            create_player_time: None,
            loginvar: None,
            banned_time: None,
            lan_config: vec![],
        },
        // 原版 `LoginResp_4` 里没有 playerId 字段（服务端靠 openId 反查），
        // 所以这里不需要绑定它；`Accepted.player_id` 留给调用方建会话用。
        LoginOutcome::Accepted {
            server_time,
            open_server_time,
            create_player_time,
            banned_time,
            loginvar,
            ..
        } => LoginResp4 {
            ret: RetCode::Ok.as_i32(),
            ok: Some(true),
            names: vec![],
            server_time: Some(server_time),
            open_server_time: Some(open_server_time),
            create_player_time: Some(create_player_time),
            // 原服这里回的是 playerId 的低 32 位，客户端拿它做校验
            loginvar: Some(loginvar),
            banned_time: Some(banned_time),
            lan_config: vec![],
        },
    }
}

/// 登录入口。
///
/// 只负责：解密 → 判定 → 组包。账号与角色查询由注入的 repository 完成，
/// 这里保持无 IO 以便单测覆盖全部分支。
///
/// # Errors
/// 解密失败或 protobuf 损坏。
pub fn handle(ctx: &HandlerCtx, body: &[u8], st: &ServerState) -> Result<LoginResp4> {
    let LoginRequest { req } = LoginRequest::decrypt(body)?;
    let open_id = req.open_id.clone().unwrap_or_default();
    tracing::info!(
        peer = %ctx.peer_ip,
        open_id = %open_id,
        server_id = ?req.server_id,
        version = ?req.version,
        "登录请求（密文已解）"
    );
    Ok(build_resp(evaluate(&req, st)))
}

/// 把解密错误转成统一的 `AppError`。
///
/// 不能写 `impl From<LoginBodyError> for AppError`：两个类型都不在本 crate，
/// 违反孤儿规则（E0117）。这里是本项目唯一的转换点，写成函数就够。
fn to_app_error(e: LoginBodyError) -> AppError {
    // 解密失败不外泄细节，只回「参数错误」
    AppError::BadRequest(format!("登录包解密失败: {e}"))
}

/// 心跳（`msgId = 5`）。
///
/// 原服务端只回一个时间戳，不校验频率 —— 这是断线检测的唯一依据。
#[must_use]
pub fn heartbeat(now_ms: i64) -> HeartbeatResp {
    HeartbeatResp { server_ms: now_ms }
}

/// 心跳响应。
#[derive(Debug, Clone, Copy)]
pub struct HeartbeatResp {
    /// 服务器毫秒时间戳。
    pub server_ms: i64,
}

/// 区服条目。
#[derive(Debug, Clone)]
pub struct ServerEntry {
    /// 区服 ID。
    pub id: i32,
    /// 名称。
    pub name: String,
    /// 状态（0 维护 / 1 流畅 / 2 繁忙 / 3 爆满）。
    pub state: i32,
}

/// 区服列表（`msgId = 3`）。
#[must_use]
pub fn server_list() -> ServerListResp {
    ServerListResp {
        servers: Vec::new(),
    }
}

/// 区服列表响应。
#[derive(Debug, Clone, Default)]
pub struct ServerListResp {
    /// 区服。
    pub servers: Vec<ServerEntry>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use prost::Message;

    fn req(server_id: i32, version: i32) -> LoginReq4 {
        LoginReq4 {
            open_id: Some("111111".into()),
            server_id: Some(server_id),
            version: Some(version),
            ..Default::default()
        }
    }

    fn base64_encode(b: &[u8]) -> String {
        use base64::Engine as _;
        base64::engine::general_purpose::STANDARD.encode(b)
    }

    #[test]
    fn wrong_server_rejects_busy() {
        let st = ServerState::healthy(1);
        assert_eq!(
            evaluate(&req(2, 0), &st),
            LoginOutcome::Reject(RetCode::ServerBusy)
        );
    }

    #[test]
    fn maintaining_rejects_weihu() {
        let mut st = ServerState::healthy(1);
        st.maintaining = true;
        assert_eq!(
            evaluate(&req(1, 0), &st),
            LoginOutcome::Reject(RetCode::ServerWeihu)
        );
    }

    #[test]
    fn not_opened_rejects_no_open() {
        let mut st = ServerState::healthy(1);
        st.opened = false;
        assert_eq!(
            evaluate(&req(1, 0), &st),
            LoginOutcome::Reject(RetCode::NoOpen)
        );
    }

    #[test]
    fn old_client_rejects_version_no() {
        let mut st = ServerState::healthy(1);
        st.required_version = 100;
        // proto 注释：99 为客户端版本不对
        assert_eq!(
            evaluate(&req(1, 99), &st),
            LoginOutcome::Reject(RetCode::VersionNo)
        );
        // 刚好等于要求值则放行
        assert_eq!(
            evaluate(&req(1, 100), &st),
            LoginOutcome::NeedCreate { names: vec![] }
        );
    }

    #[test]
    fn full_server_rejects_max_player() {
        let mut st = ServerState::healthy(1);
        st.online = 10_000;
        assert_eq!(
            evaluate(&req(1, 0), &st),
            LoginOutcome::Reject(RetCode::MaxPlayerNum)
        );
    }

    #[test]
    fn reject_resp_carries_ret_code() {
        let r = build_resp(LoginOutcome::Reject(RetCode::VersionNo));
        assert_eq!(r.ret, 99);
        assert_eq!(r.ok, None);
    }

    #[test]
    fn need_create_sets_ok_false() {
        let r = build_resp(LoginOutcome::NeedCreate {
            names: vec!["张三".into(), "李四".into()],
        });
        assert_eq!(r.ret, 0);
        assert_eq!(r.ok, Some(false));
        assert_eq!(r.names.len(), 2);
    }

    #[test]
    fn accepted_carries_times_and_loginvar() {
        let r = build_resp(LoginOutcome::Accepted {
            player_id: PlayerId::new(1129145656),
            server_time: 1_700_000_000_000,
            open_server_time: 1_600_000_000_000,
            create_player_time: 1_650_000_000_000,
            banned_time: 0,
            loginvar: 608,
            channel_id: 0,
            cid: String::new(),
            money: "20000".to_owned(),
            total_money: "0".to_owned(),
        });
        assert_eq!(r.ret, 0);
        assert_eq!(r.ok, Some(true));
        assert_eq!(r.server_time, Some(1_700_000_000_000));
        // 信里回的是**挑战值**，不是 playerId —— 原版 `builder.setLoginvar(v27)`
        assert_eq!(r.loginvar, Some(608));
    }

    /// 端到端：客户端 `xxteaEncryptToBase64` 出来的包，服务端必须能解开。
    #[test]
    fn decrypts_client_style_packet() {
        let inner = base64_encode(b"openId=111111&serverId=1");
        let cipher = ddj_codec::xxtea::encrypt(inner.as_bytes(), &SIGN_KEY);
        let plain = unpack_login_body(&cipher, &SIGN_KEY).expect("解不开");
        assert_eq!(plain, b"openId=111111&serverId=1");
    }

    /// 端到端：base64 里放真实 protobuf，能还原成 `LoginReq4`。
    #[test]
    fn decrypts_and_decodes_protobuf() {
        let r = req(1, 7);
        let bytes = r.encode_to_vec();
        let b64 = base64_encode(&bytes);
        let cipher = ddj_codec::xxtea::encrypt(b64.as_bytes(), &SIGN_KEY);
        let got = LoginRequest::decrypt(&cipher).expect("解密+解码");
        assert_eq!(got.req.open_id.as_deref(), Some("111111"));
        assert_eq!(got.req.version, Some(7));
    }

    /// 拿错密钥必须失败，而不是解出乱码。
    #[test]
    fn wrong_key_fails_loudly() {
        let inner = base64_encode(b"x");
        let cipher = ddj_codec::xxtea::encrypt(inner.as_bytes(), b"WRONGKEYWRONGKEY");
        assert!(unpack_login_body(&cipher, &SIGN_KEY).is_err());
    }
}

#[cfg(test)]
mod account_tests {
    use super::*;

    /// 账号查询的假实现。
    struct FakeLookup {
        hit: Option<Account>,
        /// 记录被查询的 (open_id, server_id)。
        seen: std::sync::Mutex<Vec<(String, i32)>>,
    }

    #[async_trait]
    impl AccountLookup for FakeLookup {
        async fn find(&self, open_id: &str, server_id: i32) -> Option<Account> {
            self.seen
                .lock()
                .expect("锁")
                .push((open_id.to_owned(), server_id));
            self.hit.clone()
        }
    }

    fn acc(pid: i64) -> Account {
        Account {
            player_id: PlayerId::new(pid),
            create_player_time: 1_700_000_000_000,
            banned_time: 0,
            channel_id: 0,
            cid: String::new(),
            money: "20000".to_owned(),
            total_money: "0".to_owned(),
        }
    }

    #[tokio::test]
    async fn resolve_without_lookup_keeps_need_create() {
        let out = resolve(
            LoginOutcome::NeedCreate { names: vec![] },
            "open-1",
            7,
            0,
            608,
            None,
            None,
        )
        .await;
        assert!(matches!(out, LoginOutcome::NeedCreate { .. }));
    }

    #[tokio::test]
    async fn resolve_finds_existing_player() {
        let l = FakeLookup {
            hit: Some(acc(9001)),
            seen: std::sync::Mutex::new(vec![]),
        };
        let out = resolve(
            LoginOutcome::NeedCreate { names: vec![] },
            "open-1",
            7,
            12345,
            608,
            Some(&l),
            None,
        )
        .await;
        match out {
            LoginOutcome::Accepted {
                player_id,
                server_time,
                create_player_time,
                banned_time,
                ..
            } => {
                assert_eq!(player_id.get(), 9001);
                assert_eq!(server_time, 12345);
                assert_eq!(create_player_time, 1_700_000_000_000);
                assert_eq!(banned_time, 0);
            }
            other => panic!("应为 Accepted，实际 {other:?}"),
        }
        assert_eq!(
            l.seen.lock().expect("锁").as_slice(),
            [("open-1".to_owned(), 7)]
        );
    }

    #[tokio::test]
    async fn resolve_missing_account_keeps_need_create() {
        let l = FakeLookup {
            hit: None,
            seen: std::sync::Mutex::new(vec![]),
        };
        let out = resolve(
            LoginOutcome::NeedCreate { names: vec![] },
            "open-1",
            7,
            0,
            608,
            Some(&l),
            None,
        )
        .await;
        assert!(matches!(out, LoginOutcome::NeedCreate { .. }));
        assert_eq!(l.seen.lock().expect("锁").len(), 1);
    }

    #[tokio::test]
    async fn reject_does_not_touch_lookup() {
        let l = FakeLookup {
            hit: Some(acc(9001)),
            seen: std::sync::Mutex::new(vec![]),
        };
        let out = resolve(
            LoginOutcome::Reject(RetCode::ServerWeihu),
            "open-1",
            7,
            0,
            608,
            Some(&l),
            None,
        )
        .await;
        assert_eq!(out, LoginOutcome::Reject(RetCode::ServerWeihu));
        assert!(
            l.seen.lock().expect("锁").is_empty(),
            "Reject 分支不得产生任何 IO"
        );
    }
}
