//! 运行时接线层：把纯函数挂到 `cmdId → 处理器` 上。
//!
//! ## 为什么需要这一层
//! 各业务模块（[`crate::item`]、[`crate::sign`] …）只暴露**无 IO 的纯函数**，
//! 依赖通过 trait 注入。这一层负责把裸字节翻译成入参、把出参编码回字节。
//!
//! ## `FnHandler` 的作用
//! 20 多条消息如果每条写一个 `struct + impl ErasedHandler`，光样板就要上千行。
//! 这里用闭包 + 一个泛型适配器，每条消息压缩到 10 行以内：
//!
//! ```ignore
//! table.insert(Box::new(FnHandler::new(11002, move |ctx, body| {
//!     let req = ItemUseReq11002::decode(body).map_err(bad)?;
//!     // …调纯函数、编码回包
//! })));
//! ```
//!
//! ## 过渡期状态：`MemWorld`
//! 真正的持久化（PostgreSQL）在 `ddj-game-core` 的仓储层，但背包/邮件/签到/琥珀
//! 的仓储接口还没实现。为了**先让链路可回归**（能被打到、错误码正确），
//! 这里用进程内内存态顶住：
//!
//! - 好处：12 条消息立刻进入冒烟覆盖，后续换 PG 只需替换 `MemWorld`；
//! - 边界：进程重启即丢，**不能当生产用**；空模板表下走到的是「没有道具/没有邮件」
//!   这类分支，恰好可以用来验证错误码。
//!
//! 模板数据（`useLimit`、签到奖励、邮件模板）等接入 `ddj-template` 后，
//! 由 `Lookup` 实现从 `TemplateStore` 读取，本文件不用改。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use bytes::Bytes;
use ddj_core::{AppError, PlayerId, Result};
use ddj_registry::{ErasedHandler, HandlerCtx, Reply};

use crate::amber::AmberModelState;
use crate::mail::MailRecord;
use crate::sign::SignState;

/// 过渡期内���态：一个玩家一份。
#[derive(Debug, Default)]
pub struct MemWorld {
    /// 玩家状态容器（按 `player_id` 分片）。
    players: Mutex<HashMap<i64, PlayerBox>>,
    /// 焚香祈愿的全服公告栏（原版 `PrayModule.noticeList`）。
    ///
    /// ⚠ 原版 `AbsSysActivityModule` 是进程级单例，所以这里的公告栏也是
    /// 全服共享、重启即丢。`PrayModule.handleEvent` 里 `while (size > 5) remove(0)`。
    pray_notices: Mutex<Vec<crate::pray::Notice>>,
}

/// 门客的成长/亲密度/皮肤等级（`Pet` 里与本流程相关的累加字段）。
#[derive(Debug, Clone, Default)]
pub struct PetExtra {
    /// `Pet.growth`。
    pub growth: i64,
    /// `Pet.intimacy`。
    pub intimacy: i64,
    /// `skinId → 皮肤等级`（`Pet.skins` 里每个 `PetSkin.lv`）。
    pub skin_lv: HashMap<i32, i32>,
    /// `Pet.interact`：互动累计值（`addInteract` 的落点）。
    pub interact: i64,
}

/// 单个玩家的全部内存态。
#[derive(Debug, Default)]
pub struct PlayerBox {
    /// `Player.wealRewardStatus`：福利领奖状态，非 0 表示已领。
    pub weal_reward_status: i32,
    /// 背包：`templateId → num`。
    pub bag: HashMap<i32, i64>,
    /// 时效道具：`id → (templateId, num, deadline)`。
    pub effective: HashMap<i64, (i32, i64, i64)>,
    /// 邮件箱，按发送时间保序。
    pub mails: Vec<MailRecord>,
    /// 每日签到。
    pub sign: SignState,
    /// 琥珀会员。
    pub amber: AmberModelState,
    /// 客户端缓存（`1111` 写 / `1112` 读）。
    pub cache: HashMap<i32, String>,
    /// 新手引导 id。
    pub guide_id: i32,
    /// 新手引导进度（`List<Integer>`）。
    pub guide_list: Vec<i32>,
    /// 角色名。
    pub name: String,
    /// 身份等级（`S-身份表-Status.csv`）。
    pub level: i32,
    /// 当前经验。
    pub exp: i64,
    /// `p.isUpdateName()`：免费改名是否已用掉。
    pub update_name_used: bool,
    /// `p.getTimeOffForbidResetName()`：改名冷却截止。
    pub forbid_reset_name_until: i64,
    /// 性别（`1` 男 / `2` 女，`1024` 可改）。
    pub sex: i32,
    /// `Player.cityId` 当前定位城市。
    pub city_id: i32,
    /// `Player.resetAddressTimes`：已改定位次数。
    pub reset_address_times: i32,
    /// 客户端缓存回写（`1100` 写，原版 `Player.newPlayerData`）。
    pub new_player_data: crate::player::PlayerCacheStore,
    /// `Player.manifesto`：出站宣言（`Player.java:287`，列宽 `BATTLE_NOTICE_SIZE`）。
    pub manifesto: String,
    /// `PlayerCommon` 的 `keyValue` 表（原版 `TypePlayerCacheKey` → 字符串）。
    ///
    /// 目前只用到 `TWBindAccount(2)`，但保留通用 map 以免以后每加一个 key
    /// 都要改结构体。
    pub key_values: HashMap<i32, String>,
    /// `ChannelPlayer.bor`（`ChannelPlayer.java:15`，默认 false）。
    ///
    /// 1004 会无条件置 true；`AbsPurposePlayPlayer.getCheatReturnData()`
    /// 用它判断连接是否走过更新包。会话级状态，断线即失效。
    pub bor: bool,
    /// `Player.createdateTime`（创角时间，毫秒）。
    pub create_time: i64,
    /// `Player.vvipTakeRewardTime`（上次领 VVIP 补发奖励的时间，毫秒）。
    pub vvip_take_reward_time: i64,
    /// `PlayerAdvertising.adRewardTimes`：今日已看广告领奖次数。
    pub ad_reward_times: i32,
    /// `PlayerAdvertising.dayTime`：用来判断是否隔天（原版 `refresh()` 用）。
    pub ad_reward_day: i64,
    /// 门客皮肤状态：`pet_id → PetSkinState`（原版 `PetSet.allPet` 的子集）。
    pub pet_skins: HashMap<i32, crate::pet::PetSkinState>,
    /// 门客的成长/亲密度/皮肤等级（原版 `Pet` 的可见字段子集）。
    pub pet_extra: HashMap<i32, PetExtra>,
    /// 门客评级等级：`pet_id → Pet.rataLv`（默认 1）。
    pub pet_rate: HashMap<i32, i32>,
    /// 门客星级：`pet_id → Pet.star`（默认 0）。
    pub pet_star: HashMap<i32, i32>,
    /// 技能阶级：`(pet_id, skill_id) → Pet.skillClass[offset]`。
    pub pet_skill_class: HashMap<(i32, i32), i32>,
    /// 技能等级：`(pet_id, skill_id) → Pet.skillLv[offset]`。
    pub pet_skill_lv: HashMap<(i32, i32), i32>,
    /// 皮肤技能等级：`skill_id → Pet.skinSkillLv`。
    pub pet_skin_skill_lv: HashMap<i32, i32>,
    /// 二级密码状态（原版 `SecondPsw` 挂在 `SysActivitySet` 下）。
    pub second_psw: crate::second_psw::SecondPswState,
    /// 防骗手册状态（原版 `AntiCheatModel`）。
    pub anticheat: crate::anticheat::AntiCheatState,
    /// 计数器集合自身的 `time`（原版 `CounterRecoverySet.time`）。
    ///
    /// 用来判断「跨天 / 跨周」；`0` 表示尚未 dbInit。
    pub counter_set_time: i64,
    /// 计数器 `(templateId, param1) → CounterState`（原版 `allCounterRecovery`）。
    ///
    /// ⚠ 原版是两层 map（先 templateId 再 param1），这里摊平成二元组键，
    /// 语义等价；`param1` 只有 `templateId == 5`（子嗣）才非 0。
    pub counter_map: HashMap<(i32, i64), crate::counter::CounterState>,
    /// `Player.channelId`：登录时从库里读回（创角时写入）。
    ///
    /// 17301/17101/17081 等按渠道判定的消息都靠它，
    /// 见 `crates/data/db/migrations/0006_player_channel.sql`。
    pub channel_id: i32,
    /// `Player.cid`：同上。
    pub cid: String,
    /// `ShortcutModel.state`：0 未保存 / 1 待领 / 2 已领。
    ///
    /// 原版是 `sys_activity_shortcut_install` 表的一行，键 `playerId`。
    pub shortcut_state: i32,
    /// `FavourableModel`：渠道好评的持久化字段。
    pub favourable: crate::favourable::FavourableState,
    /// `PrayModel`：焚香祈愿的持久化字段。
    pub pray: crate::pray::PrayState,
    /// `CornucopiaActPlayer.records`：聚宝盆自己的抽奖记录（38303 回显）。
    ///
    /// 原版是 `activity_cornucopia_player` 表的一列（`ConcurrentLinkedQueue`），
    /// 过渡期只放内存。真正的写入在 38302（抽奖），届时再决定是否迁 PG。
    pub cornucopia_records: Vec<crate::install_cornucopia::LuckRecord>,
}

impl MemWorld {
    /// 建一个空世界。
    #[must_use]
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// 在玩家状态上执行一段临界区代码。
    ///
    /// 用 `std::sync::Mutex` 而不是异步锁：所有临界区都是纯内存操作，不跨 `await`，
    /// 持锁时间在微秒级。若换成 `tokio::sync::Mutex` 反而会因为跨 `.await` 而更慢。
    ///
    /// # Panics
    /// 只在前一个持锁线程 panic（中毒）时 panic —— 那时进程状态已不可信。
    pub fn with<T>(&self, player: PlayerId, f: impl FnOnce(&mut PlayerBox) -> T) -> T {
        let mut guard = self
            .players
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        f(guard.entry(player.get()).or_insert_with(||
            PlayerBox {
                level: 1,
                ..PlayerBox::default()
            },
        ))
    }

    /// 焚香祈愿的全服公告栏（原版 `PrayModule.noticeList`，进程级）。
    ///
    /// 跨玩家共享，所以挂在 `MemWorld` 而不是 `PlayerBox`。
    pub fn pray_notices(&self) -> std::sync::MutexGuard<'_, Vec<crate::pray::Notice>> {
        self.pray_notices
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// 只读访问。
    pub fn peek<T>(&self, player: PlayerId, f: impl FnOnce(&PlayerBox) -> T) -> T {
        self.with(player, |b| f(b))
    }
}

/// `prost::DecodeError` → `AppError`（原版对应「解包失败」直接断开）。
pub fn decode_err(e: prost::DecodeError) -> AppError {
    AppError::BadRequest(format!("请求解包失败: {e}"))
}

/// 闭包式处理器：每条消息只需一个闭包。
pub struct FnHandler<F> {
    cmd: i32,
    requires_login: bool,
    f: F,
}

impl<F> FnHandler<F>
where
    F: Fn(&HandlerCtx, Bytes) -> Result<Option<Reply>> + Send + Sync + 'static,
{
    /// 需要登录的消息。
    pub fn new(cmd: i32, f: F) -> Self {
        Self {
            cmd,
            requires_login: true,
            f,
        }
    }

    /// 免登录消息（对应原版白名单 `0,3,4,5,6,7,11,98`）。
    #[must_use]
    pub fn free(mut self) -> Self {
        self.requires_login = false;
        self
    }
}

#[async_trait]
impl<F> ErasedHandler for FnHandler<F>
where
    F: Fn(&HandlerCtx, Bytes) -> Result<Option<Reply>> + Send + Sync + 'static,
{
    fn cmd_id(&self) -> i32 {
        self.cmd
    }

    fn requires_login(&self) -> bool {
        self.requires_login
    }

    async fn handle_raw(&self, ctx: &HandlerCtx, body: Bytes) -> Result<Option<Reply>> {
        (self.f)(ctx, body)
    }
}

/// 把 `MsgResources` 列表编码成回包字段用的 `(id, num)` 对。
///
/// `MsgResources.num` 在 proto 里是 **string**（`optional string num = 2;`），
/// 原版也是 `String.valueOf(num)`，所以这里统一转字符串。
#[must_use]
pub fn resources_to_msg(list: &[(i32, i64)]) -> Vec<ddj_pb::gen::common_msg_message::MsgResources> {
    list.iter()
        .map(|(id, num)| ddj_pb::gen::common_msg_message::MsgResources {
            id: Some(*id),
            num: Some(num.to_string()),
            ..Default::default()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_is_per_player() {
        let w = MemWorld::new();
        let a = PlayerId::new(1);
        let b = PlayerId::new(2);
        w.with(a, |p| p.bag.insert(100, 5));
        assert_eq!(w.peek(a, |p| p.bag.get(&100).copied()), Some(5));
        assert_eq!(w.peek(b, |p| p.bag.get(&100).copied()), None);
    }

    #[test]
    fn resources_num_is_string() {
        let got = resources_to_msg(&[(1, 50), (22, 1)]);
        assert_eq!(got[0].id, Some(1));
        assert_eq!(got[0].num.as_deref(), Some("50"));
        assert_eq!(got[1].num.as_deref(), Some("1"));
    }
}

// ────────────────────────── 异步处理器 ──────────────────────────

/// 需要 `await`（查库、跨服 RPC）的处理器。
///
/// `FnHandler` 的闭包是同步的，查 PG 放不进去。原版这些 handler 在 Netty 线程里
/// 直接读缓存，Rust 侧缓存未铺开前只能走 IO，所以单开一个异步外壳。
pub struct AsyncHandler<F> {
    cmd: i32,
    requires_login: bool,
    f: F,
}

impl<F, Fut> AsyncHandler<F>
where
    F: Fn(HandlerCtx, Bytes) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<Option<Reply>>> + Send + 'static,
{
    /// 需要登录的消息。
    pub fn new(cmd: i32, f: F) -> Self {
        Self {
            cmd,
            requires_login: true,
            f,
        }
    }
}

#[async_trait]
impl<F, Fut> ErasedHandler for AsyncHandler<F>
where
    F: Fn(HandlerCtx, Bytes) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<Option<Reply>>> + Send + 'static,
{
    fn cmd_id(&self) -> i32 {
        self.cmd
    }

    fn requires_login(&self) -> bool {
        self.requires_login
    }

    async fn handle_raw(
        &self,
        ctx: &HandlerCtx,
        body: Bytes,
    ) -> Result<Option<Reply>> {
        (self.f)(ctx.clone(), body).await
    }
}
