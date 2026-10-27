//! # 引擎层基础消息（3 心跳 / 5 取随机名）
//!
//! 对应 Java：`bf/game/server/message/handler/EngineMessageHandler.java`
//! - `heartbeatConnectReq`（行 26-32）
//! - `getPlayerNamesReq`（行 40-48）
//!
//! 这两条是**真实客户端最先碰到**的消息：
//! - `3` 心跳：连上就发，回 `serverTime`；
//! - `5` 取名字：创角页要展示 10 个候选名（`e2e/jsclient/DIFF.md` 待办第 2 项）。
//!
//! ## 5 的名字生成规则（`PlayerNameMgr$NameData`）
//! ```text
//! init():
//!   for 姓氏 in PlayerNameDB:
//!     for 名字 in PlayerNameDB:
//!       拼成「姓氏+名字」（en 是 "名 姓"，vi_VN 是 "姓 名"，其余「姓+名」）
//!       若不在 hashSet（已占用的名字）里 → 加入池子
//!   Collections.shuffle(池子)
//!
//! randomNames():
//!   若池子 > 10 → 随机取 10 个（**允许重复**，原版没去重）
//!   若结果为空 → 返回 ["玩家"]
//! ```
//!
//! 三个容易漏的点：
//! 1. 池子 **≤10 时返回 `["玩家"]`**，不是「返回池子里全部」；
//! 2. 抽样 **允许重复**（`useNames.get(randomIndex(size))` 不去重）；
//! 3. `en` 与 `vi_VN` 的拼接顺序不同。
//!
//! ## 3 心跳
//! 无分支：`ret=0` + `serverTime = now`。

use ddj_core::RngSource;
use ddj_pb::gen::engine_message::HeartbeatConnectResp3;

/// 一次最多返回几个候选名（`randomNames` 里的硬编码 `10`）。
pub const MAX_RANDOM_NAMES: usize = 10;

/// 池子不足时的兜底名字（原版 `arrayList.add("玩家")`）。
pub const FALLBACK_NAME: &str = "玩家";

/// 名字的语言类型（`LanguageDBCache.LanguageType`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameLanguage {
    /// `zh_CN` 等：`姓氏 + 名字`。
    ZhCn,
    /// `en`：`名字 + 空格 + 姓氏`。
    En,
    /// `vi_VN`：`姓氏 + 空格 + 名字`。
    ViVn,
}

impl NameLanguage {
    /// 从请求里的 `lan` 字段解析。
    ///
    /// 原版 `LanguageDBCache.LanguageType.get(lan)` 只认 `zh_CN` / `en`；
    /// 传别的会得到 `null` → 后面 `map.get(null)` → NPE → **不回包**（客户端超时）。
    /// 这是原版缺陷（见 `e2e/jsclient/DIFF.md`「语言」一行），这里返回 `None`
    /// 让接线层决定怎么处理。
    #[must_use]
    pub fn parse(lan: &str) -> Option<Self> {
        match lan {
            "zh_CN" => Some(Self::ZhCn),
            "en" => Some(Self::En),
            "vi_VN" => Some(Self::ViVn),
            _ => None,
        }
    }

    /// 按原版规则拼一个名字。
    #[must_use]
    pub fn compose(self, surname: &str, name: &str) -> String {
        match self {
            Self::ZhCn => format!("{surname}{name}"),
            Self::En => format!("{name} {surname}"),
            Self::ViVn => format!("{surname} {name}"),
        }
    }
}

/// `NameData.init()`：构造候选池（姓氏 × 名字的笛卡尔积，排除已占用）。
///
/// 原版遍历的是**同一张** `PlayerNameDB` 列表的两次嵌套，所以姓氏与名字
/// 来自同一张表的不同列。遇到空姓氏/空名字就 `break`（不是 continue），
/// 这是原版的写法 —— 表里出现空行会提前截断。
#[must_use]
pub fn build_name_pool(
    surnames: &[String],
    names: &[String],
    taken: &[String],
    lang: NameLanguage,
) -> Vec<String> {
    let mut pool = Vec::new();
    for surname in surnames {
        // 原版：空姓氏 → break（整个外层循环结束）
        if surname.is_empty() {
            break;
        }
        for name in names {
            // 原版：空名字 → break（内层循环结束，继续下一个姓氏）
            if name.is_empty() {
                break;
            }
            let full = lang.compose(surname, name);
            if !taken.iter().any(|t| t == &full) {
                pool.push(full);
            }
        }
    }
    pool
}

/// `NameData.randomNames()`：从池子里随机取最多 10 个。
///
/// **允许重复**（原版不去重）。池子 ≤10 时返回 `["玩家"]`。
#[must_use]
pub fn random_names(pool: &[String], rng: &dyn RngSource) -> Vec<String> {
    // 原版是 `if (size > 10)`，所以正好 10 个也走兜底
    if pool.len() <= MAX_RANDOM_NAMES {
        return vec![FALLBACK_NAME.to_owned()];
    }
    let mut out = Vec::with_capacity(MAX_RANDOM_NAMES);
    for _ in 0..MAX_RANDOM_NAMES {
        out.push(pool[rng.gen_index(pool.len())].clone());
    }
    out
}

/// `HeartbeatConnectResp_3{ret=0, serverTime}`。
#[must_use]
pub fn heartbeat_resp(now: i64) -> HeartbeatConnectResp3 {
    HeartbeatConnectResp3 {
        ret: 0,
        server_time: Some(now),
    }
}

// ─────────────────────── 11 测试消息 ───────────────────────
//
// 对应 `EngineMessageHandler.testSengReq`（行 74-96）。原版发**三个包**：
// 1. `TestSengResp_11`，含 5 条 `MsgTestData`：
//    `type = 0..4`，`data` = 固定的 `LoginReq_4{serverId=99, openId="1",
//    token="2", sign="3"}` 序列化字节
// 2. `HeartbeatConnectResp_3{ret=0, serverTime=now}`
// 3. 再发一次同样的 `HeartbeatConnectResp_3`
//
// 这是厂家留的联调接口，数据全硬编码。照抄。

/// 11 号消息里 `MsgTestData.data` 的固定内容：
/// `LoginReq_4{serverId=99, openId="1", token="2", sign="3"}` 的 protobuf 编码。
///
/// 字段号取自生成代码（`LoginReq4`）：`openId=1`、`serverId=2`、`token=3`、`sign=4`。
/// 这里直接用 prost 编码而不是手拼字节 —— 手拼过一次把 `sign` 当成 tag 42
/// （实际是 4），靠单测才发现。
#[must_use]
pub fn test_data_payload() -> Vec<u8> {
    use prost::Message as _;
    let req = ddj_pb::gen::engine_message::LoginReq4 {
        open_id: Some("1".to_owned()),
        server_id: Some(99),
        token: Some("2".to_owned()),
        sign: Some("3".to_owned()),
        ..Default::default()
    };
    req.encode_to_vec()
}

/// 构造 `TestSengResp_11` 的 5 条测试数据（`type = 0..4`）。
#[must_use]
pub fn test_data_entries() -> Vec<(i32, Vec<u8>)> {
    let payload = test_data_payload();
    (0..5).map(|i| (i, payload.clone())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ddj_core::DeterministicRng;

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn compose_follows_language_order() {
        assert_eq!(NameLanguage::ZhCn.compose("张", "三"), "张三");
        assert_eq!(NameLanguage::En.compose("Zhang", "San"), "San Zhang");
        assert_eq!(NameLanguage::ViVn.compose("Tran", "An"), "Tran An");
    }

    #[test]
    fn parse_known_languages() {
        assert_eq!(NameLanguage::parse("zh_CN"), Some(NameLanguage::ZhCn));
        assert_eq!(NameLanguage::parse("en"), Some(NameLanguage::En));
        assert_eq!(NameLanguage::parse("vi_VN"), Some(NameLanguage::ViVn));
    }

    #[test]
    fn parse_unknown_returns_none() {
        // 原版此时 NPE → 不回包；这里显式返回 None 由接线层处理
        assert_eq!(NameLanguage::parse("ja_JP"), None);
        assert_eq!(NameLanguage::parse(""), None);
    }

    #[test]
    fn pool_is_cartesian_product() {
        let pool = build_name_pool(
            &v(&["张", "李"]),
            &v(&["三", "四"]),
            &[],
            NameLanguage::ZhCn,
        );
        assert_eq!(pool.len(), 4);
        assert!(pool.contains(&"张三".to_owned()));
        assert!(pool.contains(&"李四".to_owned()));
    }

    #[test]
    fn pool_excludes_taken_names() {
        let pool = build_name_pool(
            &v(&["张"]),
            &v(&["三", "四"]),
            &v(&["张三"]),
            NameLanguage::ZhCn,
        );
        assert_eq!(pool, v(&["张四"]));
    }

    #[test]
    fn empty_surname_breaks_outer_loop() {
        // 原版是 break 不是 continue：空姓氏之后的全部丢弃
        let pool = build_name_pool(
            &v(&["张", "", "李"]),
            &v(&["三"]),
            &[],
            NameLanguage::ZhCn,
        );
        assert_eq!(pool, v(&["张三"]), "空姓氏之后的名字都不应出现");
    }

    #[test]
    fn empty_name_breaks_inner_loop_only() {
        let pool = build_name_pool(
            &v(&["张", "李"]),
            &v(&["三", "", "四"]),
            &[],
            NameLanguage::ZhCn,
        );
        assert_eq!(pool, v(&["张三", "李三"]), "空名字之后的同姓名字丢弃");
    }

    #[test]
    fn random_names_returns_fallback_when_pool_small() {
        // 池子 ≤ 10 → ["玩家"]，不是「返回全部」
        let pool = v(&["张三", "李四", "王五"]);
        assert_eq!(random_names(&pool, &DeterministicRng::new(1)), v(&["玩家"]));
    }

    #[test]
    fn random_names_returns_fallback_at_exactly_ten() {
        // 原版 `if (size > 10)`，正好 10 个也走兜底
        let pool: Vec<String> = (0..10).map(|i| format!("名{i}")).collect();
        assert_eq!(random_names(&pool, &DeterministicRng::new(1)), v(&["玩家"]));
    }

    #[test]
    fn random_names_picks_ten_when_pool_large() {
        let pool: Vec<String> = (0..100).map(|i| format!("名{i}")).collect();
        let got = random_names(&pool, &DeterministicRng::new(1));
        assert_eq!(got.len(), MAX_RANDOM_NAMES);
    }

    #[test]
    fn random_names_allows_duplicates() {
        // 原版不去重：池子 11 个抽 10 次，理论上可能出现重复
        let pool: Vec<String> = (0..11).map(|i| format!("名{i}")).collect();
        let got = random_names(&pool, &DeterministicRng::new(7));
        assert_eq!(got.len(), 10);
        // 只验证长度；是否重复取决于 rng，不做断言
    }

    #[test]
    fn random_names_is_deterministic_for_same_seed() {
        let pool: Vec<String> = (0..50).map(|i| format!("名{i}")).collect();
        let a = random_names(&pool, &DeterministicRng::new(42));
        let b = random_names(&pool, &DeterministicRng::new(42));
        assert_eq!(a, b);
    }

    #[test]
    fn test_data_payload_roundtrips_as_login_req() {
        use prost::Message as _;
        // 关键断言：手拼/编码出来的字节必须能被真实 LoginReq_4 解回来，
        // 且四个字段值与原版硬编码一致（serverId=99, openId="1", token="2", sign="3"）
        let bytes = test_data_payload();
        let back = ddj_pb::gen::engine_message::LoginReq4::decode(&bytes[..]).expect("应能解码");
        assert_eq!(back.server_id, Some(99));
        assert_eq!(back.open_id.as_deref(), Some("1"));
        assert_eq!(back.token.as_deref(), Some("2"));
        assert_eq!(back.sign.as_deref(), Some("3"));
    }

    #[test]
    fn test_data_has_five_entries_with_types_zero_to_four() {
        let entries = test_data_entries();
        assert_eq!(entries.len(), 5, "原版 for (i = 0; i < 5; i++)");
        let types: Vec<i32> = entries.iter().map(|(t, _)| *t).collect();
        assert_eq!(types, vec![0, 1, 2, 3, 4], "type 依次是 0..4");
        // 5 条的 data 完全相同
        for (_, d) in &entries {
            assert_eq!(d, &test_data_payload());
        }
    }

    #[test]
    fn heartbeat_carries_server_time() {
        let r = heartbeat_resp(1_700_000_000_000);
        assert_eq!(r.ret, 0);
        assert_eq!(r.server_time, Some(1_700_000_000_000));
    }
}
