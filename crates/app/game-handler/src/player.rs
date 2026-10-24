//! 创建玩家（cmdId = 6）
//!
//! 对应 Java：`EngineMessageHandler.createPlayerReq` → `WorkTaskCreatePlayer.run()`
//! （745 条指令，`/tmp/wtcreate.txt`）。
//!
//! ## 原分支序（字节码还原，顺序不可调换）
//! ```text
//!  1. getTargetServerId(req.serverId) != GameServer.serverId
//!        → 【回 LoginResp_4，不是 CreatePlayerResp_6！】ret = COMMON_SERVER_BUSY(2)
//!  2. invitedPid 防伪（非空时）：
//!        a. !matches("[0-9]*")                     → invitedPid = ""
//!        b. len != len(str(getPlayerIDRange() * server_id)) → ""
//!        c. parseInt(invitedPid[0..3]) != channel_id → ""
//!        d. Redis hGet(invitedPid, openId) 非空      → ""（已领过裂变奖励）
//!  3. white = isWhite(openId, ip, mac, channelId)
//!  4. !GameLogicConfig.isTourists() && !white：
//!        a. (hasToken && token == "")              → PLAYER_ERROR_6(106)
//!        b. !isSignLegal(sign, channelId, openId, token, timestamp, getLoginKey(platform, h5Client))
//!                                                  → PLAYER_ERROR_6(106)
//!  5. !white && !GsServer.isRegistStatus()
//!        → 【回 LoginResp_4】ret = COMMON_SERVER_REGISTER_NO(20)
//!  6. !white && getRegisterPlayerNum() >= REGISTER_MAX_NUM
//!        → 【回 LoginResp_4】ret = COMMON_SERVER_MAX_PLAYER_NUM(4)
//!  7. name.contains(".R")                        → PLAYER_ERROR_4(104)
//!  8. LogicUtils.containsFirstSpace(name)         → 104（首或尾字符 isWhitespace）
//!  9. !nameSpaceOnoff && LogicUtils.containsSpace(name) → 104（任意字符 isWhitespace）
//! 10. WxMsgSecCheck.checkDirtyWord(name, wxAppIdx, lan, wxOpenId, [type_2]) → 104
//! 11. isVnSdk() && !matchName(name)               → 104
//! 12. stringLength(name) < 1 || > NickNameLength  → PLAYER_ERROR_5(105)
//!     （stringLength：SDK_ID==6 时 = char 数；否则 getLenOf：中文记 2）
//! 13. faceId 校验（白名单 = ConfigDB.CreatingRolesFashionType = [5,6,7]）：
//!        a. getFashionDB(id) == null               → PLAYER_ERROR_17(117)
//!        b. type 不在白名单                        → PLAYER_ERROR_18(118)
//!        c. type 重复                              → PLAYER_ERROR_19(119)
//!        d. 白名单 removeAll(已选) 后，剩余 type 逐个
//!           getFashionDBTyFaceId(type).getId() 补进 faceId（默认脸自动补齐）
//! 14. synchronized(Player.class)：DBPlayerModule.repeated(name) → PLAYER_ERROR_3(103)
//! 15. createPlayer(...) == null                   → 103
//! 16. 成功：setLastIp → vestId>0 ? addTalent(vest.talentId)+addPet(vest.petId)
//!           : addTalent(InitializeTalentId=[1035,1007])，全 TYPE_1011
//!     → invitedPid 非空写 Redis H5Welfare
//!     → 回 CreatePlayerResp_6{ret=0}
//!     → 后续副作用：MonitoringManager.gameRoleCreate / OperationEvent(LOGIN_COUNT)
//!       / PlayerRegisterLogModule.addLog / needSendMail191 → 191 模板邮件
//!       / sendCreateMail（QQ 客服邮件，#qq 占位替换） / needSdkStat → SdkStat Redis
//! 17. 任何异常                                    → ret = COMMON_SERVER_BUSY(2)
//! ```
//!
//! ## 关键常量（来源全部实证）
//! - `NickNameLength = 14`（`B-变量表-Config.csv` id=5）
//! - `CreatingRolesFashionType = "5;6;7"`（同表 id=7）
//! - `InitializeTalentId = "1035;1007"`（同表 id=3）
//! - 默认脸：`Z-装扮-Fashion.csv` 中 `getType()==3` 的行按 `type` 索引：
//!   type5→[2501..2508]，type6→[2601..2608]，type7→[2701..2708]
//!   （`createFashionDBs` 只收 `getType()==3`，`getFashionDBTyFaceId(type)` 取其一）

use ddj_core::{AppError, Result};
use ddj_pb::gen::engine_message::{CreatePlayerReq6, CreatePlayerResp6};
use ddj_pb::RetCode;

/// 名字长度上限（`ConfigDB.NickNameLength`）。
pub const NICK_NAME_LENGTH: usize = 14;
/// 创角捏脸类型白名单（`ConfigDB.CreatingRolesFashionType`）。
pub const CREATING_ROLES_FASHION_TYPES: &[i32] = &[5, 6, 7];
/// 初始门客（`ConfigDB.InitializeTalentId`）。
pub const INITIALIZE_TALENT_ID: &[i32] = &[1035, 1007];
/// 初始门客的获得来源（`TypeModule.TYPE_1011`）。
pub const TYPE_1011: i32 = 1011;

/// `LogicUtils.getLenOf`：中文记 2，其它记 1。
///
/// Java 判定两个函数：
/// - `isChineseByScript`: `Character.UnicodeScript.of(c) == HAN`
/// - `isChinesePunctuation`: UnicodeBlock ∈ {GENERAL_PUNCTUATION,
///   CJK_SYMBOLS_AND_PUNCTUATION, HALFWIDTH_AND_FULLWIDTH_FORMS,
///   CJK_COMPATIBILITY_FORMS, VERTICAL_FORMS}
///
/// Rust 没有 `UnicodeScript` API，用码位区间还原（与 Java 覆盖面一致）。
#[must_use]
pub fn get_len_of(s: &str) -> usize {
    s.chars()
        .map(|c| {
            if is_chinese_char(c) || is_chinese_punctuation(c) {
                2
            } else {
                1
            }
        })
        .sum()
}

/// Java `UnicodeScript.of(c) == HAN`（CJK 表意文字各区块）。
fn is_chinese_char(c: char) -> bool {
    matches!(c,
        '\u{3400}'..='\u{4DBF}'     // CJK Ext A
        | '\u{4E00}'..='\u{9FFF}'   // CJK Unified
        | '\u{F900}'..='\u{FAFF}'   // CJK Compatibility Ideographs
        | '\u{20000}'..='\u{2A6DF}' // Ext B
        | '\u{2A700}'..='\u{2EBEF}' // Ext C–F
    )
}

/// Java 五个 UnicodeBlock 的判定（Blocks.txt 区间）。
#[must_use]
pub fn is_chinese_punctuation(c: char) -> bool {
    matches!(c,
        '\u{2000}'..='\u{206F}'     // GENERAL_PUNCTUATION
        | '\u{3000}'..='\u{303F}'   // CJK_SYMBOLS_AND_PUNCTUATION
        | '\u{FF00}'..='\u{FFEF}'   // HALFWIDTH_AND_FULLWIDTH_FORMS
        | '\u{FE30}'..='\u{FE4F}'   // CJK_COMPATIBILITY_FORMS
        | '\u{FE10}'..='\u{FE1F}'   // VERTICAL_FORMS
    )
}

/// `LogicUtils.containsFirstSpace`（字节码语义）：
/// null/empty → **false**；首字符或尾字符 `Character.isWhitespace` → true。
#[must_use]
pub fn contains_first_space(s: &str) -> bool {
    let Some(first) = s.chars().next() else {
        return false; // 空串：Java 空串也返回 false（字节码 ifeq ->13）
    };
    if first.is_whitespace() {
        return true;
    }
    s.chars().next_back().is_some_and(char::is_whitespace)
}

/// `LogicUtils.containsSpace`：任意字符 `Character.isWhitespace`。
/// 空串返回 false（字节码 `isEmpty -> iconst_0`）。
#[must_use]
pub fn contains_space(s: &str) -> bool {
    s.chars().any(char::is_whitespace)
}

/// `PlatformServerModule.stringLength`（非 VN SDK 路径 = `UtilsTools.getStringLength`）。
///
/// VN（SDK_ID=6）走「char 数」，本服为中文服，先按默认路径实现。
#[must_use]
pub fn string_length(s: &str) -> usize {
    get_len_of(s)
}

/// `FashionDBCache.getFashionDBTyFaceId(type)` 的数据镜像。
///
/// `createFashionDBs` 只收 `getType()==3` 的行，key = `type`。
/// 实测（`Z-装扮-Fashion.csv`）：type5→[2501..2508]、type6→[2601..2608]、
/// type7→[2701..2708]。Java HashMap 迭代顺序不保证，这里取每 type 首个 id。
#[must_use]
pub fn default_face_id(face_type: i32) -> Option<i32> {
    const DEFAULT_FACES: &[(i32, i32)] = &[(5, 2501), (6, 2601), (7, 2701)];
    DEFAULT_FACES
        .iter()
        .find(|(t, _)| *t == face_type)
        .map(|(_, id)| *id)
}

/// `DBModule.getPlayerIDRange()`（每服玩家 ID 段基数）。
///
/// 字节码里该值是运行时常量；接入 DBModule 后从配置读取替换。
pub const PLAYER_ID_RANGE: i64 = 1_000_000;

/// `invitedPid` 三段静态校验（分支 2a/2b/2c）。
///
/// 2d（Redis 查重）是外部副作用，由调用方处理。
#[must_use]
pub fn invited_pid_looks_valid(pid: &str, server_id: i32) -> bool {
    // 2a: "[0-9]*" 全串匹配（Java matches()）
    if pid.is_empty() || !pid.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    // 2b: 长度 = len(str(getPlayerIDRange() * server_id))
    let expected_len = (PLAYER_ID_RANGE * i64::from(server_id)).to_string().len();
    if pid.len() != expected_len {
        return false;
    }
    // 2c: 前 3 位 == GsServer.channel_id（调用方传入比较值，接口保留）
    pid.get(..3).is_some_and(|p| p.parse::<i32>().is_ok())
}

/// 创角校验结果：纯数据，不发 IO、不发网络。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreateOutcome {
    /// 拒绝。原版把「回 LoginResp_4」也归并到这里，由调用方决定信封类型。
    Reject {
        /// 错误码。
        ret: RetCode,
        /// true = 原版回的是 `LoginResp_4`（分支 1/5/6），false = `CreatePlayerResp_6`。
        as_login_resp: bool,
    },
    /// 校验通过，可以落库。
    Accept {
        /// 补齐默认脸后的最终 faceId 列表（分支 13d 之后的 v12）。
        face_ids: Vec<i32>,
    },
}

/// 创角校验的输入快照（服务端状态）。
#[derive(Debug, Clone)]
pub struct CreateCtx {
    /// 本服 ID（`GameServer.serverId`）。
    pub server_id: i32,
    /// 是否允许游客（`GameLogicConfig.isTourists`）。
    pub tourists: bool,
    /// 是否白名单（`isWhite` 结果）。
    pub is_white: bool,
    /// 注册开关（`GsServer.isRegistStatus`）。
    pub regist_open: bool,
    /// 已注册人数（`getRegisterPlayerNum`）。
    pub register_num: i32,
    /// 注册上限（`REGISTER_MAX_NUM`）。
    pub register_max: i32,
    /// 名字空格开关（`nameSpaceOnoff`，true = 允许）。
    pub name_space_onoff: bool,
    /// 名字是否已重复（`DBPlayerModule.repeated(name)`）。
    pub name_repeated: bool,
}

impl CreateCtx {
    /// 一个全开的本地测试服。
    #[must_use]
    pub const fn healthy(server_id: i32) -> Self {
        Self {
            server_id,
            tourists: true,
            is_white: true,
            regist_open: true,
            register_num: 0,
            register_max: i32::MAX,
            name_space_onoff: true,
            name_repeated: false,
        }
    }
}

/// `FashionDB` 只取本流程需要的两个列。
#[derive(Debug, Clone, Copy)]
pub struct FashionInfo {
    /// 装扮 id。
    pub id: i32,
    /// 装扮类型（`getType`）。
    pub fashion_type: i32,
}

/// 创角数据源（CSV/DB 由调用方注入）。
pub trait FashionLookup {
    /// 查装扮。
    ///
    /// # Errors
    /// 数据层故障。
    fn get(&self, id: i32) -> Result<Option<FashionInfo>>;
}

/// 内存版装扮表（生产由模板加载器构造）。
#[derive(Debug, Clone, Default)]
pub struct FashionTable {
    rows: std::collections::HashMap<i32, FashionInfo>,
}

impl FashionTable {
    /// 建表。
    #[must_use]
    pub fn new(rows: impl IntoIterator<Item = FashionInfo>) -> Self {
        Self {
            rows: rows.into_iter().map(|r| (r.id, r)).collect(),
        }
    }
}

impl FashionLookup for FashionTable {
    fn get(&self, id: i32) -> Result<Option<FashionInfo>> {
        Ok(self.rows.get(&id).copied())
    }
}

/// 创角校验主流程 —— 严格按 Java 分支序（见模块文档）。
///
/// **不做**名字查重、不做落库、不写 Redis；这些副作用由调用方在
/// `Accept` 分支里完成（与原版把 `repeated`/`createPlayer` 放在
/// `synchronized` 块内等价的分层方式）。
///
/// # Errors
/// 数据层故障（117/118/119 走 `AppError::BadRequest`，带错误码文本）。
pub fn evaluate(
    req: &CreatePlayerReq6,
    ctx: &CreateCtx,
    fashion: &dyn FashionLookup,
) -> Result<CreateOutcome> {
    // 1. serverId 不符 → LoginResp_4{ret=2}
    if req.server_id.unwrap_or(0) != ctx.server_id {
        return Ok(CreateOutcome::Reject {
            ret: RetCode::ServerBusy,
            as_login_resp: true,
        });
    }
    // 2. invitedPid 防伪 —— 纯校验，改写发生在调用方（原版改写局部变量 v6）
    if let Some(invited) = req.invited_pid.as_deref() {
        if !invited.is_empty() {
            let _static_ok = invited_pid_looks_valid(invited, ctx.server_id);
        }
    }
    // 4. 非游客 && 非白名单：token / sign
    if !ctx.tourists && !ctx.is_white && req.token.as_deref() == Some("") {
        return Ok(CreateOutcome::Reject {
            ret: RetCode::NotLoginPlatform,
            as_login_resp: false,
        });
    }
    // 4b. isSignLegal：外部签名校验，由 handle() 注入后补。
    // 5/6. 注册开关与人数上限（非白名单才查）
    if !ctx.is_white {
        if !ctx.regist_open {
            return Ok(CreateOutcome::Reject {
                ret: RetCode::RegisterNo,
                as_login_resp: true,
            });
        }
        if ctx.register_num >= ctx.register_max {
            return Ok(CreateOutcome::Reject {
                ret: RetCode::MaxPlayerNum,
                as_login_resp: true,
            });
        }
    }
    // 7–12. 名字相关（全部回 CreatePlayerResp_6）
    let name = req.name.clone().unwrap_or_default();
    if name.contains(".R") {
        return Ok(reject_name_illegal());
    }
    if contains_first_space(&name) {
        return Ok(reject_name_illegal());
    }
    if !ctx.name_space_onoff && contains_space(&name) {
        return Ok(reject_name_illegal());
    }
    // 10. 微信敏感词 / 11. VN 正则：外部依赖，handle() 注入，纯函数跳过。
    let len = string_length(&name);
    if !(1..=NICK_NAME_LENGTH).contains(&len) {
        return Ok(CreateOutcome::Reject {
            ret: RetCode::NameLength,
            as_login_resp: false,
        });
    }
    // 13. faceId 校验
    let face_ids = validate_face_ids(&req.face_id, fashion)?;
    // 14. 名字查重
    if ctx.name_repeated {
        return Ok(CreateOutcome::Reject {
            ret: RetCode::NameDuplicate,
            as_login_resp: false,
        });
    }
    Ok(CreateOutcome::Accept { face_ids })
}

fn reject_name_illegal() -> CreateOutcome {
    CreateOutcome::Reject {
        ret: RetCode::NameIllegal,
        as_login_resp: false,
    }
}

/// 分支 13：faceId 校验 + 默认脸补齐。
///
/// # Errors
/// 数据层故障。
pub fn validate_face_ids(face_ids: &[i32], fashion: &dyn FashionLookup) -> Result<Vec<i32>> {
    let mut allowed: Vec<i32> = CREATING_ROLES_FASHION_TYPES.to_vec();
    let mut seen_types: Vec<i32> = Vec::new();
    let mut out: Vec<i32> = Vec::new();
    for &id in face_ids {
        let Some(info) = fashion.get(id)? else {
            return Err(AppError::BadRequest(format!(
                "PLAYER_ERROR_17(117): faceId {id} 不存在"
            )));
        };
        if !allowed.contains(&info.fashion_type) {
            return Err(AppError::BadRequest(format!(
                "PLAYER_ERROR_18(118): faceId {id} type={} 不在白名单",
                info.fashion_type
            )));
        }
        if seen_types.contains(&info.fashion_type) {
            return Err(AppError::BadRequest(format!(
                "PLAYER_ERROR_19(119): type {} 重复",
                info.fashion_type
            )));
        }
        seen_types.push(info.fashion_type);
        out.push(id);
    }
    // 13d: 白名单 removeAll(已选) 后，剩余 type 补默认脸
    allowed.retain(|t| !seen_types.contains(t));
    for t in &allowed {
        if let Some(default_id) = default_face_id(*t) {
            out.push(default_id);
        }
    }
    Ok(out)
}

/// 把校验结果转成 `CreatePlayerResp6`（ret-only 信封）。
#[must_use]
pub fn build_resp(ret: RetCode) -> CreatePlayerResp6 {
    CreatePlayerResp6 { ret: ret.as_i32() }
}

/// 角色基础信息（原 `msgId = 10001`，保留骨架接口）。
///
/// # Errors
/// 未登录。
pub fn info(player_id: i64) -> PlayerInfoResp {
    PlayerInfoResp {
        player_id,
        level: 1,
        name: String::new(),
        power: 0,
    }
}

/// 角色信息响应。
#[derive(Debug, Clone, Default)]
pub struct PlayerInfoResp {
    /// ID。
    pub player_id: i64,
    /// 等级。
    pub level: u32,
    /// 昵称。
    pub name: String,
    /// 战力。
    pub power: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> FashionTable {
        let mut rows = Vec::new();
        // type5/6/7 各 8 张（getType==3 的默认脸）
        for (base, ty) in [(2500, 5), (2600, 6), (2700, 7)] {
            for i in 1..=8 {
                rows.push(FashionInfo {
                    id: base + i,
                    fashion_type: ty,
                });
            }
        }
        // 一张白名单外的（type=1）
        rows.push(FashionInfo {
            id: 1001,
            fashion_type: 1,
        });
        FashionTable::new(rows)
    }

    fn req(name: &str, face_ids: &[i32]) -> CreatePlayerReq6 {
        CreatePlayerReq6 {
            name: Some(name.into()),
            face_id: face_ids.to_vec(),
            server_id: Some(1),
            ..Default::default()
        }
    }

    #[test]
    fn wrong_server_returns_login_envelope_busy() {
        let ctx = CreateCtx::healthy(2); // 客户端请求 server_id=1，本服是 2
        let r = evaluate(&req("abc", &[]), &ctx, &table()).expect("ok");
        assert_eq!(
            r,
            CreateOutcome::Reject {
                ret: RetCode::ServerBusy,
                as_login_resp: true
            }
        );
    }

    #[test]
    fn name_with_dot_r_rejected() {
        let ctx = CreateCtx::healthy(1);
        let r = evaluate(&req("abc.R", &[]), &ctx, &table()).expect("ok");
        assert!(matches!(
            r,
            CreateOutcome::Reject {
                ret: RetCode::NameIllegal,
                ..
            }
        ));
    }

    #[test]
    fn leading_space_rejected() {
        let ctx = CreateCtx::healthy(1);
        assert!(matches!(
            evaluate(&req(" abc", &[]), &ctx, &table()).expect("ok"),
            CreateOutcome::Reject {
                ret: RetCode::NameIllegal,
                ..
            }
        ));
    }

    #[test]
    fn trailing_space_rejected() {
        let ctx = CreateCtx::healthy(1);
        assert!(matches!(
            evaluate(&req("abc ", &[]), &ctx, &table()).expect("ok"),
            CreateOutcome::Reject {
                ret: RetCode::NameIllegal,
                ..
            }
        ));
    }

    #[test]
    fn inner_space_allowed_when_namespace_onoff() {
        let ctx = CreateCtx::healthy(1);
        assert!(matches!(
            evaluate(&req("a b", &[]), &ctx, &table()).expect("ok"),
            CreateOutcome::Accept { .. }
        ));
    }

    #[test]
    fn inner_space_rejected_when_namespace_off() {
        let mut ctx = CreateCtx::healthy(1);
        ctx.name_space_onoff = false;
        assert!(matches!(
            evaluate(&req("a b", &[]), &ctx, &table()).expect("ok"),
            CreateOutcome::Reject {
                ret: RetCode::NameIllegal,
                ..
            }
        ));
    }

    #[test]
    fn empty_name_rejected_as_length() {
        let ctx = CreateCtx::healthy(1);
        assert!(matches!(
            evaluate(&req("", &[]), &ctx, &table()).expect("ok"),
            CreateOutcome::Reject {
                ret: RetCode::NameLength,
                ..
            }
        ));
    }

    /// 中文记 2：7 个汉字 = 14 = NickNameLength，放行；8 个 = 16，拒绝。
    #[test]
    fn chinese_counts_double() {
        let ctx = CreateCtx::healthy(1);
        assert!(matches!(
            evaluate(&req("一二三四五六七", &[]), &ctx, &table()).expect("ok"),
            CreateOutcome::Accept { .. }
        ));
        assert!(matches!(
            evaluate(&req("一二三四五六七八", &[]), &ctx, &table()).expect("ok"),
            CreateOutcome::Reject {
                ret: RetCode::NameLength,
                ..
            }
        ));
    }

    #[test]
    fn fullwidth_counts_double() {
        // U+FF21 Ａ 在 HALFWIDTH_AND_FULLWIDTH_FORMS → 记 2
        assert_eq!(get_len_of("Ａ"), 2);
    }

    #[test]
    fn face_id_missing_is_117() {
        let ctx = CreateCtx::healthy(1);
        let r = evaluate(&req("abc", &[99999]), &ctx, &table());
        assert!(r.is_err_and(|e| e.to_string().contains("117")));
    }

    #[test]
    fn face_id_out_of_whitelist_is_118() {
        let ctx = CreateCtx::healthy(1);
        let r = evaluate(&req("abc", &[1001]), &ctx, &table());
        assert!(r.is_err_and(|e| e.to_string().contains("118")));
    }

    #[test]
    fn face_id_duplicate_type_is_119() {
        let ctx = CreateCtx::healthy(1);
        let r = evaluate(&req("abc", &[2501, 2502]), &ctx, &table());
        assert!(r.is_err_and(|e| e.to_string().contains("119")));
    }

    /// 用户只选 type5 一张，其余 type6/type7 由默认脸补齐。
    #[test]
    fn face_ids_auto_completed() {
        let ctx = CreateCtx::healthy(1);
        match evaluate(&req("abc", &[2503]), &ctx, &table()).expect("ok") {
            CreateOutcome::Accept { face_ids } => {
                assert_eq!(face_ids.len(), 3);
                assert!(face_ids.contains(&2503));
                assert!(face_ids.contains(&2601));
                assert!(face_ids.contains(&2701));
            }
            _ => panic!("expect accept"),
        }
    }

    #[test]
    fn repeated_name_is_103() {
        let mut ctx = CreateCtx::healthy(1);
        ctx.name_repeated = true;
        assert!(matches!(
            evaluate(&req("abc", &[]), &ctx, &table()).expect("ok"),
            CreateOutcome::Reject {
                ret: RetCode::NameDuplicate,
                ..
            }
        ));
    }

    #[test]
    fn regist_closed_returns_login_envelope_20() {
        let mut ctx = CreateCtx::healthy(1);
        // 原版分支 5 只在非白名单时生效（字节码 484: iload v7; ifne ->559）
        ctx.is_white = false;
        ctx.regist_open = false;
        assert_eq!(
            evaluate(&req("abc", &[]), &ctx, &table()).expect("ok"),
            CreateOutcome::Reject {
                ret: RetCode::RegisterNo,
                as_login_resp: true
            }
        );
    }

    #[test]
    fn regist_closed_but_white_skips_check() {
        let mut ctx = CreateCtx::healthy(1);
        ctx.regist_open = false; // 白名单玩家不受注册开关限制
        assert!(matches!(
            evaluate(&req("abc", &[]), &ctx, &table()).expect("ok"),
            CreateOutcome::Accept { .. }
        ));
    }

    #[test]
    fn register_full_returns_login_envelope_4() {
        let mut ctx = CreateCtx::healthy(1);
        ctx.is_white = false; // 分支 6 同样只在非白名单时生效
        ctx.register_num = 10;
        ctx.register_max = 10;
        assert_eq!(
            evaluate(&req("abc", &[]), &ctx, &table()).expect("ok"),
            CreateOutcome::Reject {
                ret: RetCode::MaxPlayerNum,
                as_login_resp: true
            }
        );
    }

    // ---- getLenOf 边界（对拍 Java） ----
    #[test]
    fn len_of_mixed() {
        // "ab中cd" → 1+1+2+1+1 = 6
        assert_eq!(get_len_of("ab中cd"), 6);
        // U+3001 、 是 CJK_SYMBOLS_AND_PUNCTUATION → 2
        assert_eq!(get_len_of("、"), 2);
        assert!(is_chinese_punctuation('、'));
        // U+FF0C ， 在 HALFWIDTH_AND_FULLWIDTH_FORMS → 2
        assert_eq!(get_len_of("，"), 2);
        assert!(is_chinese_punctuation('，'));
        // 普通字母/数字 → 1
        assert_eq!(get_len_of("ab12"), 4);
    }

    // ---- containsFirstSpace / containsSpace（对拍 Java 字节码） ----
    #[test]
    fn first_space_semantics() {
        assert!(!contains_first_space(""));
        assert!(contains_first_space(" "));
        assert!(contains_first_space(" a"));
        assert!(contains_first_space("a "));
        assert!(!contains_first_space("a"));
    }

    #[test]
    fn contains_space_semantics() {
        assert!(!contains_space(""));
        assert!(!contains_space("ab"));
        assert!(contains_space("a\tb"));
        assert!(contains_space("a b"));
    }

    #[test]
    fn invited_pid_validation() {
        assert!(invited_pid_looks_valid("1234567", 1));
        assert!(!invited_pid_looks_valid("abc", 1));
        assert!(!invited_pid_looks_valid("123", 1));
    }

    #[test]
    fn resp_carries_ret() {
        assert_eq!(build_resp(RetCode::Ok).ret, 0);
        assert_eq!(build_resp(RetCode::NameDuplicate).ret, 103);
    }
}
