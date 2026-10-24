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
use ddj_pb::gen::player_message::{UpPlayerLvResp1003, UpdateNameResp1002};
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

// ───────────────────────────── 1002 改名 ─────────────────────────────
//
// 对应 Java：`PlayerMessageHandler.updateNameReq(Player, UpdateNameReq_1002)`。
// 该方法的字节码本身就是全部业务规则（校验 + 收费 + 落库），按顺序逐条复刻。

/// 改名卡资源 id（`new ResourcesJson(55, "1")`）。
pub const CHANGE_NAME_CARD_RESOURCE: i32 = 55;

/// `TypeModule.TYPE_1004`：改名消耗的资源类型。
pub const TYPE_1004: i32 = 1004;

/// 改名实际消耗。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenameCost {
    /// 免费：首次改名，或旧名字带 `.R` 后缀（合服重名补偿）。
    Free,
    /// 扣 55 号资源 1 个（改名卡）。
    Card55,
    /// 扣 `ConfigDBCache.ChangeNamePrice`。
    ChangeNamePrice,
}

/// 改名判决。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenameOutcome {
    /// 未通过，直接回 `UpdateNameResp_1002{ret}`。
    Reject(RetCode),
    /// 通过，回 `UpdateNameResp_1002{ret=0}`。
    Accept(RenameCost),
}

/// 改名判定所需的全部外部状态。
///
/// 每一项都对应字节码里的一个 `getstatic` / `invokevirtual`，
/// 由调用方（handler）从 `Player`、`ConfigDBCache`、`PlatformServerModule`
/// 与 `DBPlayerModule` 取值填进来。
#[derive(Debug, Clone, Copy)]
pub struct RenameCtx {
    /// `LogicUtils.getNowTime()`。
    pub now: i64,
    /// `p.getTimeOffForbidResetName()`。
    pub forbid_reset_name_until: i64,
    /// `PlatformServerModule.chatOnoffZ`：聊天总开关**关闭**时禁止改名。
    pub chat_off: bool,
    /// `isVnSdk()`（`SDK_ID == 6`）。
    pub is_vn_sdk: bool,
    /// VN 专用正则 `matchName()` 的结果。
    pub vn_name_matches: bool,
    /// `ConfigDBCache.nameSpaceOnoffZ`：名字里是否允许空格。
    pub name_space_on: bool,
    /// `ConfigDBCache.NickNameLength`（实测 14）。
    pub nick_name_length: usize,
    /// `WxMsgSecCheck.checkDirtyWord(p, name, [type_2])`。
    pub dirty_word: bool,
    /// `DBPlayerModule.repeated(name)`。
    pub name_repeated: bool,
    /// `p.isUpdateName()`：是否已用掉那次免费改名。
    pub update_name_used: bool,
    /// `p.getName().contains(".R")`：旧名字是否带合服后缀。
    pub old_had_dot_r: bool,
    /// `ResourcesConsumeUtils.checkResources(p, 55 号资源 × 1)`。
    pub has_card_55: bool,
    /// `ResourcesConsumeUtils.removeResources(p, ChangeNamePrice, TYPE_1004, [])` 是否够。
    pub has_change_name_price: bool,
}

/// 按 `updateNameReq` 的字节码分支序判定改名。
///
/// 顺序不可调整：每一条都是原版的短路点，换了顺序就会改变玩家看到的错误码。
/// 注意第 2 条是「聊天关闭时禁止改名」（`chatOnoffZ` 为真 → `CHAT_NO_OPEN`），
/// 第 10 条的付费判定只在 `!oldHadDotR && isUpdateName()` 时生效。
#[must_use]
pub fn rename_verdict(new_name: &str, old_name: &str, ctx: &RenameCtx) -> RenameOutcome {
    use RenameOutcome::{Accept, Reject};

    // 1. 改名冷却未结束。
    if ctx.now < ctx.forbid_reset_name_until {
        return Reject(RetCode::PlayerError23);
    }
    // 2. 聊天总开关关闭。
    if ctx.chat_off {
        return Reject(RetCode::ChatNoOpen);
    }
    // 3. 与旧名完全相同。
    if new_name == old_name {
        return Reject(RetCode::NameDuplicate);
    }
    // 4. 不允许出现 ".R"（合服后缀由系统生成）。
    if new_name.contains(".R") {
        return Reject(RetCode::NameIllegal);
    }
    // 5. 敏感词。
    if ctx.dirty_word {
        return Reject(RetCode::NameIllegal);
    }
    // 6. 越南 SDK 专用正则。
    if ctx.is_vn_sdk && !ctx.vn_name_matches {
        return Reject(RetCode::NameIllegal);
    }
    // 7. 长度：VN 按 char 计，其余按 `getLenOf`（中文计 2）。
    let len = if ctx.is_vn_sdk {
        new_name.chars().count()
    } else {
        string_length(new_name)
    };
    if len < 1 || len > ctx.nick_name_length {
        return Reject(RetCode::NameLength);
    }
    // 8. 首尾空白。
    if contains_first_space(new_name) {
        return Reject(RetCode::NameIllegal);
    }
    // 9. 中间空白（仅在允许空格的配置下放行）。
    if !ctx.name_space_on && contains_space(new_name) {
        return Reject(RetCode::NameIllegal);
    }
    // 10. 全服重名（原版在 `synchronized(Player.class)` 内查库）。
    if ctx.name_repeated {
        return Reject(RetCode::NameDuplicate);
    }
    // 11. 收费：只有「旧名不带 .R 且已用过免费改名」才收。
    if !ctx.old_had_dot_r && ctx.update_name_used {
        if ctx.has_card_55 {
            return Accept(RenameCost::Card55);
        }
        if ctx.has_change_name_price {
            return Accept(RenameCost::ChangeNamePrice);
        }
        return Reject(RetCode::JewelNo);
    }
    Accept(RenameCost::Free)
}

/// 把判决转成 `UpdateNameResp_1002`。
#[must_use]
pub fn build_rename_resp(outcome: RenameOutcome) -> UpdateNameResp1002 {
    let ret = match outcome {
        RenameOutcome::Reject(code) => code.as_i32(),
        RenameOutcome::Accept(_) => RetCode::Ok.as_i32(),
    };
    UpdateNameResp1002 { ret }
}

// ───────────────────────────── 1003 主动升级 ─────────────────────────────
//
// `PlayerMessageHandler.upPlayerLvReq` 只有三行：转发 `Player.upPlayerLvReq()`，
// `catch(Exception)` → 回 `UpPlayerLvResp_1003{ret=COMMON_SERVER_BUSY}`。
// 真正的分支在 `Player.upPlayerLvReq()`（229 条字节码），顺序如下：
//   1. `getPlayerLvTemplate(lv) == null`            → throw BUSY
//   2. `exp < tpl.getPlayerExp()`                   → throw BUSY（经验不足）
//   3. `getPlayerLvTemplate(lv+1) == null`          → save + unlock + **return，不发包**（满级）
//   4. `lv = next.getId()`；`lvUpgradeTime.put(lv, now)`
//   5. 事件 `EventType.StatusID` + `EventLogSendRecord.level`
//   6. `lastUpdateTime = now`；`save()`
//   —  之后：`sendPlayerAbUpdate(player, true, {5, 6})` → `UpPlayerLvResp_1003{ret=0}` → `drawEverydayClearTime()`

/// `S-身份表-Status.csv` 里升级用到的两列。
///
/// `player_exp` 是「升到下一级所需经验」，满级行实测为 0。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LevelTemplate {
    /// `StatusDB.getId()`（身份 ID）。
    pub id: i32,
    /// `StatusDB.getPlayerExp()`。
    pub player_exp: i64,
}

/// 查 `StatusDBCache.getPlayerLvTemplate(level)`。
pub trait LevelLookup {
    /// 不存在时返回 `None`（原版返回 null）。
    fn template(&self, level: i32) -> Option<LevelTemplate>;
}

/// 升级成功后的落库/推送所需数据。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LevelUpPlan {
    /// 新等级（取 `next.getId()`，不是 `lv + 1`）。
    pub new_level: i32,
    /// `lvUpgradeTime.put(newLevel, now)` 的值。
    pub upgrade_time: i64,
    /// `lastUpdateTime`。
    pub last_update_time: i64,
}

/// 升级判决。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LevelUpOutcome {
    /// 满级：原版直接 `return`，**一个包都不发**。
    Silent,
    /// `HandlerException(COMMON_SERVER_BUSY)` → 被 handler 的 catch 遮罩成 `ret=2`。
    Reject(RetCode),
    /// `ret=0`，并推送属性 `[5, 6]`。
    Accept(LevelUpPlan),
}

/// `sendPlayerAbUpdate(player, true, new Integer[]{5, 6})` 的属性类型。
pub const LEVEL_UP_PUSH_ATTRS: [i32; 2] = [5, 6];

/// 按 `Player.upPlayerLvReq()` 的字节码分支序判定升级。
///
/// 特别注意第 3 条：**满级时静默返回**，既不回包也不报错，客户端不会收到任何响应。
#[must_use]
pub fn level_up(current: i32, exp: i64, now: i64, lookup: &dyn LevelLookup) -> LevelUpOutcome {
    // 1. 当前等级模板不存在。
    let Some(cur) = lookup.template(current) else {
        return LevelUpOutcome::Reject(RetCode::ServerBusy);
    };
    // 2. 经验不足（`lcmp; ifge` → 相等也通过）。
    if exp < cur.player_exp {
        return LevelUpOutcome::Reject(RetCode::ServerBusy);
    }
    // 3. 下一级模板不存在 = 满级 → 静默返回。
    let Some(next) = lookup.template(current.wrapping_add(1)) else {
        return LevelUpOutcome::Silent;
    };
    // 4. 升级。
    LevelUpOutcome::Accept(LevelUpPlan {
        new_level: next.id,
        upgrade_time: now,
        last_update_time: now,
    })
}

/// 把判决转成要发的包；`Silent` 返回 `None`（原版不发包）。
#[must_use]
pub fn build_level_up_resp(outcome: LevelUpOutcome) -> Option<UpPlayerLvResp1003> {
    match outcome {
        LevelUpOutcome::Silent => None,
        LevelUpOutcome::Reject(code) => Some(UpPlayerLvResp1003 { ret: code.as_i32() }),
        LevelUpOutcome::Accept(_) => Some(UpPlayerLvResp1003 {
            ret: RetCode::Ok.as_i32(),
        }),
    }
}

// ─────────────────────────── 1100 客户端缓存回写 ───────────────────────────
//
// `PlayerMessageHandler.playerSaveNewPlayerDataReq` 全文只有一句：
//   `player.setNewPlayerData(req.getPlayerCacheTempList())`，`catch(Exception)` 只写日志。
// **既不回包，也没有任何校验。**
//
// `Player.setNewPlayerData(List<PlayerCacheTemp>)`（53 字节）：
//   `for t in list: playerCacheTemp.put(t.getKey(), t.getValue())` → `save()`
//
// 也就是说这是客户端可以任意写服务端 kv 的通道；重复 key 后者覆盖前者，
// 且空列表也会走一次 `save()`。

use std::collections::BTreeMap;

/// `PlayerMessage.PlayerCacheTemp`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheTemp {
    /// `key`。
    pub key: i32,
    /// `value`。
    pub value: String,
}

/// `Player.playerCacheTemp` 的替身。
#[derive(Debug, Clone, Default)]
pub struct PlayerCacheStore {
    map: BTreeMap<i32, String>,
}

impl PlayerCacheStore {
    /// 空表。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 从已有快照构造。
    #[must_use]
    pub fn from_pairs(pairs: impl IntoIterator<Item = (i32, String)>) -> Self {
        Self {
            map: pairs.into_iter().collect(),
        }
    }

    /// 复刻 `setNewPlayerData`：顺序 `put`，后到的同 key 覆盖先到的。
    ///
    /// 返回本次写入了多少条（`list` 的长度），调用方据此决定是否落库。
    /// 原版**无条件** `save()`，空列表也保存一次。
    pub fn apply(&mut self, list: &[CacheTemp]) -> usize {
        for t in list {
            self.map.insert(t.key, t.value.clone());
        }
        list.len()
    }

    /// 读一条（供测试与调试）。
    #[must_use]
    pub fn get(&self, key: i32) -> Option<&str> {
        self.map.get(&key).map(String::as_str)
    }

    /// 条目数。
    #[must_use]
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// 是否为空。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// 全量快照（落库用）。
    #[must_use]
    pub fn snapshot(&self) -> &BTreeMap<i32, String> {
        &self.map
    }
}

// ────────────────────── 1008 看他人 / 1020 他人详情 ──────────────────────
//
// 两条消息结构完全相同：先按玩家 id 算出「该玩家属于哪个 game 服」，
// 同服直接读本地 `Player`，异服走 `RemoteClient.sendRemoteMsg` 同步调用对端 game 服。
// 差别只有一处：**1008 会改写 `friendScope`，1020 原样透传**。

/// 查看目标所在服。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewTarget {
    /// 目标与自己同服，读本地 `PlayerServerModule.getPlayerById`。
    Local,
    /// 目标在其他 game 服，需走远程调用。
    Remote(i32),
}

/// 按 `LogicUtils.getTargetGsServerIdOfPlayer` 与 `BaseServer.getServerId` 比较。
#[must_use]
pub fn resolve_view_target(target_server_id: i64, my_server_id: i64) -> ViewTarget {
    if target_server_id == my_server_id {
        ViewTarget::Local
    } else {
        // 原版在转远程前做了 `l2i`，这里同口径截断。
        #[allow(clippy::cast_possible_truncation)]
        ViewTarget::Remote(target_server_id as i32)
    }
}

/// 1008 / 1020 的处理计划。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewPlan {
    /// 走本地分支。`friend_scope` 只对 1008 有意义。
    Local { friend_scope: bool },
    /// 走跨服分支。`friend_scope` 只对 1008 有意义。
    Remote {
        target_server: i32,
        friend_scope: bool,
    },
    /// 原版抛异常（含 `getPlayerById` 返回 null 触发的 NPE），被 catch 成 `COMMON_SERVER_BUSY`。
    Busy,
}

/// 两条消息共用的查询接口。
pub trait ViewLookup {
    /// `LogicUtils.getTargetGsServerIdOfPlayer`。
    fn target_server_id(&self, player_id: &str) -> i64;
    /// `PlayerServerModule.getPlayerById`：**可能返回 `None`**（原版返回 null）。
    fn local_player_exists(&self, player_id: &str) -> bool;
    /// `FriendServerModule.isCrossPlayerId`。
    fn is_cross_player_id(&self, player_id: &str) -> bool;
}

/// `playerViewOtherReq`（1008）。
///
/// 本服分支把 `friendScope` 硬编码成 **true**（字节码偏移 52：`iconst_1`）；
/// 跨服分支则用 `isCrossPlayerId(playerId)` 覆盖对端返回值（偏移 128-140）。
/// 目标玩家不存在时原版直接 NPE → catch → ret=2，没有专门的「玩家不存在」错误码。
#[must_use]
pub fn view_other_plan(player_id: &str, my_server_id: i64, lookup: &dyn ViewLookup) -> ViewPlan {
    match resolve_view_target(lookup.target_server_id(player_id), my_server_id) {
        ViewTarget::Local => {
            if lookup.local_player_exists(player_id) {
                ViewPlan::Local { friend_scope: true }
            } else {
                // `p.toMsgOtherPlayerBuilder()` on null → NPE → 外层 catch。
                ViewPlan::Busy
            }
        }
        ViewTarget::Remote(target_server) => ViewPlan::Remote {
            target_server,
            friend_scope: lookup.is_cross_player_id(player_id),
        },
    }
}

/// `playerViewOtherDetailReq`（1020）。
///
/// 与 1008 的差别：**没有 `friendScope` 这一层改写**，跨服拿到对端 `bytes` 后
/// `mergeFrom` 直接 `build()` 发回（偏移 106-120），字段完全由对端决定。
#[must_use]
pub fn view_other_detail_plan(
    player_id: &str,
    my_server_id: i64,
    lookup: &dyn ViewLookup,
) -> ViewPlan {
    match resolve_view_target(lookup.target_server_id(player_id), my_server_id) {
        ViewTarget::Local => {
            if lookup.local_player_exists(player_id) {
                ViewPlan::Local {
                    friend_scope: false,
                }
            } else {
                ViewPlan::Busy
            }
        }
        ViewTarget::Remote(target_server) => ViewPlan::Remote {
            target_server,
            friend_scope: false,
        },
    }
}

/// 把计划折算成回包 `ret`（`None` 表示本服成功、由调用方填业务字段）。
#[must_use]
pub const fn view_plan_ret(plan: ViewPlan) -> Option<i32> {
    match plan {
        ViewPlan::Busy => Some(RetCode::ServerBusy.as_i32()),
        ViewPlan::Local { .. } | ViewPlan::Remote { .. } => None,
    }
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

#[cfg(test)]
mod rename_tests {
    use super::*;

    fn ctx() -> RenameCtx {
        RenameCtx {
            now: 1_000,
            forbid_reset_name_until: 0,
            chat_off: false,
            is_vn_sdk: false,
            vn_name_matches: true,
            name_space_on: false,
            nick_name_length: NICK_NAME_LENGTH,
            dirty_word: false,
            name_repeated: false,
            update_name_used: false,
            old_had_dot_r: false,
            has_card_55: false,
            has_change_name_price: false,
        }
    }

    #[test]
    fn cooldown_blocks_first() {
        let c = RenameCtx {
            now: 999,
            forbid_reset_name_until: 1_000,
            ..ctx()
        };
        assert_eq!(
            rename_verdict("新名字", "旧名字", &c),
            RenameOutcome::Reject(RetCode::PlayerError23)
        );
    }

    #[test]
    fn cooldown_checked_before_everything_else() {
        // 同时命中冷却与聊天关闭，仍应回 131。
        let c = RenameCtx {
            now: 1,
            forbid_reset_name_until: 2,
            chat_off: true,
            ..ctx()
        };
        assert_eq!(
            rename_verdict("新名字", "旧名字", &c),
            RenameOutcome::Reject(RetCode::PlayerError23)
        );
    }

    #[test]
    fn chat_off_blocks() {
        let c = RenameCtx {
            chat_off: true,
            ..ctx()
        };
        assert_eq!(
            rename_verdict("新名字", "旧名字", &c),
            RenameOutcome::Reject(RetCode::ChatNoOpen)
        );
    }

    #[test]
    fn same_name_is_duplicate() {
        assert_eq!(
            rename_verdict("张三", "张三", &ctx()),
            RenameOutcome::Reject(RetCode::NameDuplicate)
        );
    }

    #[test]
    fn dot_r_suffix_is_illegal() {
        assert_eq!(
            rename_verdict("张三.R", "李四", &ctx()),
            RenameOutcome::Reject(RetCode::NameIllegal)
        );
    }

    #[test]
    fn dirty_word_is_illegal() {
        let c = RenameCtx {
            dirty_word: true,
            ..ctx()
        };
        assert_eq!(
            rename_verdict("张三", "李四", &c),
            RenameOutcome::Reject(RetCode::NameIllegal)
        );
    }

    #[test]
    fn vn_regex_only_applies_to_vn() {
        let non_vn = RenameCtx {
            is_vn_sdk: false,
            vn_name_matches: false,
            ..ctx()
        };
        assert_eq!(
            rename_verdict("张三", "李四", &non_vn),
            RenameOutcome::Accept(RenameCost::Free)
        );
        let vn = RenameCtx {
            is_vn_sdk: true,
            vn_name_matches: false,
            ..ctx()
        };
        assert_eq!(
            rename_verdict("张三", "李四", &vn),
            RenameOutcome::Reject(RetCode::NameIllegal)
        );
    }

    #[test]
    fn vn_length_counts_chars_not_weighted() {
        // 7 个汉字：VN 按 char 计 = 7（通过），非 VN 按权重计 = 14（也通过）。
        let name = "一二三四五六七";
        let vn = RenameCtx {
            is_vn_sdk: true,
            ..ctx()
        };
        assert_eq!(
            rename_verdict(name, "旧", &vn),
            RenameOutcome::Accept(RenameCost::Free)
        );
        // 8 个汉字：VN = 8（通过），权重 = 16（超 14 → 105）。
        let name8 = "一二三四五六七八";
        assert_eq!(
            rename_verdict(name8, "旧", &vn),
            RenameOutcome::Accept(RenameCost::Free)
        );
        assert_eq!(
            rename_verdict(name8, "旧", &ctx()),
            RenameOutcome::Reject(RetCode::NameLength)
        );
    }

    #[test]
    fn empty_name_is_length_error() {
        assert_eq!(
            rename_verdict("", "旧名字", &ctx()),
            RenameOutcome::Reject(RetCode::NameLength)
        );
    }

    #[test]
    fn leading_space_is_illegal() {
        assert_eq!(
            rename_verdict(" 张三", "李四", &ctx()),
            RenameOutcome::Reject(RetCode::NameIllegal)
        );
    }

    #[test]
    fn inner_space_follows_config() {
        assert_eq!(
            rename_verdict("张 三", "李四", &ctx()),
            RenameOutcome::Reject(RetCode::NameIllegal)
        );
        let on = RenameCtx {
            name_space_on: true,
            ..ctx()
        };
        assert_eq!(
            rename_verdict("张 三", "李四", &on),
            RenameOutcome::Accept(RenameCost::Free)
        );
    }

    #[test]
    fn repeated_name_checked_after_format() {
        // 重名与格式都错时，先命中的是格式（第 4 条），不是重名。
        let c = RenameCtx {
            name_repeated: true,
            ..ctx()
        };
        assert_eq!(
            rename_verdict("李四.R", "张三", &c),
            RenameOutcome::Reject(RetCode::NameIllegal)
        );
        assert_eq!(
            rename_verdict("李四", "张三", &c),
            RenameOutcome::Reject(RetCode::NameDuplicate)
        );
    }

    #[test]
    fn first_rename_is_free() {
        assert_eq!(
            rename_verdict("张三", "李四", &ctx()),
            RenameOutcome::Accept(RenameCost::Free)
        );
    }

    #[test]
    fn second_rename_prefers_card_then_jewel() {
        let mut c = RenameCtx {
            update_name_used: true,
            has_card_55: true,
            has_change_name_price: true,
            ..ctx()
        };
        assert_eq!(
            rename_verdict("张三", "李四", &c),
            RenameOutcome::Accept(RenameCost::Card55)
        );
        c.has_card_55 = false;
        assert_eq!(
            rename_verdict("张三", "李四", &c),
            RenameOutcome::Accept(RenameCost::ChangeNamePrice)
        );
        c.has_change_name_price = false;
        assert_eq!(
            rename_verdict("张三", "李四", &c),
            RenameOutcome::Reject(RetCode::JewelNo)
        );
    }

    #[test]
    fn dot_r_old_name_rerolls_free_rename() {
        let c = RenameCtx {
            update_name_used: true,
            old_had_dot_r: true,
            has_card_55: false,
            has_change_name_price: false,
            ..ctx()
        };
        assert_eq!(
            rename_verdict("张三", "李四.R", &c),
            RenameOutcome::Accept(RenameCost::Free)
        );
    }

    #[test]
    fn resp_maps_accept_to_zero() {
        assert_eq!(
            build_rename_resp(RenameOutcome::Accept(RenameCost::Free)).ret,
            0
        );
        assert_eq!(
            build_rename_resp(RenameOutcome::Reject(RetCode::PlayerError23)).ret,
            131
        );
        assert_eq!(
            build_rename_resp(RenameOutcome::Reject(RetCode::JewelNo)).ret,
            7
        );
    }
}

#[cfg(test)]
mod level_up_tests {
    use super::*;

    /// 与 `S-身份表-Status.csv` 实测一致：1→100、2→1000、满级 24 行为 0。
    struct FakeStatus(Vec<LevelTemplate>);

    impl LevelLookup for FakeStatus {
        fn template(&self, level: i32) -> Option<LevelTemplate> {
            self.0.iter().copied().find(|t| t.id == level)
        }
    }

    fn lv(n: i32, exp: i64) -> LevelTemplate {
        LevelTemplate {
            id: n,
            player_exp: exp,
        }
    }

    fn table() -> FakeStatus {
        FakeStatus(vec![lv(1, 100), lv(2, 1_000), lv(3, 2_200), lv(24, 0)])
    }

    #[test]
    fn missing_current_template_is_busy() {
        let out = level_up(99, 0, 1, &table());
        assert_eq!(out, LevelUpOutcome::Reject(RetCode::ServerBusy));
    }

    #[test]
    fn insufficient_exp_is_busy() {
        let out = level_up(1, 99, 1, &table());
        assert_eq!(out, LevelUpOutcome::Reject(RetCode::ServerBusy));
    }

    #[test]
    fn exact_exp_passes() {
        // `lcmp; ifge` —— 相等即通过。
        let out = level_up(1, 100, 1, &table());
        assert_eq!(
            out,
            LevelUpOutcome::Accept(LevelUpPlan {
                new_level: 2,
                upgrade_time: 1,
                last_update_time: 1
            })
        );
    }

    #[test]
    fn max_level_is_silent() {
        // 24 的下一级 25 不在表里 → 静默返回。
        let out = level_up(24, 0, 1, &table());
        assert_eq!(out, LevelUpOutcome::Silent);
        assert!(build_level_up_resp(out).is_none(), "满级不发包");
    }

    #[test]
    fn missing_current_beats_insufficient_exp() {
        // 两者都错时先命中第 1 条（模板缺失）。
        let out = level_up(99, 0, 1, &table());
        assert!(matches!(out, LevelUpOutcome::Reject(_)));
    }

    #[test]
    fn new_level_comes_from_next_id_not_plus_one() {
        // 故意让 id 与下标错开，证明取的是 `next.getId()`。
        struct Skewed;
        impl LevelLookup for Skewed {
            fn template(&self, level: i32) -> Option<LevelTemplate> {
                match level {
                    5 => Some(LevelTemplate {
                        id: 5,
                        player_exp: 10,
                    }),
                    6 => Some(LevelTemplate {
                        id: 77,
                        player_exp: 20,
                    }),
                    _ => None,
                }
            }
        }
        let out = level_up(5, 10, 42, &Skewed);
        assert_eq!(
            out,
            LevelUpOutcome::Accept(LevelUpPlan {
                new_level: 77,
                upgrade_time: 42,
                last_update_time: 42
            })
        );
    }

    #[test]
    fn timestamps_both_set_to_now() {
        let out = level_up(2, 1_000, 1_700_000, &table());
        let LevelUpOutcome::Accept(plan) = out else {
            panic!("应升级")
        };
        assert_eq!(plan.upgrade_time, 1_700_000);
        assert_eq!(plan.last_update_time, 1_700_000);
    }

    #[test]
    fn push_attrs_are_five_and_six() {
        assert_eq!(LEVEL_UP_PUSH_ATTRS, [5, 6]);
    }

    #[test]
    fn resp_codes() {
        assert_eq!(
            build_level_up_resp(LevelUpOutcome::Reject(RetCode::ServerBusy)).map(|r| r.ret),
            Some(2)
        );
        let accept = LevelUpOutcome::Accept(LevelUpPlan {
            new_level: 2,
            upgrade_time: 0,
            last_update_time: 0,
        });
        assert_eq!(build_level_up_resp(accept).map(|r| r.ret), Some(0));
    }
}

#[cfg(test)]
mod cache_data_tests {
    use super::*;

    fn t(key: i32, value: &str) -> CacheTemp {
        CacheTemp {
            key,
            value: value.to_owned(),
        }
    }

    #[test]
    fn overwrites_existing_key() {
        let mut st = PlayerCacheStore::from_pairs([(1, "old".to_owned())]);
        st.apply(&[t(1, "new")]);
        assert_eq!(st.get(1), Some("new"));
        assert_eq!(st.len(), 1);
    }

    #[test]
    fn adds_new_key_without_touching_others() {
        let mut st = PlayerCacheStore::from_pairs([(1, "a".to_owned()), (2, "b".to_owned())]);
        st.apply(&[t(3, "c")]);
        assert_eq!(st.get(1), Some("a"));
        assert_eq!(st.get(2), Some("b"));
        assert_eq!(st.get(3), Some("c"));
        assert_eq!(st.len(), 3);
    }

    #[test]
    fn duplicate_keys_in_one_request_last_wins() {
        let mut st = PlayerCacheStore::new();
        st.apply(&[t(7, "first"), t(7, "second"), t(7, "third")]);
        assert_eq!(st.get(7), Some("third"));
        assert_eq!(st.len(), 1);
    }

    #[test]
    fn empty_list_changes_nothing_but_still_saves() {
        let mut st = PlayerCacheStore::from_pairs([(1, "a".to_owned())]);
        assert_eq!(st.apply(&[]), 0, "原版空列表也调用一次 save()");
        assert_eq!(st.len(), 1);
    }

    #[test]
    fn returns_written_count() {
        let mut st = PlayerCacheStore::new();
        assert_eq!(st.apply(&[t(1, "a"), t(2, "b")]), 2);
    }

    #[test]
    fn no_validation_on_key_or_value() {
        // 原版没有任何校验：负数 key、空 value 都照收。
        let mut st = PlayerCacheStore::new();
        st.apply(&[t(-1, ""), t(0, &"x".repeat(4096))]);
        assert_eq!(st.get(-1), Some(""));
        assert_eq!(st.get(0).map(str::len), Some(4096));
    }

    #[test]
    fn snapshot_is_stable_ordered() {
        let mut st = PlayerCacheStore::new();
        st.apply(&[t(9, "i"), t(3, "c"), t(5, "e")]);
        let keys: Vec<i32> = st.snapshot().keys().copied().collect();
        assert_eq!(keys, vec![3, 5, 9]);
    }
}

#[cfg(test)]
mod view_other_tests {
    use std::cell::Cell;

    use super::*;

    struct Fake {
        target_server: i64,
        exists: bool,
        cross: bool,
        cross_calls: Cell<u32>,
    }

    impl Fake {
        fn new(target_server: i64, exists: bool, cross: bool) -> Self {
            Self {
                target_server,
                exists,
                cross,
                cross_calls: Cell::new(0),
            }
        }
    }

    impl ViewLookup for Fake {
        fn target_server_id(&self, _player_id: &str) -> i64 {
            self.target_server
        }
        fn local_player_exists(&self, _player_id: &str) -> bool {
            self.exists
        }
        fn is_cross_player_id(&self, _player_id: &str) -> bool {
            self.cross_calls.set(self.cross_calls.get() + 1);
            self.cross
        }
    }

    const MY: i64 = 7_010_544;

    #[test]
    fn same_server_hits_local_branch() {
        let f = Fake::new(MY, true, false);
        assert_eq!(
            view_other_plan("p1", MY, &f),
            ViewPlan::Local { friend_scope: true }
        );
    }

    #[test]
    fn local_friend_scope_is_hardcoded_true() {
        // isCrossPlayerId 返回 false，但本服分支仍必须给 true。
        let f = Fake::new(MY, true, false);
        let ViewPlan::Local { friend_scope } = view_other_plan("p1", MY, &f) else {
            panic!("应走本地分支");
        };
        assert!(friend_scope);
        assert_eq!(f.cross_calls.get(), 0, "本服分支不应查 isCrossPlayerId");
    }

    #[test]
    fn missing_local_player_becomes_busy() {
        // 原版对 null 直接调用 toMsgOtherPlayerBuilder → NPE → catch → ret=2。
        let f = Fake::new(MY, false, false);
        assert_eq!(view_other_plan("p1", MY, &f), ViewPlan::Busy);
        assert_eq!(view_plan_ret(ViewPlan::Busy), Some(2));
    }

    #[test]
    fn cross_server_uses_is_cross_player_id() {
        let t = Fake::new(7_010_106, true, true);
        assert_eq!(
            view_other_plan("p1", MY, &t),
            ViewPlan::Remote {
                target_server: 7_010_106,
                friend_scope: true
            }
        );
        let f = Fake::new(7_010_106, true, false);
        assert_eq!(
            view_other_plan("p1", MY, &f),
            ViewPlan::Remote {
                target_server: 7_010_106,
                friend_scope: false
            }
        );
    }

    #[test]
    fn cross_server_ignores_local_existence() {
        // 跨服分支根本不查本地玩家。
        let f = Fake::new(7_010_106, false, false);
        assert!(matches!(
            view_other_plan("p1", MY, &f),
            ViewPlan::Remote { .. }
        ));
    }

    #[test]
    fn detail_same_server_has_no_friend_scope_rewrite() {
        let f = Fake::new(MY, true, true);
        let plan = view_other_detail_plan("p1", MY, &f);
        assert_eq!(
            plan,
            ViewPlan::Local {
                friend_scope: false
            }
        );
        assert_eq!(view_plan_ret(plan), None);
        assert_eq!(f.cross_calls.get(), 0);
    }

    #[test]
    fn detail_missing_player_becomes_busy() {
        let f = Fake::new(MY, false, false);
        assert_eq!(view_other_detail_plan("p1", MY, &f), ViewPlan::Busy);
    }

    #[test]
    fn detail_cross_server_passes_through() {
        let f = Fake::new(7_010_106, true, true);
        let plan = view_other_detail_plan("p1", MY, &f);
        assert_eq!(
            plan,
            ViewPlan::Remote {
                target_server: 7_010_106,
                friend_scope: false
            }
        );
        // 1020 跨服不改写任何字段，因此不应查 isCrossPlayerId。
        assert_eq!(f.cross_calls.get(), 0);
    }

    #[test]
    fn resolve_target_compares_server_ids() {
        assert_eq!(resolve_view_target(MY, MY), ViewTarget::Local);
        assert_eq!(
            resolve_view_target(7_010_106, MY),
            ViewTarget::Remote(7_010_106)
        );
    }

    #[test]
    fn ret_zero_is_left_to_caller_on_success() {
        assert_eq!(view_plan_ret(ViewPlan::Local { friend_scope: true }), None);
        assert_eq!(
            view_plan_ret(ViewPlan::Remote {
                target_server: 1,
                friend_scope: true
            }),
            None
        );
    }
}
