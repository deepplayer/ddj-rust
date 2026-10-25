# 移植审计台账

对照源：`data_d/<service>/sources/...`（从 `data/` 构建产物反编译，2026-10-25 生成）。
规则：**每条分支都要有「原版位置 → Rust 实现 → 单测」三件套**；发现偏离就改 Rust，
不改原版语义（原版缺陷照样复刻，并注明「复刻缺陷」）。

状态图例：✅ 已核对一致　🔧 已修偏离　⚠ 已知差异（架构性/有意的）　⬜ 待核对

---

## A1 `crates/app/game-handler/src/item.rs`

原版：`data_d/game_7010544/sources/bf/game/server/module/item/ItemSet.java`
（`ItemMessageHandler.java` 只做转发，5 个方法：11002/11003/11005/11006/11008）

| 分支 | 原版位置 | 原版行为 | Rust | 用例 | 状态 |
|---|---|---|---|---|---|
| 11002-1 | `ItemSet.java:265` | `item == null \|\| num <= 0` → `HandlerException(BUSY)` | `use_item` 首检 | `no_item` / `zero_num` | ✅ |
| 11002-2 | `:269-271` | 模板 `getItemTemplate` 后直接 `num > getUseLimit()`，模板 null 会 NPE → BUSY | `item_template` 为 None 直接回 BUSY | `missing_template` | ✅ |
| 11002-3 | `:272-276` | `num > useLimit` → BUSY | 同 | `over_use_limit` / `at_use_limit_is_ok` | ✅ |
| 11002-4 | `:277-280` | `type ∈ {EFFECTIVE(13), CHESS_SCORE(14)}` → BUSY | 同（`TypeResources` 源码已核：13/14） | `effective_type` / `chess_score_type` | ✅ |
| 11002-5 | `:281` | `removeNum = item.getUseRemoveNum(player, num, param)` | `lookups.use_remove_num` | — | ✅ |
| 11002-6 | `:282-286` | `item.getNum() < removeNum` → 回 `ITEM_ERROR_2(1102)` 并 unlock | 回 `Fail(Resp(ItemError2))` | `insufficient_uses_remove_num_not_num` | ✅ |
| 11002-7 | `:288-289` | `getGiftDB` 纯查表可返 null；`checkForUsing(null)` 首行 `getUseLimit()` NPE → handler catch → ret=2 | **已修**：gift 为 None → `Fail(Handler(ServerBusy))` | `missing_gift_row_is_busy` | 🔧 |
| 11002-8 | `:289-293` | `checkForUsing != null` → 原样回该码 | 同 | `gift_block_returns_its_code` | ✅ |
| 11002-9 | `:294-296` | 代价不足 → `HandlerException(USE_COST_RESOURCES_NO=26)`，且乘数是 **removeNum** | `cost * remove_num` | `cost_scales_with_remove_num` / `insufficient_cost_is_masked_to_busy` | ✅ |
| 11002-10 | `:298-300` | `item.use()` 抛异常 → log 后**当作成功** | `unwrap_or_else` 降级 | `use_exception_treated_as_success` | ✅ |
| 11002-11 | `:301-306` | `use()` 返回 `LanguageUtils != null` → 回该码 | 同 | `use_error_code_is_returned_verbatim` | ✅ |
| 11002-12 | `:307-310` | `removeNum(player, removeNum, false, TYPE_502, ...)` | 交给调用方 | — | ✅ |
| 11002-13 | `:308-310` | 有代价时 `revmoeResources(TYPE_507)` | 交给调用方 | — | ✅ |
| 11002-14 | `:311-318` | `sendItemUpdate`；`ret=0` + 可选 `msgUseResult` | `has_msg_use_result` | `msg_use_result_flag_passes_through` | ✅ |
| 11003 | `:itemCompound` | 11 步：`compoundNum<=0`→29；模板 null→29；type 13/14→29；无 `useItemId` 消耗 `ci[1]*compoundNum`；有则沿 `ci[0]` 链连乘；不足→1101；`addItem` 后 ret=0；**原版无环检测** | `compound()` + `MAX_COMPOUND_DEPTH=64` 护栏 | `compound_tests`（14） | ⚠ 环检测护栏（原版死循环） |
| 11005 | `:itemEffectiveUse` | 与 11002 六处差异：容器 `allEffectiveItem`、首检 `!containsKey`→RESOURCES_NO(6)、`now>=deadline`→1111、`use()` 异常不捕获、`removeNum` 第 4 参 **true** | `effective_use()` | `effective_use_tests`（12） | ✅ |
| 11005-gift | 同上 `:26-28` | 同样不判空 `checkForUsing(null)` → NPE → ret=2 | **已修**：gift None → BUSY | 覆盖于 11005 用例 | 🔧 |
| 11006 | `:itemEffectiveOneKeyUse` | 过期条目 `iterator.remove()`；deadline 升序；`removeNum < getNum()` 才 break（方向与 11005 相反）；`use()` 失败只 break；循环外只校验单份 cost；`useCostJson==null` 原版 NPE→ret=2 | `effective_one_key_use()` | `one_key_tests`（12） | ✅ |
| 11008 | `:confirmOverdue` | `now >= deadline` 才收（相等算过期）+ `setSendClient(true)`；`ret` 恒 0 | `confirm_overdue()` | `compound_tests` | ✅ |
| 推送 | `ItemServerModule.sendItemUpdate/sendItemEffectiveUpdate` | 列表为空**不发包** | `push.rs` 返回 `Option` | `push`（5） | ✅ |

---

## A2 `crates/app/game-handler/src/player.rs`

（待填：创角 17 分支 / 1002 改名 11 段 / 1003 升级 / 1100 存档 / 1008 / 1020）

## A3 `crates/app/game-handler/src/sign.rs`

| 分支 | 原版位置 | 原版行为 | Rust | 用例 | 状态 |
|---|---|---|---|---|---|
| refresh | `Sign.java:refresh` | 只清 `signedDay` 与 `boxIds`，**保留 `signedTime`** | **已修**：`SignState { signed_day:0, box_ids:vec![], ..clone }` | `refresh_resets_when_cycle_full_and_day_changed` | 🔧 |
| 常量 | `DailySignBoxDBCache.maxDailySignDBID` | 20 行实测 `= 21`（Java 侧也是硬编码） | `DAILY_SIGN_CYCLE = 21` | — | ✅ |
| sign | `Sign.java:sign` | `isTodaySigned` → 1201；`addLongStat` + `OperationEvent` 后 `signedDay++` | 计数已实现，两个副作用待接线时执行 | `sign_repeat_rejected` 等 | ⚠ 副作用待接线 |
| boxReward | `Sign.java:boxReward` | `contains` → 1202；`add` | 同 | — | ✅ |
| 16003 顺序 | `SignMessageHandler.boxReward` | 先 `days > signedDay` → 1203，再 `boxReward` 查重 | `claim_box()` 同序 | — | ✅ |
| rand | `DailySignDB.rand` | 不放回抽 `randTimes` 个 | `rand_pick`，池空 break（原版抛 IndexOutOfBounds） | — | ⚠ 护栏 |

## A4 `mail.rs`（待填）
## A5 `amber.rs`（待填）
## A6 `player_misc.rs`（待填）
## A7 `login.rs` + `loginvar.rs` + `handshake.rs`（待填）
## A8 `player_state.rs` + `push.rs` + `handlers.rs`（待填）

---

## 已知架构性差异（不修，但记录）

| 差异 | 原版 | 本项目 | 理由 |
|---|---|---|---|
| 并发控制 | 每对象 `GameReadWriteLock`，`lockWrite` 包住整个方法 | 纯函数 + 上层持锁 / 分片容器 | 锁语义保留在调用方，函数可测 |
| 数值溢出 | Java `long` 乘法回绕 | `saturating_mul` | 极端数值才分叉，选择不 panic |
| 反射注册 | `@LabelMessageHandler` + 反射扫包 | `inventory` 编译期注册 | 见 ADR-0002 |
| 模板加载 | 反射按 CSV 列名赋值到 `ConfigDBCache` | 代码生成 + `prost`/serde | 见 ADR-0003 |

---

## 待修缺陷（2026-10-25 冒烟发现）

### D1 `ErrorResp_1` 信封的帧头 cmdId 用错
- **现象**：16002/16003 的错误分支回的是 `ErrorResp_1{msgId, errorCode}`，
  但帧头 cmdId 仍是**请求**的 cmdId（16003），body 里 f1=16003、f2=1203。
- **原版**：handler 抛 `HandlerException` → 框架回 `ErrorResp_1`，
  帧头 cmdId 应是 **1**（`ErrorResp_1` 的消息号），body 的 `msgId` 才是请求号。
- **影响**：客户端按帧 cmdId 派发，会拿 16003 的回包解析器去读 `msgId`，
  把 16003 当成 `ret` —— 协议层错位。
- **根因**：`FnHandler` 的返回类型只有 `Option<Bytes>`，无法表达「回包用不同 cmdId」；
  `ddj_registry::ErasedHandler::handle_raw` 同样只返回 body，组帧处统一用请求 cmdId。
- **修法**：把两个接口的返回值改成 `Reply{cmd: Option<i32>, body}`，
  `None` 表示沿用请求 cmdId；`impl From<Bytes> for Reply` 保证现有 21 个 handler 零改动。
