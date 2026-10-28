//! 消息号常量与通用错误码
//!
//! 消息号全部来自 proto message 名后缀：`ItemUseReq_11002` → `11002`。
//! 由 `cargo xt gen-cmds` 生成；这里手写高频命令，其余在生成时补齐。

/// 登录相关（原 `LoginCmdId`）。
pub mod login {
    /// 版本检查。
    pub const VERSION: i32 = 0;
    /// 获取区服列表。
    pub const SERVER_LIST: i32 = 3;
    /// 登录（**唯一加密的消息**，XXTEA + Base64）。
    pub const LOGIN: i32 = 4;
    /// 取可用名字（`GetPlayerNamesReq_5` 实际是 5 号之外的独立号，见 proto）。
    pub const NAMES: i32 = 5;
    /// 心跳。
    pub const HEARTBEAT: i32 = 5;
    /// 断线重连。
    pub const RECONNECT: i32 = 6;
    /// 登出。
    pub const LOGOUT: i32 = 7;
    /// 公告。
    pub const NOTICE: i32 = 11;
    /// 服务器时间。
    pub const SERVER_TIME: i32 = 98;
}

/// 道具相关。
pub mod item {
    /// 使用道具 —— 本次逆向的起点。
    pub const USE_REQ: i32 = 11002;
    /// 合成道具。
    pub const COMPOUND_REQ: i32 = 11003;
    /// 背包变化推送（`ItemUpdateResp_11004`，服务端主动发）。
    pub const UPDATE_RESP_11004: i32 = 11004;
    /// 时效道具使用。
    pub const EFFECTIVE_USE_REQ: i32 = 11005;
    /// 时效道具一键使用。
    pub const EFFECTIVE_ONE_KEY_REQ: i32 = 11006;
    /// 时效道具变化推送（`ItemEffectiveUpdateResp_11007`）。
    pub const EFFECTIVE_UPDATE_RESP_11007: i32 = 11007;
    /// 过期道具确认。
    pub const CONFIRM_OVERDUE_REQ: i32 = 11008;
}

/// 通用错误码（`bf.common.utils.LanguageUtils`）。
///
/// ## 来源
/// 用 `tools/jclass2.py` 反汇编 `jb3.common.v1.jar` 里
/// `bf/common/utils/LanguageUtils.class` 的 `<clinit>`，逐个枚举常量取第二个
/// `push` 操作数（第一个是 ordinal，第二个才是 `value`，也就是协议里的 `ret`）。
///
/// ## 为什么不能自己编号
/// 客户端拿到 `ret` 后去多语言表查文案，`ret` 本身就是协议的一部分。
/// 少一个码最多显示空白，错一个码会给玩家看错提示。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum RetCode {
    /// 成功。
    Ok = 0,
    /// 服务器正在维护中。
    ServerWeihu = 1,
    /// 服务器繁忙，请稍后再试!
    ServerBusy = 2,
    /// 服务器停服了。
    ServerStop = 3,
    /// 当前服务器人数过多。
    MaxPlayerNum = 4,
    /// 金币不够。
    MoneyNo = 5,
    /// 资源不够。
    ResourcesNo = 6,
    /// 元宝不够。
    JewelNo = 7,
    /// 后台把玩家踢下线了。
    GmOffline = 9,
    /// 被封禁了，不可以登录。
    NotSignIn = 10,
    /// 被禁言了，不可以聊天。
    NotChat = 11,
    /// 您输入的字符有非法字符。
    StrIllegality = 12,
    /// 您输入的字符太长。
    StrTooLong = 13,
    /// 您还没有开启该系统。
    SysNoOpen = 14,
    /// 访问频率限制。
    RequestLimit = 15,
    /// 跨服服务器繁忙。
    CrossServerError = 16,
    /// VIP 等级不够。
    VipNo = 17,
    /// 家族等级不够。
    JiazuNo = 18,
    /// 财运等级不够。
    CaiyunNo = 19,
    /// 当前不能注册新用户了。
    RegisterNo = 20,
    /// 活动已结束。
    ActivityEnd = 21,
    /// 活动还未结束。
    ActivityEndNo = 22,
    /// 服务器开没有开服。
    NoOpen = 23,
    /// 操作太频繁。
    OperateTooOften = 24,
    /// 财力不足。
    MoneyLimit = 25,
    /// 道具使用所需资源消耗不够。
    UseCostResourcesNo = 26,
    /// 功能维护中，请耐心等待！
    ChatNoOpen = 27,
    /// 开服天数不够。
    ServerOpenDayNo = 28,
    /// 参数错误。
    ParamError = 29,
    /// 阶段错误。
    StageError = 30,
    /// 体力不足。
    EnergyNotEnough = 31,
    /// 免费次数不足。
    BuyLimitFreeTimes = 32,
    /// 玩家等级不足。
    PlayerLvNo = 33,
    /// 购买次数达到上限。
    BuyLimitTimes = 34,
    /// 活动被封禁。
    ActivityForbid = 35,
    /// 瓦肆服务器繁忙。
    HallServerBusy = 36,
    /// 跨服榜单分组不同。
    RankGroupError = 37,
    /// 输入财力不符合规范。
    InputMoneyError = 38,
    /// 账号被限制登录该区服。
    LoginForbidden = 39,
    /// 客户端版本不对（`LoginResp_4.ret` 注释点名的 99）。
    VersionNo = 99,
    /// 该客户端之前已经登录过了!
    RepeatLogin = 101,
    /// 您的账号在其它地方抢登了。
    Kicked = 102,
    /// 玩家名字重复了。
    NameDuplicate = 103,
    /// 玩家名字有非法字符。
    NameIllegal = 104,
    /// 玩家名字长度不符合。
    NameLength = 105,
    /// 您还没有登录平台。
    NotLoginPlatform = 106,
    /// 登录时已经加入到排队列里面去了。
    InQueue = 107,
    /// 琥珀福利：不是琥珀/HeyTap 会员（`AMBER_WELFARE_IS_NOT_VIP`）。
    AmberWelfareIsNotVip = 4701,
    /// 琥珀福利：签到失败（`AMBER_WELFARE_SIGN_ERROR`）。
    AmberWelfareSignError = 4702,
    /// 琥珀福利：今天不是专属奖励日（`AMBER_WELFARE_IS_NOT_EXCLUSIVE_DAY`）。
    AmberWelfareIsNotExclusiveDay = 4703,
    /// 琥珀福利：专属奖励领取失败（`AMBER_WELFARE_EXCLUSIVE_ERROR`）。
    AmberWelfareExclusiveError = 4704,
    /// 琥珀福利：充值礼包领取失败（`AMBER_WELFARE_CHARGE_GIFT_ERROR`）。
    AmberWelfareChargeGiftError = 4705,
    /// 琥珀福利：充值礼包未达标（`AMBER_WELFARE_CHARGE_GIFT_ERROR2`）。
    AmberWelfareChargeGiftError2 = 4706,
    /// 今天已经签过到了（`SIGN_ERROR_REPEATED`）。
    SignErrorRepeated = 1201,
    /// 这个签到宝箱已经领过了（`SIGN_ERROR_REPEATED_BOX`）。
    SignErrorRepeatedBox = 1202,
    /// 签到天数还不够开这个宝箱（`SIGN_ERROR_NOT_SIGNED`）。
    SignErrorNotSigned = 1203,
    /// 邮件领取失败——请求里没有任何一封可领（`MAIL_ERROR_1`）。
    MailError1 = 301,
    /// 改名冷却未结束（`PLAYER_ERROR_23`）。
    PlayerError23 = 131,
    /// 礼包码不存在。
    PlayerError10 = 110,
    /// 改定位次数已用尽 / 无免费次数。
    PlayerError15 = 115,
    /// 目标城市不存在。
    PlayerError16 = 116,
    /// 免费改定位只能用一次。
    PlayerError21 = 121,
    /// 已经在该城市。
    PlayerError22 = 122,
    /// 道具合成 - 资源不够（`ITEM_ERROR_1`）。
    ItemError1 = 1101,
    /// 道具数量不足（`ITEM_ERROR_2`）。
    ItemError2 = 1102,
    /// 道具错误 3（`ITEM_ERROR_3`）。
    ItemError3 = 1103,
    /// 道具错误 4（`ITEM_ERROR_4`）。
    ItemError4 = 1104,
    /// 道具错误 5（`ITEM_ERROR_5`）。
    ItemError5 = 1105,
    /// 时效道具已过期（`ITEM_ERROR_11`）。
    ItemError11 = 1111,
    /// 宣言中有非法字符（`DECLARE_ERROR_STRING_ILLEGAL`）。
    DeclareErrorStringIllegal = 2303,
    /// 宣言的长度不符（`DECLARE_ERROR_LONG_ERROR`）。
    DeclareErrorLongError = 2304,
    /// 你当前没有权限修改宣言（`FORTUNE_ERROR_NO_JURISDICTION`）。
    FortuneErrorNoJurisdiction = 2305,
    /// 鉴宝大会 - 已领取（`APPRAISAL_GAIN_ACHIEVEMENT`）。
    AppraisalGainAchievement = 4601,
    /// 观看广告奖励领取次数达到上限（`PLAYER_ERROR_20`）。
    PlayerError20 = 120,
    /// 人才 - 门客尚未解锁（`TALENT_ERROR_5`）。
    TalentError5 = 405,
    /// 人才 - 皮肤不属于该门客（`TALENT_ERROR_6`）。
    TalentError6 = 406,
    /// 人才 - 还未获得皮肤（`TALENT_ERROR_7`）。
    TalentError7 = 407,
    /// 宠物系统还未开放（`PET_ERROR_UNOPEN_SYSTEM`）。
    PetErrorUnopenSystem = 914,
    /// VIP 等级不足（`PET_ERROR_INTERACT_ALL_VIP`）。
    PetErrorInteractAllVip = 915,
    /// 激活皮肤资源不足（`PET_ERROR_ACTIVATE_SKIN_RESOURCES`）。
    PetErrorActivateSkinResources = 938,
    /// 升皮肤资源不足（`PET_ERROR_UPGRADE_SKIN_RESOURCES`）。
    PetErrorUpgradeSkinResources = 939,
    /// 珍兽数量太少（`PET_ERROR_PET_NUM_LIMIT`）。
    PetErrorPetNumLimit = 940,
    /// 珍兽评级尚未开启（`PETRATE_IS_NOT_OPEN`）。
    PetRateIsNotOpen = 3600,
    /// 珍兽评级 - 玩家没有该珍兽（`PETRATE_HAVE_NOT_PET`）。
    PetRateHaveNotPet = 3601,
    /// 珍兽评级 - 已经是最大等级（`PETRATE_REACH_MAX_LEVEL`）。
    PetRateReachMaxLevel = 3602,
    /// 珍兽评级 - 没有该等级配置（`PETRATE_HAVE_NOT_CONFIG`）。
    PetRateHaveNotConfig = 3603,
    /// 珍兽评级 - 没有满足升级条件（`PETRATE_NOT_ENOUGHT_CONDISION`）。
    PetRateNotEnoughtCondision = 3604,
    /// 珍兽评级 - 预定等级珍兽已达最大数量（`PETRATE_REACH_MAX_NUM`）。
    PetRateReachMaxNum = 3606,
    /// 宠物 - 未获得宠物（`PET_ERROR_NOT_EXIST_PET`）。
    PetErrorNotExistPet = 901,
    /// 宠物 - 精力不足（`PET_ERROR_TEMPLATE_2_LIMIT`）。
    PetErrorTemplate2Limit = 905,
    /// 十连互动资源不足（`PET_ERROR_TEN_TIMES_COST`）。
    PetErrorTenTimesCost = 942,
    /// 宠物 - 已经是最大星级（`PET_ERROR_MAX_START`）。
    PetErrorMaxStart = 906,
    /// 宠物 - 亲密度不足（`PET_ERROR_INTIMACY_LIMIT`）。
    PetErrorIntimacyLimit = 907,
    /// 宠物 - 成长值不足（`PET_ERROR_GROWTH_LIMIT`）。
    PetErrorGrowthLimit = 908,
    /// 宠物 - 技能未解锁（`PET_ERROR_SKILL_LOCK`）。
    PetErrorSkillLock = 909,
    /// 宠物 - 技能升阶所需资源不足（`PET_ERROR_SKILL_CLASS_LIMIT`）。
    PetErrorSkillClassLimit = 911,
    /// 宠物 - 技能已达最大等级（`PET_ERROR_SKILL_MAX_LV`）。
    PetErrorSkillMaxLv = 912,
    /// 宠物 - 技能已达最大阶级（`PET_ERROR_SKILL_MAX_CLASS`）。
    PetErrorSkillMaxClass = 913,
    /// 宠物 - 互动经验不足（`PET_ERROR_INTERACTION_LIMIT`）。
    PetErrorInteractionLimit = 910,
    /// 珍兽 - 升级条件不满足（`PET_ERROR_CONDITION_NOT_MET`）。
    PetErrorConditionNotMet = 946,
    /// 二级密码 - 长度错误（`SECOND_PSW_LENGTH_ERROR`）。
    SecondPswLengthError = 9501,
    /// 二级密码 - 只能是数字（`SECOND_PSW_FORMAT_ERROR`）。
    SecondPswFormatError = 9502,
    /// 二级密码 - 还未验证（`SECOND_PSW_NO_VERIFY`）。
    SecondPswNoVerify = 9503,
    /// 二级密码 - 验证次数不足（`SECOND_PSW_VERIFY_NUM_LIMIT`）。
    SecondPswVerifyNumLimit = 9504,
    /// 二级密码 - 密码错误（`SECOND_PSW_PSW_ERROR`）。
    SecondPswPswError = 9505,
    /// 防骗手册 - 未阅读完成无法领取奖励（`ANTICHEAT_READ`）。
    AnticheatRead = 6451,
    /// 冲榜活动 - 奖励已领取不能重复领（`ACTIVITY_RANK_LIST_ERROR_REPEATED`）。
    ActivityRankListErrorRepeated = 1501,
    /// 保存桌面 - 不是指定渠道（`SHORTCUT_CID_ERROR`）。
    ShortcutCidError = 4731,
    /// 保存桌面 - 尚未执行保存桌面（`SHORTCUT_NOT_INSTALL`）。
    ShortcutNotInstall = 4732,
    /// 渠道好评 - 渠道没开放好评系统（`FAVOURABLE_ERROR_1`）。
    FavourableError1 = 3941,
    /// 渠道好评 - 已经评价过（`FAVOURABLE_ERROR_2`）。
    FavourableError2 = 3942,
    /// 活动进行中 - 参数错误（`ACTIVITY_PARAM_ERROR`）。
    ActivityParamError = 2754,
    /// 活动不存在（`ACTIVITY_NO_EXITS`）。
    ActivityNoExits = 2751,
    /// 活动已结束（`ACTIVITY_NO_OPEN`）。
    ActivityNoOpen = 2752,
    /// 焚香祈愿 - 未开放（`PRAY_ERROR_UN_OPEN`）。
    PrayErrorUnOpen = 3901,
    /// 焚香祈愿 - 资源不足（`PRAY_ERROR_COST`）。
    PrayErrorCost = 3902,
    /// VIVO 特权 - 领取奖励失败（`VIVO_WELFARE_SIGN_ERROR`）。
    ///
    /// ⚠ 名字是 VIVO 福利的，但 `ShortcutModel.gainShortcutReward` 的
    /// 「已领过」分支用的就是它（原版复制粘贴，照抄）。
    VivoWelfareSignError = 4722,
}

impl RetCode {
    /// 转成协议里的 `ret` 数值。
    #[must_use]
    pub const fn as_i32(self) -> i32 {
        self as i32
    }

    /// 从协议数值还原；未识别的返回 `None`（不要自己造码）。
    #[must_use]
    pub const fn from_i32(v: i32) -> Option<Self> {
        Some(match v {
            0 => Self::Ok,
            1 => Self::ServerWeihu,
            2 => Self::ServerBusy,
            3 => Self::ServerStop,
            4 => Self::MaxPlayerNum,
            5 => Self::MoneyNo,
            6 => Self::ResourcesNo,
            7 => Self::JewelNo,
            9 => Self::GmOffline,
            10 => Self::NotSignIn,
            11 => Self::NotChat,
            12 => Self::StrIllegality,
            13 => Self::StrTooLong,
            14 => Self::SysNoOpen,
            15 => Self::RequestLimit,
            16 => Self::CrossServerError,
            17 => Self::VipNo,
            18 => Self::JiazuNo,
            19 => Self::CaiyunNo,
            20 => Self::RegisterNo,
            21 => Self::ActivityEnd,
            22 => Self::ActivityEndNo,
            23 => Self::NoOpen,
            24 => Self::OperateTooOften,
            25 => Self::MoneyLimit,
            26 => Self::UseCostResourcesNo,
            27 => Self::ChatNoOpen,
            28 => Self::ServerOpenDayNo,
            29 => Self::ParamError,
            30 => Self::StageError,
            31 => Self::EnergyNotEnough,
            32 => Self::BuyLimitFreeTimes,
            33 => Self::PlayerLvNo,
            34 => Self::BuyLimitTimes,
            35 => Self::ActivityForbid,
            36 => Self::HallServerBusy,
            37 => Self::RankGroupError,
            38 => Self::InputMoneyError,
            39 => Self::LoginForbidden,
            99 => Self::VersionNo,
            101 => Self::RepeatLogin,
            102 => Self::Kicked,
            103 => Self::NameDuplicate,
            104 => Self::NameIllegal,
            105 => Self::NameLength,
            106 => Self::NotLoginPlatform,
            107 => Self::InQueue,
            131 => Self::PlayerError23,
            110 => Self::PlayerError10,
            115 => Self::PlayerError15,
            116 => Self::PlayerError16,
            121 => Self::PlayerError21,
            122 => Self::PlayerError22,
            4701 => Self::AmberWelfareIsNotVip,
            4702 => Self::AmberWelfareSignError,
            4703 => Self::AmberWelfareIsNotExclusiveDay,
            4704 => Self::AmberWelfareExclusiveError,
            4705 => Self::AmberWelfareChargeGiftError,
            4706 => Self::AmberWelfareChargeGiftError2,
            1201 => Self::SignErrorRepeated,
            1202 => Self::SignErrorRepeatedBox,
            1203 => Self::SignErrorNotSigned,
            301 => Self::MailError1,
            1101 => Self::ItemError1,
            1102 => Self::ItemError2,
            1103 => Self::ItemError3,
            1104 => Self::ItemError4,
            1105 => Self::ItemError5,
            1111 => Self::ItemError11,
            2303 => Self::DeclareErrorStringIllegal,
            2304 => Self::DeclareErrorLongError,
            2305 => Self::FortuneErrorNoJurisdiction,
            4601 => Self::AppraisalGainAchievement,
            120 => Self::PlayerError20,
            405 => Self::TalentError5,
            406 => Self::TalentError6,
            407 => Self::TalentError7,
            914 => Self::PetErrorUnopenSystem,
            915 => Self::PetErrorInteractAllVip,
            938 => Self::PetErrorActivateSkinResources,
            939 => Self::PetErrorUpgradeSkinResources,
            940 => Self::PetErrorPetNumLimit,
            3600 => Self::PetRateIsNotOpen,
            3601 => Self::PetRateHaveNotPet,
            3602 => Self::PetRateReachMaxLevel,
            3603 => Self::PetRateHaveNotConfig,
            3604 => Self::PetRateNotEnoughtCondision,
            3606 => Self::PetRateReachMaxNum,
            901 => Self::PetErrorNotExistPet,
            905 => Self::PetErrorTemplate2Limit,
            942 => Self::PetErrorTenTimesCost,
            906 => Self::PetErrorMaxStart,
            907 => Self::PetErrorIntimacyLimit,
            908 => Self::PetErrorGrowthLimit,
            909 => Self::PetErrorSkillLock,
            911 => Self::PetErrorSkillClassLimit,
            912 => Self::PetErrorSkillMaxLv,
            913 => Self::PetErrorSkillMaxClass,
            910 => Self::PetErrorInteractionLimit,
            946 => Self::PetErrorConditionNotMet,
            9501 => Self::SecondPswLengthError,
            9502 => Self::SecondPswFormatError,
            9503 => Self::SecondPswNoVerify,
            9504 => Self::SecondPswVerifyNumLimit,
            9505 => Self::SecondPswPswError,
            6451 => Self::AnticheatRead,
            1501 => Self::ActivityRankListErrorRepeated,
            4731 => Self::ShortcutCidError,
            4732 => Self::ShortcutNotInstall,
            4722 => Self::VivoWelfareSignError,
            3901 => Self::PrayErrorUnOpen,
            3902 => Self::PrayErrorCost,
            3941 => Self::FavourableError1,
            3942 => Self::FavourableError2,
            2754 => Self::ActivityParamError,
            2751 => Self::ActivityNoExits,
            2752 => Self::ActivityNoOpen,
            _ => return None,
        })
    }

    /// 中文文案（原 `LanguageUtils` 里写死的那句）。
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::Ok => "成功",
            Self::ServerWeihu => "服务器正在维护中",
            Self::ServerBusy => "服务器繁忙，请稍后再试!",
            Self::ServerStop => "服务器停服了",
            Self::MaxPlayerNum => "当前服务器人数过多",
            Self::MoneyNo => "金币不够",
            Self::ResourcesNo => "资源不够",
            Self::JewelNo => "元宝不够",
            Self::GmOffline => "后台把玩家踢下线了",
            Self::NotSignIn => "被封禁了，不可以登录",
            Self::NotChat => "被禁言了，不可以聊天",
            Self::StrIllegality => "您输入的字符有非法字符",
            Self::StrTooLong => "您输入的字符太长",
            Self::SysNoOpen => "您还没有开启该系统",
            Self::RequestLimit => "访问频率限制",
            Self::CrossServerError => "跨服服务器繁忙",
            Self::VipNo => "VIP等级不够",
            Self::JiazuNo => "家族等级不够",
            Self::CaiyunNo => "财运等级不够",
            Self::RegisterNo => "当前不能注册新用户了",
            Self::ActivityEnd => "活动已结束",
            Self::ActivityEndNo => "活动还未结束",
            Self::NoOpen => "服务器开没有开服",
            Self::OperateTooOften => "操作太频繁",
            Self::MoneyLimit => "财力不足",
            Self::UseCostResourcesNo => "道具使用所需资源消耗不够",
            Self::ChatNoOpen => "功能维护中，请耐心等待！",
            Self::ServerOpenDayNo => "开服天数不够",
            Self::ParamError => "参数错误",
            Self::StageError => "阶段错误",
            Self::EnergyNotEnough => "体力不足",
            Self::BuyLimitFreeTimes => "免费次数不足",
            Self::PlayerLvNo => "玩家等级不足",
            Self::BuyLimitTimes => "购买次数达到上限",
            Self::ActivityForbid => "活动被封禁",
            Self::HallServerBusy => "瓦肆服务器繁忙",
            Self::RankGroupError => "跨服榜单分组不同",
            Self::InputMoneyError => "输入财力不符合规范",
            Self::LoginForbidden => "账号被限制登录该区服",
            Self::VersionNo => "客户端版本不对",
            Self::RepeatLogin => "该客户端之前已经登录过了!",
            Self::Kicked => "您的账号在其它地方抢登了",
            Self::NameDuplicate => "玩家名字重复了",
            Self::NameIllegal => "玩家名字有非法字符",
            Self::NameLength => "玩家名字长度不符合",
            Self::NotLoginPlatform => "您还没有登录平台",
            Self::InQueue => "登录时已经加入到排队列里面去了",
            Self::AmberWelfareIsNotVip => "您还不是琥珀会员",
            Self::AmberWelfareSignError => "签到失败",
            Self::AmberWelfareIsNotExclusiveDay => "今天不是专属奖励日",
            Self::AmberWelfareExclusiveError => "专属奖励领取失败",
            Self::AmberWelfareChargeGiftError => "充值礼包领取失败",
            Self::AmberWelfareChargeGiftError2 => "充值未达标，无法领取该礼包",
            Self::SignErrorRepeated => "今天已经签过到了",
            Self::SignErrorRepeatedBox => "这个签到宝箱已经领过了",
            Self::SignErrorNotSigned => "签到天数还不够开这个宝箱",
            Self::MailError1 => "邮件领取失败",
            Self::PlayerError23 => "改名冷却未结束",
            Self::PlayerError10 => "礼包码不存在",
            Self::PlayerError15 => "没有可用的改定位次数",
            Self::PlayerError16 => "目标城市不存在",
            Self::PlayerError21 => "免费改定位只能使用一次",
            Self::PlayerError22 => "已经在该城市",
            Self::ItemError1 => "道具合成 - 资源不够",
            Self::ItemError2 => "道具数量不足",
            Self::ItemError3 => "道具错误 3",
            Self::ItemError4 => "道具错误 4",
            Self::ItemError5 => "道具错误 5",
            Self::ItemError11 => "时效道具已过期",
            Self::DeclareErrorStringIllegal => "宣言中有非法字符",
            Self::DeclareErrorLongError => "宣言的长度不符",
            Self::FortuneErrorNoJurisdiction => "你当前没有权限修改宣言",
            Self::AppraisalGainAchievement => "鉴宝大会- 已领取",
            Self::PlayerError20 => "观看广告奖励领取次数达到上限",
            Self::TalentError5 => "人才- 门客尚未解锁",
            Self::TalentError6 => "人才- 皮肤不属于改门客",
            Self::TalentError7 => "人才- 还未获得皮肤",
            Self::PetErrorUnopenSystem => "系统还未开放",
            Self::PetErrorInteractAllVip => "VIP等级不足",
            Self::PetErrorActivateSkinResources => "激活皮肤资源不足",
            Self::PetErrorUpgradeSkinResources => "升皮肤资源不足",
            Self::PetErrorPetNumLimit => "珍兽数量太少",
            Self::PetRateIsNotOpen => "珍兽评级--尚未开启",
            Self::PetRateHaveNotPet => "珍兽评级--玩家没有该珍兽",
            Self::PetRateReachMaxLevel => "珍兽评级--珍兽已经是最大等级",
            Self::PetRateHaveNotConfig => "珍兽评级--没有该等级配置",
            Self::PetRateNotEnoughtCondision => "珍兽评级--没有满足升级条件",
            Self::PetRateReachMaxNum => "珍兽评级--预定等级珍兽已经达到最大数量",
            Self::PetErrorNotExistPet => "宠物- 未获得宠物",
            Self::PetErrorTemplate2Limit => "宠物- 精力不足",
            Self::PetErrorTenTimesCost => "十连互动资源不足",
            Self::PetErrorMaxStart => "宠物- 已经是最大星级",
            Self::PetErrorIntimacyLimit => "宠物- 亲密度不足",
            Self::PetErrorGrowthLimit => "宠物- 成长值不足",
            Self::PetErrorSkillLock => "宠物- 技能未解锁",
            Self::PetErrorSkillClassLimit => "宠物- 技能升阶所需资源不足",
            Self::PetErrorSkillMaxLv => "宠物- 技能已经达到最大等级",
            Self::PetErrorSkillMaxClass => "宠物- 技能已经达到最大阶级",
            Self::PetErrorInteractionLimit => "宠物- 互动经验不足",
            Self::PetErrorConditionNotMet => "珍兽- 升级条件不满足",
            Self::SecondPswLengthError => "二级密码- （设置密码）长度错误",
            Self::SecondPswFormatError => "二级密码- （设置密码）密码只能是数字",
            Self::SecondPswNoVerify => "二级密码- 还未验证二级密码",
            Self::SecondPswVerifyNumLimit => "二级密码- （密码验证）次数不足",
            Self::SecondPswPswError => "二级密码- （密码验证）密码错误",
            Self::AnticheatRead => "防骗手册-未阅读完成无法领取奖励",
            Self::ActivityRankListErrorRepeated => "冲榜活动- 奖励已经领取，不能重复领取",
            Self::ShortcutCidError => "保存桌面-不是指定渠道",
            Self::ShortcutNotInstall => "保存桌面-尚未执行保存桌面",
            Self::VivoWelfareSignError => "VIVO特权-领取奖励失败",
            Self::PrayErrorUnOpen => "焚香祈愿-未开放",
            Self::PrayErrorCost => "焚香祈愿-资源不足",
            Self::FavourableError1 => "渠道没开放好评系统",
            Self::FavourableError2 => "已经评价过",
            Self::ActivityParamError => "活动进行中-参数错误",
            Self::ActivityNoExits => "活动不存在",
            Self::ActivityNoOpen => "活动已结束",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_no_is_99_as_proto_says() {
        // LoginResp_4 的注释明确写了「99为客户端版本不对」
        assert_eq!(RetCode::VersionNo.as_i32(), 99);
        assert_eq!(RetCode::from_i32(99), Some(RetCode::VersionNo));
    }

    #[test]
    fn banned_is_10() {
        // 注释：「10, 被封禁了，不可以登录」
        assert_eq!(RetCode::NotSignIn.as_i32(), 10);
        assert_eq!(RetCode::NotSignIn.text(), "被封禁了，不可以登录");
    }

    #[test]
    fn roundtrip_all_variants() {
        for v in [
            0, 1, 2, 3, 4, 5, 6, 7, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
            25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 99, 101, 102, 103, 104,
            105, 106, 107,
            2303, 2304, 2305, 4601, 120,
            405, 406, 407, 914, 915, 938, 939, 940,
            3600, 3601, 3602, 3603, 3604, 3606, 901, 905, 942,
            906, 907, 908, 909, 911, 912, 913, 910, 946,
            9501, 9502, 9503, 9504, 9505, 6451, 1501,
            4731, 4732, 4722, 3941, 3942, 2754, 2751, 2752, 3901, 3902,
        ] {
            let c = RetCode::from_i32(v).unwrap_or_else(|| panic!("缺 {v}"));
            assert_eq!(c.as_i32(), v);
            assert!(!c.text().is_empty());
        }
        // 8 在原枚举里不存在（GM_OFFLINE=9 跳过了 8）；100 同样空缺。
        assert_eq!(RetCode::from_i32(8), None);
        assert_eq!(RetCode::from_i32(100), None);
    }
}
