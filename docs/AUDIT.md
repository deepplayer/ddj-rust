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

> 现状：代码已实现并有单测（`player.rs` 1771 行），但**未逐分支登记到本表**。
> 补齐时按 A1 的列结构（分支 / 原版位置 / 原版行为 / Rust / 用例 / 状态）填。

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

> 现状：4 条消息（10001-10004）已接线，`mail.rs` 641 行 + `install_mail.rs` 182 行，有单测。
> 未登记逐分支台账。模板 `MailTemplateDB` 未接（`template` 闭包恒 `None`）。

## A5 `amber.rs`（待填）

> 现状：6 条消息（17091-17095/17097）已接线，`amber.rs` 1013 行 + `install_amber.rs`，
> 31 行真实 CSV 已接。本轮补了 17093/17094 的 VIP 门禁与 17097 的 `checkHeyTapVip`。
> 未登记逐分支台账。`17096` 推送未发（原版在异步任务里发）。

## A6 `player_misc.rs`（待填）

> 现状：4 条消息（1009/1010/1015/1024）已接线，`player_misc.rs` 588 行，有单测。
> 未登记逐分支台账。过渡期保守默认见 `install_misc.rs` 头部注释。

## A7' `engine.rs`（3 / 5 / 7 / 11）

| 分支 | 原版位置 | 原版行为 | Rust | 用例 | 状态 |
|---|---|---|---|---|---|
| 3 | `EngineMessageHandler:26-32` | 无分支，`ret=0` + `serverTime=now` | `heartbeat_resp` | `heartbeat_carries_server_time` | ✅ |
| 5-a | `PlayerNameMgr$NameData.randomNames` | 池子 `>10` 才抽，否则 `["玩家"]` | `random_names` 的 `<=` 判断 | `random_names_returns_fallback_at_exactly_ten` | ✅ |
| 5-b | 同上 | 抽样**允许重复**（不去重） | `gen_index` 不去重 | `random_names_allows_duplicates` | ✅ |
| 5-c | `NameData.init` | `en`="名 姓"，`vi_VN`="姓 名"，其余"姓+名" | `NameLanguage::compose` | `compose_follows_language_order` | ✅ |
| 5-d | `init` 外层 | 空姓氏 → **break**（不是 continue） | `build_name_pool` 用 `break` | `empty_surname_breaks_outer_loop` | ✅ |
| 5-e | `init` 内层 | 空名字 → break（该姓氏下截断） | 同上 | `empty_name_breaks_inner_loop_only` | ✅ |
| 5-f | `LanguageType.get(未知)` | 返回 null → NPE → **不回包** | 返回 `Ok(None)` + warn | `parse_unknown_returns_none` | ✅ 照抄 |
| 5-g | 真实 CSV | `surname`/`name` 列是**多语言 key**，中文在 `*Remarks` | 读 `*Remarks` | `loads_real_table_with_306000_combinations` | ✅ 306000 |
| 7-a | `QueueUpManager:41-44` | `index<0?0:index` | `queue_position` | `not_in_queue_but_others_are` | ✅ |
| 7-b | `:44` | `maxIndex<=0?1:maxIndex` | 同上 | `empty_queue_reports_one_of_one` | ✅ |
| 7-c | `:46` | `setNowIndex(index2+1)` 1-based | 同上 | `first_in_queue_is_one_based` | ✅ |
| 7-d | `:50-52` | catch → 只设 `ret=BUSY`，不设位置 | `queue_busy_resp` | `busy_resp_has_no_positions` | ✅ |
| 11-a | `EngineMessageHandler:74-96` | 5 条 `MsgTestData{type=0..4}` | `test_data_entries` | `test_data_has_five_entries_with_types_zero_to_four` | ✅ |
| 11-b | 同上 | `data` = `LoginReq_4{99,"1","2","3"}` 字节 | prost 编码 | `test_data_payload_roundtrips_as_login_req` | ✅ |
| 11-c | 同上 | 连发两次 `HeartbeatConnectResp_3` | `sess.push` 两次 | 端到端验证 3 个包 | ✅ |

## A9 `pet.rs`（7023 / 7024 / 7025）

三条**分支序互不相同**，这是最容易写错的地方：

| 分支 | 原版位置 | 原版行为 | Rust | 用例 | 状态 |
|---|---|---|---|---|---|
| 7023-1 | `PetSet.java:624` | `PetSkinDBCache.get(skinId)` 在 **try 之外**，模板缺失直接 NPE → ret=2 | `activate_skin_verdict` 首检模板 | `activate_missing_template_is_busy` | ✅ |
| 7023-2 | `:628` | `pet == null` → `TALENT_ERROR_5(405)` | 同 | `activate_missing_pet_is_talent_error_5` | ✅ |
| 7023-3 | `:631` | `petSkinDB.getPetId() != petId` → `TALENT_ERROR_6(406)` | 同 | `activate_skin_belongs_to_other_pet` | ✅ |
| 7023-4 | `:634` | `existedSkin(skinId)` → `BUSY(2)` | 同 | `activate_already_owned_is_busy` | ✅ |
| 7023-5 | `:637` | 扣 `activateCostJson` 失败 → `938` | `can_pay` 参数 | `activate_insufficient_resources` | ✅ |
| 7023-6 | `:640-644` | `addSkin`+`setSkin`+`init`+`addGrowth(407)`+`addIntimacy(406)` | `SkinEffect::Activate` | `activate_accept_carries_cost_and_gains` | ✅ |
| 7024-1 | `:679` | **先判 `pet == null`**（与 7023 相反）→ 405 | `upgrade_skin_verdict` 首检 pet | `upgrade_missing_pet_checked_before_template` | ✅ |
| 7024-2 | `:682` | `getSkin(skinId) == null` → `TALENT_ERROR_7(407)` | 同 | `upgrade_not_owned_is_talent_error_7` | ✅ |
| 7024-3 | `:687` | `upgradeCostJson == null` → `BUSY(2)` | 同 | `upgrade_no_cost_is_busy` | ✅ |
| 7024-4 | `:690` | 扣费失败 → `939` | `can_pay` | `upgrade_insufficient_resources` | ✅ |
| 7024-5 | `:693-696` | `addSkinLv`+`addGrowth(407)`+`addIntimacy(406)` | `SkinEffect::Upgrade` | `upgrade_accept_carries_cost` | ✅ |
| 7025-1 | `:709` | `pet == null` → 405 | `set_skin_verdict` | `set_missing_pet_is_talent_error_5` | ✅ |
| 7025-2 | `:713` | `skinId != 0 && !existedSkin` → 407（**0 是合法卸下**） | 同 | `set_zero_unmounts_without_ownership` | ✅ |
| 7025-3 | `:717-719` | `setSkin` + 推 `PetUpdateResp_7001{toMsgPetData(10)}` | `SkinEffect::Set` + push | 端到端 | ✅ |

### 回包差异（三条都是 `throws HandlerException`）

| 情形 | 回包 | 帧号 |
|---|---|---|
| 成功 | `MsgActivatePetSkinResp_7023` / `..._7024` / `..._7025`，`ret=0` | 请求号 |
| 失败 | **`ErrorResp_1{msgId, errorCode}`** | **1** |

7025 成功时**额外**推 `PetUpdateResp_7001`，且 `MsgPetData.updateFieldsList=[10]`
（只更新 `skinId` 字段）。实测确认：

```text
7023 → frame=1 ErrorResp_1 msgId=7023 errorCode=2    （模板未注入，原版 NPE）
7025 → frame=1 ErrorResp_1 msgId=7025 errorCode=405  （门客不存在）
```

## A10 `pet.rs` 珍兽评级（7026 / 7029）

### ⚠ 两条的错误出口不同（最重要的一处差异）

| cmd | 失败时的出口 | 帧号 |
|---|---|---|
| 7026 `setPetRate` | 抛 `HandlerException` → **`ErrorResp_1{msgId=7026, errorCode}`** | **1** |
| 7029 `MsgPetRateLvReplaceReq_7029` | **返回 `MsgPetRateLvReplaceResp_7029{ret}`** | 7029 |

实测确认（同一份 `3601`）：

```text
7026: frame=1    ErrorResp_1 msgId=7026 code=3601
7029: frame=7029 Resp ret=3601
```

### 7026 分支序

| 分支 | 原版位置 | 原版行为 | Rust | 用例 | 状态 |
|---|---|---|---|---|---|
| 1 | `:786` | `!isOpen(pid,23)` → 3600 | `set_pet_rate_verdict` | `rate_system_closed_is_3600` | ✅ |
| 2/3 | `:789-796` | `pet==null` 或 `PetDB==null` → 3601 | 同 | `rate_missing_pet_is_3601` | ✅ |
| 4 | `:797` | `rataLv >= maxLv` → 3602 | 同 | `rate_at_max_level_is_3602` | ✅ |
| 5 | `:800-801` | `toRateLv = rataLv+1`；`toRateLvNum` 数同等级 | `current_lv+1` / `same_lv_count` | — | ✅ |
| 6 | `:802-805` | 配置缺失 → 3603 | 同 | `rate_missing_config_is_3603` | ✅ |
| 7 | `:806-808` | 亲密度**或**成长不足 → 3604 | 同 | `rate_insufficient_intimacy_is_3604` / `..._growth_...` | ✅ |
| 8 | `:809-811` | `toRateLvNum >= limitNum` → 3606 | 同 | `rate_limit_reached_is_3606` | ✅ |
| 9 | `:812-815` | `setRataLv` + 推 `7001(toMsgPetData(11))` | `push_pet_field(...,11,...)` | — | ✅ |

### 7029 分支序（含一处原版错位）

| 分支 | 原版位置 | 原版行为 | Rust | 用例 | 状态 |
|---|---|---|---|---|---|
| 1 | `:834` | `!isOpen(23)` → ret=3600 | `replace_pet_rate_verdict` | `replace_system_closed_is_3600_in_resp` | ✅ |
| 2/3 | `:837-846` | 任一 pet 或 PetDB 缺失 → 3601 | 同 | `replace_missing_pet_is_3601` | ✅ |
| 4 | `:847-850` | `if (petUp.rataLv > petDown.rataLv)` **交换** | `high_lv`/`low_lv` 归一 | `replace_reverse_order_same_result` | ✅ |
| 5 | `:851-853` | `abs(down.rataLv - up.rataLv) != 1` → 3604 | 同 | `replace_level_gap_not_one_is_3604` | ✅ |
| 6 | `:854-857` | `getPetRateByLv(**petDown**.rataLv)` → 查**高等级**配置 | `rate_tpl(high_lv)` | `replace_missing_config_is_3603` | ✅ |
| 7 | `:858-860` | 但校验的是 **`petUp`**（低等级）的 intimacy/growth | 用 `high_*` 变量 | `replace_uses_high_level_side_for_condition_check` | ✅ ⚠ 原版错位照抄 |
| 8 | `:861-877` | 交换 `rataLv`，各推一条 `7001` | 同 | `replace_swaps_levels` | ✅ |

**第 6/7 步的口径错位**：查配置用高等级那方，校验数值用低等级那方。
这是原版写法（`petDown` 与 `petUp` 在交换后语义反了），照抄并单测锁住。

## A11 `counter.rs` 计数器恢复 + `pet.rs` 互动（7002 / 7030）

### `CounterRecovery.refresh`（`CounterRecovery.java:107-124`）

| 分支 | 原版行为 | Rust | 用例 | 状态 |
|---|---|---|---|---|
| 1 | `num >= maxLimit` → 直接 return，不动 `time` | `refresh` 首检 | `at_max_limit_does_nothing` | ✅ |
| 2 | `_num = (now - time) / cdTime`；`_num <= 0` → 不恢复 | 同 | `partial_cycle_does_not_recover` | ✅ |
| 3 | `num += _num * everyNum`，封顶 `maxLimit` | 同 | `recovery_clamps_at_max_limit` | ✅ |
| 4 | **`time` 只在加完仍未满时推进** | 同 | `full_after_recovery_keeps_old_time_this_is_original_bug` | ✅ ⚠ 原版缺陷 |
| 5 | 对照：走 `removeNum` 会重置 `time` | `remove_num` | `remove_then_refresh_does_not_double_count` | ✅ |

**第 4 条是原版缺陷**：加满时不推进 `time`，若 `num` 被外部改小（不走 `removeNum`），
下次 `refresh` 会重复结算整段区间 → 瞬间补满。照抄并单测锁住。

### 互动经验公式（`PetSet.interactInner`，两处完全一样）

```text
interact = (long)( a0 + (a1 * intimacy) / (a2 + (a3 * intimacy) / (intimacy + a4)) )
```

⚠ 是 **double 运算后向零截断**，不是整数除法。
单测 `interact_exp_matches_java_formula` 用 `intimacy=9` 验证：
`10 + 900/1.9 = 483.68 → 483`。

奇迹加成：`interact *= (long)(param2 / 100.0)`，`multiple = param2`。

### 7002 `interact`

| 分支 | 原版位置 | 原版行为 | Rust | 用例 | 状态 |
|---|---|---|---|---|---|
| 1 | `:295` | `allPet.isEmpty()` → 901 | `interact_verdict` | `interact_no_pet_is_901` | ✅ |
| 2 | `:298` | `!checkNum(pid,2,1)` → 905 | 同 | `interact_no_energy_is_905` | ✅ |
| 3 | `:307-315` | `while (还有精力)` 随机分配；`all==0` 只做 1 次 | `times` 计算 | `interact_single_when_all_is_zero` / `interact_all_drains_energy` | ✅ |
| 4 | `:316` | `removeNum(pid,2,counterValue)` | `counter::remove_num` | — | ✅ |
| 5 | `:330-335` | 推 `7001(toMsgPetData(4))` | `push_pet_i64_field(...,4,...)` | 端到端 | ✅ |

### 7030 `interactTimes`

| 分支 | 原版位置 | 原版行为 | Rust | 用例 | 状态 |
|---|---|---|---|---|---|
| 1 | `:194` | `allPet.isEmpty()` → 901 | `interact_times_verdict` | `times_no_pet_is_901` | ✅ |
| 2 | `:198` | `times > itemDB.getUseLimit()` → BUSY | 同 | `times_over_use_limit_is_busy` | ✅ |
| 3 | `:202` | `item == null \|\| num < times` → BUSY | 同 | `times_insufficient_item_is_busy` | ✅ |
| 4 | `:214` | `for (i < times * counterRecovery.getMaxLimit())` | `loops = times × max_limit` | `times_loops_is_times_times_max_limit` | ✅ ⚠ 易误读 |
| 5 | `:237-245` | 推 `7001(toMsgPetData(4))` | 同 | — | ✅ |

**第 4 条最易误读**：循环次数是 `times × 计数器上限`，不是 `times`。
一次「十连」在 `maxLimit=100` 时实际互动 1000 次。

### 端到端实测

```text
7002 all=1  → ErrorResp_1{msgId=7002, code=901}   新账号无宠物
7030 num=10 → ErrorResp_1{msgId=7030, code=901}   同上
```

## A12 `pet.rs` 升星 7003 / 技能升阶 7006

两条都**抛 `HandlerException`** → 失败走 `ErrorResp_1`。

### 7003 `PetSet.star`（`:342-371`）

| 分支 | 原版位置 | 原版行为 | Rust | 用例 | 状态 |
|---|---|---|---|---|---|
| 1 | `:344` | `pet == null` → 901 | `pet_star_verdict` | `star_missing_pet_is_901` | ✅ |
| 2 | `:350-353` | `getPetStarDB(star + 1) == null` → 906 已满星 | 同 | `star_max_is_906` | ✅ |
| 3 | `:354-356` | `intimacy < starDB.needIntimacy` → 907 | 同 | `star_insufficient_intimacy_is_907` | ✅ |
| 4 | `:357-359` | `growth < starDB.needGrowth` → 908 | 同 | `star_insufficient_growth_is_908` | ✅ |
| 5 | `:360-363` | `starUp()` + `notifyTalentRefresh()` | `PetStarOutcome::Accept` | `star_accept_increments` | ✅ |

⚠ **第 2 步查 `star + 1`，第 3/4 步用 `star`** —— 用当前星级的门槛升到下一星级。
单测 `star_uses_current_tier_threshold_not_next` 给下一级设极高门槛，仍应通过。

### 7006 `PetSet.skillClass`（`:373-417`）

| 分支 | 原版位置 | 原版行为 | Rust | 用例 | 状态 |
|---|---|---|---|---|---|
| 1 | `:374-377` | `pet == null` → 901 | `pet_skill_class_verdict` | `class_missing_pet_is_901` | ✅ |
| 2 | `:378-381` | `skillId.indexOf < 0` → `PARAM_ERROR(29)` | 同 | `class_skill_not_belonging_is_param_error` | ✅ |
| 3 | `:384-387` | `skillLv == 0` → 909 未解锁 | 同 | `class_skill_locked_is_909` | ✅ |
| 4 | `:388-391` | `PetSkillDB == null \|\| stepAdd == 0` → 909 | 同 | `class_step_add_zero_is_909` | ✅ |
| 5 | `:393-397` | `getSkillClassUpgradeDB(cls + 1) == null` → 913 已满阶 | 同 | `class_max_is_913` | ✅ |
| 6 | `:398-401` | `getAllPet().size() < upgradeDB.getPetNum()` → 940 | 同 | `class_insufficient_pet_count_is_940` | ✅ |
| 7 | `:402-405` | 扣 `getCost(skillOffset)` 失败 → 911 | `can_pay` | `class_insufficient_resources_is_911` | ✅ |
| 8 | `:406-411` | `setSkillClass` + `updateSkinSkillLv` + 推 `7001(7,6)` | `push_pet_skill_class` | `class_accept_carries_offset_and_cost` | ✅ |

⚠ **第 6 步的「宠物数量」是玩家拥有的宠物总数**（`getAllPet().size()`），
不是该技能对应的宠物数量。这是原版写法。

### 端到端实测

```text
7003 petId=100              → ErrorResp_1{msgId=7003, code=901}
7006 petId=100 skillId=1001 → ErrorResp_1{msgId=7006, code=901}
```

## A13 `pet.rs` 技能升级 7005

对应 `PetSet.skillLevel`（`PetSet.java:419-497`）。**两条分支扣的东西不同**，这是核心。

### 公共前置

| 分支 | 原版位置 | 原版行为 | Rust | 用例 | 状态 |
|---|---|---|---|---|---|
| 1 | `:420-423` | `pet == null` → 901 | `pet_skill_level_verdict` | `level_missing_pet_is_901` | ✅ |
| 2 | `:424-427` | `PetSkillDB == null \|\| petId != petId` → 29 | 同 | `level_missing_skill_template_is_param_error` / `level_skill_not_belonging_is_param_error` | ✅ |

### 分支 A：皮肤技能（`isPetSkinSkill` 为真）

| 分支 | 原版位置 | 原版行为 | Rust | 用例 | 状态 |
|---|---|---|---|---|---|
| 3 | `:431-433` | `skinSkillLv` 不含 → 909 | 同 | `skin_skill_not_present_is_909` | ✅ |
| 4 | `:434-436` | `!isOpenPetSkinSkill` → 909 | 同 | `skin_skill_not_open_is_909` | ✅ |
| 5 | `:437-443` | `nextUpgradeDB(lv+1) == null` → 912 | 同 | `skin_skill_max_lv_is_912` | ✅ |
| 6 | `:444-446` | `goodId == 0` → 909 | 同 | `skin_skill_good_id_zero_is_909` | ✅ |
| 7 | `:447-450` | 扣 `getSkinCost()[offset]` 失败 → **`ResourcesNo(6)`** | `can_pay_good` | `skin_skill_insufficient_good_is_resources_no` | ✅ ⚠ 不是 911 |
| 8 | `:438` | `offset = skillNum - 1` | `skill_num - 1` | `skin_skill_uses_skill_num_minus_one_as_offset` | ✅ |
| 9 | `:451` | `setSkinSkillLv(skillId, lv+1)` | `PetSkillLevelEffect::Skin` | — | ✅ |

### 分支 B：普通技能

| 分支 | 原版位置 | 原版行为 | Rust | 用例 | 状态 |
|---|---|---|---|---|---|
| 3 | `:454-458` | `indexOf` / `lv == 0` → 909 | 同 | `normal_skill_locked_is_909` / `normal_skill_no_offset_is_909` | ✅ |
| 4 | `:459-463` | `nextUpgradeDB(lv+1) == null` → 912 | 同 | `normal_skill_max_lv_is_912` | ✅ |
| 5 | `:464-467` | `getAllPet().size() < petNum` → 940 | 同 | `normal_skill_insufficient_pet_count_is_940` | ✅ |
| 6 | `:468-471` | `unlockSkillType > 0 && !isFitUnlockCondition` → 946 | 同 | `normal_skill_unlock_condition_is_946` | ✅ |
| 7 | `:472-476` | `interaction < cost` → **910** | 同 | `normal_skill_insufficient_interaction_is_910` | ✅ |
| 8 | `:477-479` | `interaction -= cost`；`setSkillLv(lv+1)` | `PetSkillLevelEffect::Normal` | `normal_skill_costs_interaction_not_item` | ✅ |

### ⚠ 两条分支的资源类型不同

| 分支 | 扣什么 | 错误码 |
|---|---|---|
| 皮肤技能 | `goodId` **道具** | `ResourcesNo(6)` |
| 普通技能 | `Pet.interaction` **互动经验**（7002/7030 攒的） | `910` |

两条各有一个反向单测，确保不会交叉检查：
- `skin_branch_ignores_interaction` —— 互动值为 0 时皮肤分支仍通过
- `normal_branch_ignores_good_id` —— 道具不够时普通分支仍通过

### 端到端实测

```text
7005 petId=100 skillId=1001 → ErrorResp_1{msgId=7005, code=901}
7023 对照                    → ErrorResp_1{msgId=7023, code=2}   模板未注入
```

## A7 `login.rs` + `loginvar.rs` + `handshake.rs`（待填）

> 现状：登录 4 / 创角 6 / 握手 16-18 已接线，走真实 PG。`login.rs` 630 行。
> 已复核的分支：区服不符→2、维护→1、未开服→23、版本→99、人数满→4。
> **未实现**：`WorkTaskPlayerSignIn` 的游客/token/白名单三段判定（原版 L「非游客+无 token+
> 非白名单 → ret=1」），以及 `PlayerNameMgr.randomNames` 的 10 个候选名。

## A8 `player_state.rs` + `push.rs` + `handlers.rs`（待填）

> 现状：1100/1103/1111/1112 已接线，`push.rs` 的 11004/11007 构造器已就绪且本轮已
> 在 `install_item.rs` 里真正用上（此前从未被调用）。
> 未登记逐分支台账。

---

## 已知架构性差异（不修，但记录）

| 差异 | 原版 | 本项目 | 理由 |
|---|---|---|---|
| 并发控制 | 每对象 `GameReadWriteLock`，`lockWrite` 包住整个方法 | 纯函数 + 上层持锁 / 分片容器 | 锁语义保留在调用方，函数可测 |
| 数值溢出 | Java `long` 乘法回绕 | `saturating_mul` | **⚠ 待裁决**，见下 §待裁决偏离 |
| 反射注册 | `@LabelMessageHandler` + 反射扫包 | `inventory` 编译期注册 | 见 ADR-0002 |
| 模板加载 | 反射按 CSV 列名赋值到 `ConfigDBCache` | 代码生成 + `prost`/serde | 见 ADR-0003 |

---

## 待裁决偏离（照抄策略 + 可定位记录）

**决策（2026-10-27）**：三处全部**保留护栏、照抄原版语义**，但每次真的触发时
必须留下可定位的记录。等真实客户端连上来跑一遍，用记录判断是原版缺陷被踩到、
还是移植写错了，再决定是否改成「完全照抄原版异常」。

### 记录机制

代码：`crates/app/game-handler/src/divergence.rs`

| 项 | 内容 |
|---|---|
| 触发时 | 计数器 +1，打一条 `WARN`，`target=ddj::divergence` |
| 日志字段 | `code` / `original`（原版会怎样）/ `ours`（本工程怎样）/ `occurrences` / `detail`（本次上下文） |
| 查看计数 | `ddj status <service>` 单服务查询时打印；`Stats.divergences` 也随控制通道返回 |
| 全 0 时 | 打印「全部未触发（正常）」 |
| 非 0 时 | 逐条列出 `P1 ×N 原版：...`，并提示去日志搜 `target=ddj::divergence` |

日志样例：

```text
WARN target=ddj::divergence code=P1 occurrences=1
  original="ItemSet.itemCompound 的 while(true) 无环检测，会永久循环卡住玩家线程"
  ours="限制 64 层，超出回 PARAM_ERROR(29)"
  detail="compoundId=3000 useItemId=9999 compoundNum=1 depth=64 chain=[3000, 3001, 3000, 3001, ...]"
```

`detail` 的设计目标是**够定位**：

| 分叉 | `detail` 里带什么 | 怎么用 |
|---|---|---|
| P1 | `compoundId` / `useItemId` / `compoundNum` / 完整 `chain` 路径 | 拿 chain 去 `D-道具表-Item.csv` 查这几行的 `compoundItem` 列，看哪几行成环 |
| P2 | `signed_day` / `rand_times` / `pool_len` / `drawn` | 拿 `signed_day` 去 `M-每日签到表-DailySign.csv` 查该行奖励条数是否少于 `rand_times` |
| P3 | 两个操作数（`a` 与 `b`） | 判断是表数据异常还是客户端伪造了超大数量 |

### 三处明细

| # | 位置 | 原版行为 | 本工程 | 触发条件 | 状态 |
|---|---|---|---|---|---|
| P1 | `item.rs` `MAX_COMPOUND_DEPTH = 64` | `ItemSet.itemCompound` 链式 `while(true)` 无环检测，成环时**永久循环**，玩家线程卡死 | 超 64 层记录 P1 + 回 `PARAM_ERROR(29)` | `compoundItem` 列成环。真表 37 行非 0，当前无环 | ✅ 已记录 |
| P2 | `sign.rs` `rand_pick` | `DailySignDB.rand` 池空抛 `IndexOutOfBoundsException` → handler 外层 catch → `ret=2` | 池空记录 P2 + 提前 `break`，返回已抽到的部分 | `randTimes > 该天奖励条数`。`randTimes` 硬编码 3，若某天只有 1-2 条奖励就触发 | ✅ 已记录 |
| P3 | `item.rs` `mul_or_record` / `add_or_record` | Java `long` 乘法溢出**回绕成负数**，随后 `num < removeNum` / `checkItem` 结论不同 | 溢出记录 P3 + 饱和到 `i64` 边界 | 数量级接近 `i64::MAX`（正常数据不可能） | ✅ 已记录 |

### 触发路径与单测

| 分叉 | 单测 | 断言 |
|---|---|---|
| P1 | `item::compound_tests::cycle_increments_p1_divergence_counter` | 成环后计数 +1 |
| P2 | `sign::tests::rand_pick_short_pool_records_p2` | 池空后计数 +1 |
| P2 反例 | `sign::tests::rand_pick_exact_pool_does_not_record` | 刚好抽完不计数 |
| P3 | `item::compound_tests::mul_overflow_records_p3` | 溢出后计数 +1 |
| P3 反例 | `item::compound_tests::mul_without_overflow_does_not_record` | 正常乘法不计数 |
| P3 | `item::compound_tests::add_overflow_records_p3` | 加法溢出 +1 |

### 真实客户端连上后的处置流程

1. `ddj status game` 看三个计数器是否非 0；
2. 非 0 → 日志搜 `target=ddj::divergence`，取 `detail`；
3. 按上表「怎么用」定位到具体 CSV 行；
4. 若确认是**表数据问题** → 修表（不动代码）；
5. 若确认是**原版缺陷**（例如某天签到奖励本来就只有 2 条，原版客户端也会遇到 ret=2）
   → 保持现状，把该 CSV 行写进本文档；
6. 若确认是**移植写错** → 改代码，并从 `divergence.rs` 移除该分叉。

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

## A1' 新发现：模板默认值与原版偏差（RawTable 接真数据时暴露）

| 项 | 原版（真 CSV 实测） | 当前 Rust 过渡值 | 影响 |
|---|---|---|---|
| `ItemDB.useLimit` | 2674 行道具中仅 **1178 行**填了值，其余为空 | `ASSUMED_USE_LIMIT = 999` | 空值 ⇒ `getUseLimit()==0` ⇒ 任何 `num>=1` 都触发 `num > useLimit` → throw BUSY。即**这类道具原版不可使用**，而当前实现会放行 |
| `GiftDB.useCost` 列名 | CSV 列名是 **`useCost`**（Java 字段 `useCostJson`） | 未接 | 接表时别按 `useCostJson` 找列 |
| `GiftDB` 有消耗的行 | 1066 行中仅 **10 行**填了 `useCost` | 未接 | 绝大多数礼包无消耗，`cost` 分支很少触发 |

结论：`ASSUMED_USE_LIMIT = 999` 是**已知偏差**，接 `RawTable` + `ItemLookup` 时改为按 CSV 取值，空值即 0。
