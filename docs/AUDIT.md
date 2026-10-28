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

## A14 `second_psw.rs`（17351-17355）

对应 `SecondPsw.java`（210 行）。handler 侧 5 条全部只做转发。

### ⚠ 17352 是唯一「失败也走 Resp」的一条

| cmd | 失败出口 | 帧号 |
|---|---|---|
| 17351 / 17353 / 17354 | `throws HandlerException` → `ErrorResp_1` | **1** |
| **17352** | 自己 catch 住 → `MsgVerifyPswResp_17352{ret=code}` | **17352** |

实测确认：

```text
17351 短密码 → frame=1     ErrorResp_1 msgId=17351 code=9501
17352 错密码 → frame=17352 Resp ret=9505
```

### ⚠ setPsw 的外层 catch 吞掉内层错误

```java
try {
    Integer.parseInt(psw);
    if (isApplyResetState()) throw COMMON_SERVER_STAGE_ERROR;        // 30
    if (psw != null && !isVerifySuccess) throw SECOND_PSW_NO_VERIFY; // 9503
    ...
} catch (Throwable th) {
    throw new HandlerException(SECOND_PSW_FORMAT_ERROR);             // 9502
}
```

内层的 `30` / `9503` 全被外层吞成 `9502`（「密码只能是数字」）。
所以「正在重置中」与「未验证就改密码」客户端看到的是同一个错误。
两个单测分别锁住：
- `set_psw_modify_requires_verify_but_is_masked_to_9502`
- `set_psw_during_reset_is_masked_to_9502`

### 各条分支

| cmd | 分支 | 原版位置 | Rust | 用例 | 状态 |
|---|---|---|---|---|---|
| 17351 | `psw == null` → 29 | `:119-121` | `set_psw_verdict` | `set_psw_null_is_param_error` | ✅ |
| 17351 | 长度 ≠ limit → 9501 | `:122-124` | 同 | `set_psw_wrong_length_is_9501` | ✅ |
| 17351 | 非数字 → 9502 | `:126` | 同 | `set_psw_non_digit_is_9502` | ✅ |
| 17351 | 首次设置 → `enabled=true` | `:134-137` | `first_set` | `set_psw_first_time_accepts` | ✅ |
| 17352 | 未设密码 → 30（走 Resp） | `:148-150` | `verify_psw_verdict` | `verify_not_set_is_30_via_resp` | ✅ |
| 17352 | 错误次数 ≥ attempt → 9504 | `:151-153` | 同 | `verify_too_many_errors_is_9504` | ✅ |
| 17352 | 密码不符 → 9505 + 计数 +1 | `:157-162` | 同 | `verify_wrong_psw_is_9505` | ✅ |
| 17352 | 通过 → 清空计数 | `:163-167` | `VerifyOk` | `verify_correct_accepts` | ✅ |
| 17353 | 未验证 → 9503 | `:174-176` | `set_enabled_verdict` | `enabled_requires_verify` | ✅ |
| 17354 | 已在重置 → 30 | `:183-185` | `apply_reset_verdict` | `apply_reset_twice_is_30` | ✅ |
| 17354 | `resetTime * 3600000` | `:186` | 同 | `apply_reset_sets_end_time_in_hours` | ✅ |
| 17354 | 取消需在重置中 | `:193-195` | `cancel_reset_verdict` | `cancel_reset_requires_pending` | ✅ |
| 17355 | `toMsg()` 四字段 | `:202-210` | `to_snapshot` | `snapshot_reflects_state` | ✅ |

### 全链路实测

```text
17355 → ret=0 初始状态
17351 短密码 → ErrorResp_1{code=9501}
17351 123456 → ret=0 haveSetPsw=true
17352 错密码 → ret=9505（帧号 17352）
17352 对密码 → ret=0
17353 enabled=0 → ret=0 enabled=false
17354 type=1 → ret=0 type=1
17355 → ret=0 更新后状态
```

## A15 `anticheat.rs` + `install_anticheat.rs`（17151 / 17152 / 17153）

对应 `SysActivityMessageHandler.java:119-162` + `AntiCheatModel.java`（75 行）
+ `bf/template/config/AntiCheatRewardJson.java`。

### ⚠ 三条都不走 `ErrorResp_1`

`antiCheatReward` / `antiCheatRead` / `antiCheat` 的签名**没有 `throws HandlerException`**，
业务失败全在 Resp 里：

| cmd | 失败出口 | 帧号 |
|---|---|---|
| 17151 | `AntiCheatRewardResp_17151{ret=code}` | **17151** |
| 17152 | 成功 `AntiCheatReadResp_17152{ret=0}`；失败**不发包** | 17152 |
| 17153 | 恒成功 `AntiCheatResp_17153{info}` | 17153 |

### ⚠ 两处「不发包」

```java
// 17152：越界直接 return（客户端超时）
if (!ConfigDBCache.AntiCheatReward.canReward(req.getIndex())) return;

// 17151：外层 catch 只记日志，同样不发包
try { ... } catch (Exception e) { LoggerService.getLogicLog().error(...); }
```

### 17151 的分支序（不能调换）

```text
!canReward(index)              → ret = 29   (COMMON_SERVER_PARAM_ERROR)
!read.contains(index)          → ret = 6451 (ANTICHEAT_READ)
!addReward(index)（已领过）    → ret = 1501 (ACTIVITY_RANK_LIST_ERROR_REPEATED)
否则                            → 发奖 + ret = 0
```

「越界」优先于「未阅读」，「未阅读」优先于「已领」—— 三条单测各锁一条：
`reward_branch_order_range_before_read` / `reward_branch_order_read_before_duplicate` /
`reward_already_got_is_1501`。

### 配置来源（`B-变量表-Config.csv:416`，id=568）

```text
568,AntiCheatReward,
  1=25;231=1;211=1;201=1|1=25;231=1;211=2;201=2|1=25;231=1;45=1;202=1|1=25;231=1;46=1;202=1
```

`AntiCheatRewardJson.parse`：`param.split("\\|")` → 每段 `ResourcesJson.toResourcesJsons`
（按 `;` 再按 `=`）。实测 **4 组**，所以 `index 0..3` 合法、`4` 越界。

### 分支明细

| cmd | 分支 | 原版位置 | Rust | 用例 | 状态 |
|---|---|---|---|---|---|
| 17151 | `canReward` 假（含负数） | `:123` | `can_reward` | `can_reward_checks_index_range` | ✅ |
| 17151 | 越界 → 29 | `:123-124` | `reward_verdict` | `reward_out_of_range_is_param_error` | ✅ |
| 17151 | 未阅读 → 6451 | `:129-130` | 同 | `reward_not_read_is_6451` | ✅ |
| 17151 | 已领 → 1501 | `:131` | 同 | `reward_already_got_is_1501` | ✅ |
| 17151 | 通过 → 发奖 | `:133-136` | 同 | `reward_accept_carries_group` | ✅ |
| 17152 | 越界 → **静默** | `:144-146` | `read_verdict` | `read_out_of_range_is_silent` | ✅ |
| 17152 | 范围内 → `ret=0` | `:149-151` | 同 | `read_in_range_accepts` | ✅ |
| 17153 | `toMsg()` 两列表 | `:154-161` | `to_info` | `to_info_mirrors_both_lists` | ✅ |

### 端到端实测（2026-10-27）

```text
17151 index=4 越界     → 帧号 17151，ret=29
17151 index=0 未阅读   → 帧号 17151，ret=6451
17152 index=4 越界     → **无回包**（客户端超时）
17152 index=0          → 帧号 17152，ret=0
17151 index=0 已阅读   → 帧号 17151，ret=0
17151 index=0 再领     → 帧号 17151，ret=1501
17153                  → info{reward=[0], read=[0]}
```

> 探针坑：`AntiCheatInfo` 的 `repeated int32` 是 `packed = "false"`（proto2 默认不打包），
> **每个元素独立带 tag**。按 packed 解析会读到空列表。

### 配置畸形时的分叉

`ConfigDBCache.init` 的 `catch (Exception e2)` 只记日志，`AntiCheatReward` 保持 `null`；
之后 `canReward(...)` 直接 NPE → 17151/17152 都不发包。
Rust 没有 `null`，退化为「空配置」：17151 → `ret=29`、17152 → 静默。
**只在畸形配置下分叉**，正常数据完全一致。

---

## A16 `counter.rs` + `install_counter.rs`（8001 / 8002）

对应 `CounterRecoveryMessageHandler.java:17-39`、`CounterRecoverySet.java`、
`CounterRecovery.java`、`CounterRecoveryDB.java`、`VipDB.getCounterMax`。

### ⚠ 8002 的两层 containsKey 都没有 else

```java
if (this.allCounterRecovery.containsKey(templateId)) {
    Map<Long, CounterRecovery> m = this.allCounterRecovery.get(templateId);
    if (m.containsKey(param1)) { ... send ... }
    // ← 没有 else：param1 不存在 → 静默
}
// ← 没有 else：templateId 不存在 → 静默
```

客户端会超时。已实测确认。

### ⚠ 8001/8002 的 catch 也走 Resp

两条 handler 都是 `try { ... } catch (Exception e) { send(Resp{ret=BUSY=2}) }`，
所以帧号恒为 8001/8002。

### ⚠ `counterGetReq` 的 `param1` 恒为 0

```java
public void counterGetReq(Player player, long... args) {
    if (args.length == 1) templateId = (int) args[0];
    else if (args.length == 2) { templateId = (int) args[0]; param1 = args[1]; }
```

handler 只传 `req.getTemplateId()` → `args.length == 1` → `param1 = 0`。
**所以子嗣（`templateId=5`）的记录用 8002 查不到。**

### ⚠ `isLimit()` 的语义与列名相反

`CounterRecoveryDB.isLimit()` 返回 `this.isLimit == 0`。
CSV 列注释写「是否允许超出上限」，但列值 `0` 才是**受限**（到上限停表）。
实测 92 行里 `isLimit=0` 占多数，`30/32/201/202/203/204` 等是 `1`（不受限）。

### ⚠ `time == -1` 表示停表（这是上一轮的理解错误，已修正）

上一轮 `counter.rs` 的注释写「加满时 `time` 保持不变」。**实际不是**：
`_addNumWithoutLock` 在加满时把 `time` 置为 `-1`：

```java
if (counterRecoveryDB.isLimit()) {
    if (this.num >= maxLimit) {
        this.time = -1L;
        if (!cdRecovery) this.expectMaxNumTime = 0L;
    }
}
```

后果完全不同：
- 「保持不变」→ 下次 refresh 会重复结算同一段区间（瞬间补满）
- 「-1」→ 下次 refresh 首行 `time == -1` 直接 return（不恢复）

现在的实现与单测都按 `-1`：`reaching_max_sets_time_to_minus_one` /
`remove_from_full_starts_timer`（满 → 扣一点 → 从扣减时刻重新计时）。

### ⚠ 新建的计数器是**满的**

`CounterRecoverySet.dbInit`：
`new CounterRecovery(playerId, templateId, 0L, counterMax, -1L)`，
构造签名 `(playerId, templateId, param1, num, time)` ——
第 4 个参数是 `num`。所以新玩家所有计数器**直接满**、`time = -1`。
已实测：`8001` 返回 91 条，每条 `num == VipDB.getCounterMax(0, id)`。

### ⚠ `calcExpectMaxNumTime` 早于 `_num > 0` 判断

```java
if (this.num < maxLimit) {
    calcExpectMaxNumTime(maxLimit);        // ← 先写字段
    long _num = intervalTime / cdTime;
    if (_num > 0) { ... }
}
```

所以哪怕一个 CD 都没走完，`expectMaxNumTime` 也已经落库。
单测 `expect_max_num_time_is_written_when_not_full` 锁这条。

### ⚠ `type=4` 的 2 行永远不恢复

`CounterRecoverySet.refresh` 的 `switch` 只有 `case 1/2/3`，
`type=4` 落 `default` 什么都不做。实测 CSV 里 `type=4` 有 **2 行**
（`type=1` 77 行 / `type=2` 13 行 / `type=4` 2 行）。照抄，不补 case 4。

### 分支明细

| 函数 | 原版位置 | 用例 | 状态 |
|---|---|---|---|
| `cd_recovery` | `:100-149` | `stopped_counter_never_recovers` / `future_time_is_skipped` / `recovers_one_per_cd_and_advances_time` / `partial_cycle_does_not_recover` / `reaching_max_sets_time_to_minus_one` / `at_max_returns_immediately` / `unlimited_counter_caps_at_i64_max_not_max_limit` / `zero_cd_time_is_noop` | ✅ |
| `auto_recovery` | `:167-183` | `auto_recovery_fills_to_max` / `auto_recovery_at_max_is_noop` | ✅ |
| `get_cd_time` | `:151-165` | `cd_time_of_stopped_is_minus_one` / `cd_time_of_full_is_minus_one` / `cd_time_normal_is_time_plus_cd` | ✅ |
| `set_max_num` | `:185-207` | `set_max_num_fills_and_stops_when_limited` / `set_max_num_uses_timestamp_when_unlimited` | ✅ |
| `remove_num` | `:283-327` | `remove_from_full_starts_timer` / `remove_while_running_keeps_time` / `remove_insufficient_fails` / `remove_without_check_clamps_zero` / `remove_negative_is_rejected` / `remove_zero_succeeds_without_touching_state` | ✅ |
| `to_counter_data` | `:390-404` | `msg_cd_time_for_template_5_is_always_minus_one` / `msg_cd_time_for_type_1_is_deadline` / `msg_cd_time_for_type_2_is_raw_time` | ✅ |
| `refresh_set` | `CounterRecoverySet:170-195` | `refresh_set_type1_runs_cd_recovery` / `refresh_set_type2_only_on_new_day` / `refresh_set_type3_only_on_new_week` / `refresh_set_unknown_template_is_skipped` | ✅ |
| `is_same_day` / `is_same_week` | `UtilsTools:271-287` | `same_day_across_midnight_in_utc` / `same_week_rejects_beyond_eight_days` / `same_week_within_a_week` / `week_number_advances_after_seven_days` | ✅ |
| `init_missing` | `CounterRecoverySet.dbInit` | `init_skips_child_template` / `init_creates_full_and_stopped` / `init_skips_existing` | ✅ |

### 端到端实测（2026-10-27）

启动日志：

```text
计数器模板已装载  counter_rows=92 vip_levels=18
服务就绪          handlers=71
```

```text
8001 → 91 条（92 模板 - 跳过 5 号子嗣）
       2 号 num=3（VIP0 上限）cdTime=-1（满值停表）
       不含 5 号
       4 号 num=5（VIP0 上限）
8002 templateId=2      → ret=0，单条 templateId=2 num=3 cdTime=-1
8002 templateId=999999 → **无回包**（原版没有 else）
```

### 已知分叉

| 场景 | 原版 | 本工程 | 影响 |
|---|---|---|---|
| `cdTime <= 0` 且 `type=1` | 除零 `ArithmeticException` → catch → `ret=2` | 当作「不恢复」 | 只在畸形模板下分叉（实测 `type=1` 的 77 行 `cd` 全部 > 0） |
| `is_same_week` 的年初边界 | `java.util.Calendar` 的 `WEEK_OF_YEAR` | 自实现「第一个完整周一起算第 1 周」 | `type=3` 实测 **0 行**，无数据可验证 |

---

## A17 `shortcut.rs` + `install_shortcut.rs`（17301 / 17302 / 17303）

对应 `SysActivityMessageHandler.java:231-254`、`ShortcutModel.java`（126 行）、
`PlatformServerModule.isEligibleShortcut`（`:712-715`）+ `parseConfig59`（`:1315-1336`）。

### ⚠ 17301 恒成功，17302/17303 走 `ErrorResp_1`

| cmd | 方法签名 | 失败出口 | 帧号 |
|---|---|---|---|
| 17301 | `public void queryShortcutModel(...)` **无 `throws`** | 无失败分支 | 17301 |
| 17302 | `throws HandlerException` | `ErrorResp_1` | **1** |
| 17303 | `throws HandlerException` | `ErrorResp_1` | **1** |

17301 的方法体里 `shortcutModel.queryShortcutData()` 内部自己发包，
没有任何 `throw`，所以它只有成功分支。

### ⚠ 渠道判定优先于状态判定

17302 与 17303 都是**先判渠道**：

```java
boolean eligible = PlatformServerModule.isEligibleShortcut(channelId, cid);
if (!eligible) throw new HandlerException(SHORTCUT_CID_ERROR);   // 4731
// 之后才判 state
```

所以渠道不匹配时永远先看到 **4731**，看不到 30 / 4732 / 4722。
两条单测分别锁：
`install_rejects_ineligible_first`、`reward_branch_order_cid_before_not_install`。

### ⚠ 17303 的「已领过」用的是 VIVO 的码

```java
if (this.state != TaskRewardState.NOT_RECEIVE.getState()) {
    throw new HandlerException(LanguageUtils.VIVO_WELFARE_SIGN_ERROR);   // 4722
}
```

「保存桌面已领过」回的是 `VIVO_WELFARE_SIGN_ERROR(4722, "VIVO特权 - 领取奖励失败")`。
复制粘贴 bug，照抄。`RetCode::VivoWelfareSignError` 的文档注释里已标注。

### ⚠ `bf_config.configId=59` 实测为空串

`data/sql/platform.sql:562`：
```sql
INSERT INTO `bf_config` VALUES ('72', '59', '', '保存桌面：开启渠道控制=cid(多个cid用;分隔)', ...);
```

`parseConfig59("")` 里 `if (!str.isEmpty())` 直接跳过 →
`shortcutConfigMap` 为空 → `isEligibleShortcut` **恒 false** →
17302/17303 在真实数据下**恒回 4731**。

这不是移植 bug，是原版线上数据的真实状态。等运营配了渠道才有意义。

### 状态机

| `TaskRewardState` | 值 | 含义 |
|---|---|---|
| `NO_REWARD` | 0 | 未保存到桌面 |
| `NOT_RECEIVE` | 1 | 已保存，待领 |
| `RECEIVED` | 2 | 已领取 |

### 分支明细

| cmd | 分支 | 原版位置 | Rust | 用例 | 状态 |
|---|---|---|---|---|---|
| 17301 | `toMsgShortcutData` 四字段 | `:110-124` | `to_msg` | `to_msg_carries_four_fields` | ✅ |
| 17301 | 空配置 → `eligible=false` | 同上 | 同 | `to_msg_eligible_false_when_not_configured` | ✅ |
| 17302 | 渠道不符 → 4731 | `:76-79` | `install_verdict` | `install_rejects_ineligible_first` | ✅ |
| 17302 | `state != 0` → 30 | `:80-82` | 同 | `install_rejects_when_already_installed` / `..received` | ✅ |
| 17302 | 通过 → `state=1` | `:83-84` | 同 | `install_accepts_from_no_reward` | ✅ |
| 17303 | 渠道不符 → 4731 | `:100-103` | `reward_verdict` | `reward_rejects_ineligible_first` | ✅ |
| 17303 | `state == 0` → 4732 | `:104-106` | 同 | `reward_rejects_when_not_installed` | ✅ |
| 17303 | `state != 1` → 4722 | `:107-109` | 同 | `reward_rejects_when_already_received` | ✅ |
| 17303 | 通过 → `state=2` + 发奖 | `:110-116` | 同 | `reward_accepts_from_not_receive` | ✅ |
| 配置 | `parseConfig59` 解析 | `:1315-1336` | `ShortcutConfig::parse` | `parse_multi_channel` / `parse_skips_malformed_segments` | ✅ |

### 本轮打通的数据链

`channelId` / `cid` 之前完全没落库，17301-17303 无法判定。现在：

```text
CreatePlayerReq_6.channelId (字段 11) ─┐
CreatePlayerReq_6.cid      (字段 12) ─┤
                                      ↓
              NewPlayerRow.channel_id / .cid
                                      ↓
              player 表（0006_player_channel.sql）
                                      ↓
              PlayerRow → Account → LoginOutcome::Accepted
                                      ↓
              MemWorld.PlayerBox.channel_id / .cid
                                      ↓
              17301 / 17302 / 17303 的 is_eligible
```

`LoginHandler` 之前拿不到 `MemWorld`（只有 `ServerState` / `AccountLookup` / `HandshakeStore`），
本轮加 `with_world`。**后续所有按渠道判定的消息都靠这条链**，包括：
17101-17103（渠道好评）、17081（渠道卡池）、15002（游历）、
以及 `SystemOpenUtils.isOpen` 里按渠道开的功能。

### 已知分叉

| 场景 | 原版 | 本工程 | 影响 |
|---|---|---|---|
| 渠道白名单来源 | `bf_config` 表（DB），`PlatformServerModule` 启动时读 | 暂不读库，显式注入空表 | 与实测数据（空串）行为一致；等 PC 配置接进 PG 后补 |

---

## A18 `favourable.rs` + `install_favourable.rs`（17101 / 17102 / 17103）

对应 `FavourableMessageHandler.java:29-83`、`FavourableModel.java`（110 行）、
`PlatformServerModule.java:88-90`（字段）+ `:357-377`（读取）。

### ⚠ 三条都不走 `ErrorResp_1`

三个方法的签名**都没有** `throws HandlerException`，方法体内也没有 `throw`：

| cmd | 方法签名 | 失败出口 | 帧号 |
|---|---|---|---|
| 17101 | `public Message getFavourable(...)` 无 `throws` | 无失败分支 | 17101 |
| 17102 | 无 `throws` | `CloseFavourableWindowResp_17102{ret}` | 17102 |
| 17103 | 无 `throws` | `FeedBackResp_17103{ret}` | 17103 |

### ⚠ 17102 渠道不符回 `ret=0`，不是错误码

```java
int channelId = player.getChannelId();
if (!PlatformServerModule.FAVOURABLE_CHANNELS.contains(Integer.valueOf(channelId))) {
    return builder.setRet(0).build();   // ← 0，且不带任何数据
}
CommentDB commentDB = CommentDBCache.getCache().get(id);
if (commentDB == null) {
    return builder.setRet(LanguageUtils.ACTIVITY_PARAM_ERROR.getValue()).build();  // 2754
}
```

客户端收到 `ret=0` 会把这次关窗当成"没发生过"（`lastCloseTime=0`、列表空）。

### ⚠ `closeWindow` 的取模翻转有三处反直觉

```java
if (!openCnt.containsKey(id)) {
    openCnt.put(id, 1);
} else {
    int maxTimes = commentDB.getMaxTimes();
    if (maxTimes > 0) {
        if (maxTimes != openCnt.get(id)) isRefresh = false;      // ← 是 !=，不是 <=
        openCnt.put(id, (openCnt.get(id) + 1) % maxTimes);
    }
}
if (isRefresh) lastCloseTime = now;
```

1. **`maxTimes == 0` 时什么都不做** —— 计数停在旧值，但 `isRefresh` 保持 `true`，
   所以 `lastCloseTime` **每次都会刷新**。
2. **`maxTimes != 当前计数` → 不刷新时间**。因为后面 `% maxTimes` 把计数限制在
   `[0, maxTimes)`，`maxTimes == 当前计数` 只可能出现在 `maxTimes == 1` 且计数为 `1`。
   也就是绝大多数情况下 `lastCloseTime` **不会**被更新。
3. `(count + 1) % maxTimes` 让计数在 `0..maxTimes-1` 循环。

三条各有一个单测锁：
`close_max_times_zero_freezes_count` /
`close_refresh_only_when_count_equals_max_times` /
`close_wraps_with_modulo` + `close_wraps_back_to_zero`。

### ⚠ 17103 渠道判定优先于「已评价」

```java
if (!FAVOURABLE_CHANNELS.contains(channelId)) return setRet(FAVOURABLE_ERROR_1);  // 3941
if (model.isFeedBack())                       return setRet(FAVOURABLE_ERROR_2);  // 3942
```

单测 `feedback_channel_checked_before_already_done` 锁这条。

### ⚠ 17101 已评价时只回 `ret=0`

```java
if (!isFeedBack) {
    builder.setLastCloseTime(...);
    for (...) builder.addMsgFavourable(...);
    builder.setCafeUrl(CAFE_URL);
    builder.setOneStoreUrl(ONE_STORE_URL);
}
return builder.setRet(0).build();
```

已评价过 → 四个字段全不设置。

### 实测数据状态

| 数据 | 实测 | 后果 |
|---|---|---|
| `CommentDB`（445 张 CSV 里搜不到这个实体） | 不存在 | `CommentDBCache.get(id)` 恒 `null` → **17102 恒回 2754** |
| `bf_config.configId=19/20/21`（`platform.sql:517-519`） | 全是 `NULL` | `CAFE_URL`/`ONE_STORE_URL` 为空、`FAVOURABLE_CHANNELS` 为空 → **17103 恒回 3941** |
| `ConfigDBCache.CommentReward`（`B-变量表-Config.csv:456`，id=456） | `1=100;231=2;211=1;202=1` | 4 个奖励，但走不到发放分支 |

### 端到端实测（2026-10-28）

启动日志：`好评奖励已装载 count=4` / `服务就绪 handlers=78`

```text
17101 → ret=0, lastCloseTime=0, 列表空, 无 URL
17102 → 帧号 17102, ret=0        （渠道不符，不是 2754 —— 渠道判定在前）
17103 → 帧号 17103, ret=3941     （渠道没开放好评系统）
```

### 分支明细

| cmd | 分支 | 原版位置 | Rust | 用例 | 状态 |
|---|---|---|---|---|---|
| 17101 | 未评价 → 四字段 | `:35-46` | `install_favourable` 内联 | 端到端 | ✅ |
| 17101 | 已评价 → 只回 ret=0 | `:47` | 同 | 端到端 | ✅ |
| 17102 | 渠道不符 → ret=0 | `:52-54` | `close_verdict` | `close_not_in_channel_returns_ok` | ✅ |
| 17102 | 模板缺失 → 2754 | `:56-59` | 同 | `close_missing_template_is_2754` | ✅ |
| 17102 | 首次 → count=1 | `:62-63` | 同 | `close_first_time_sets_count_one_and_refreshes` | ✅ |
| 17102 | `maxTimes=0` → 冻结计数 | `:65-71` | 同 | `close_max_times_zero_freezes_count` | ✅ |
| 17102 | `maxTimes != count` → 不刷新 | `:67-69` | 同 | `close_refresh_only_when_count_equals_max_times` | ✅ |
| 17102 | `(count+1) % maxTimes` | `:70` | 同 | `close_wraps_with_modulo` / `..back_to_zero` | ✅ |
| 17103 | 渠道不符 → 3941 | `:66-68` | `feedback_verdict` | `feedback_not_in_channel_is_3941` | ✅ |
| 17103 | 已评价 → 3942 | `:74-76` | 同 | `feedback_already_done_is_3942` | ✅ |
| 17103 | 通过 → 发奖 | `:77-81` | 同 | `feedback_accepts_with_rewards` | ✅ |
| 配置 | `configId=19/20/21` 解析 | `:357-377` | `PlatformUrls::parse` | `parse_urls_and_channels` / `parse_all_none_yields_empty` | ✅ |

### 已知分叉

无。三条的每个分支都对上了。

---

## A19 `drop.rs` + `pray.rs` + `install_pray.rs`（掉落底座 + 17071 / 17072）

对应 `DropUtils.java`（160 行）、`WeightRandomModelUtils.java`、
`DropOneDB` / `DropTwoDB` / `DropSignDB`、`PrayMessageHandler.java`（150 行）、
`PrayModel.java`、`PrayModule.java`、`PrayDBCache`。

### 掉落三层结构

```text
DropOneDB（一级）   extractNum 次抽取，每次从 idTwo 里挑一个二级
  ↓ 可被 givenDropId 命中（按累计次数强制指定）
DropTwoDB（二级）   formulaId 判公式 → 命中则用该二级的 randItem()
  ↓ 没命中公式 → 按 probability 权重抽
DropSignDB（下标）  addType / resetType 控制「抽了多少次」的累加与重置
```

### ⚠ 权重和不足时有**哑元空抽**

```java
new WeightRandomModelUtils(itemWithWeights, 1000000).random()
```

第二个参数是 `minWeightTheSum`。实测 `D-掉落2-DropTwo.csv` 762 行里：
- **745 行**权重和正好 `1000000` → 无哑元
- **17 行**不足 → 补一个 `weight = 缺额`、`t = null` 的哑元

| id 段 | 权重和 | 空抽概率 |
|---|---|---|
| `10101`~`11001` 等 12 行 | 500000 | **50%** |
| 1 行 | 600000 | 40% |
| 1 行 | 450000 | 55% |
| 1 行 | 400000 | 60% |
| 1 行 | 350000 | 65% |
| 1 行 | 250000 | 75% |

这是保底前的稀有掉落配置，不是 bug。单测
`weight_sum_distribution_matches_measurement` 直接断言 **745 / 17**。

⚠ 补齐时哑元的 `weight` 用的是**补齐后的总和**（`this.weightTheSum`），
不是差值 —— `WeightRandomModelUtils.java:56`。

### ⚠ `formulaRand` 的区间与比较

```java
boolean ret = UtilsTools.random(0, FeastSeatInfo.CANNOT_ATTACT_TIME_OFFSET) < value;
```

- `random(0, 1000000)` 是**闭区间** → 1000001 个值
- 比较是 `<` **严格小于**

### ⚠ 两个公式的 VIP 项是整数除法

```java
float value2 = 1.0f - (player.getVipLv() / formulaValue.get(2));   // int / int
```

`getVipLv()` 与 `formulaValue.get(2)` 都是 `int`，除完是**整数**再转 `float`。
VIP 低于除数时 `1.0f - 0 = 1.0f`（不打折）；高于除数才会 < 1；
`>= 2*除数` 时 `v2 < 0` → 返回 0（永不命中）。
单测 `formula1_computes_with_integer_vip_division` 锁这三个档位。

### ⚠ `randOnce` 的累加是「无条件」的

```java
if (!givenDropId.isEmpty()) {
    two = getGivenDropId(one, modelOne);   // 内部用 times + 1 比对
    modelOne.addTimes(one.getId(), 1);     // ⚠ 不管有没有命中都 +1
}
```

且累加发生在 `getGivenDropId` **之后**（后者用的是 `times + 1`）。

### ⚠ `DropSignDB.onRand` 的四种组合

```java
if ( randOK && addType == 1) counter.addTimes(id, 1);   // 成功累加（保底计数）
if (!randOK && addType == 0) counter.addTimes(id, 1);   // 失败累加（连败计数）
if ( randOK && resetType == 1) counter.resetTimes(id);  // 成功重置
```

### 祈愿的三个保底层

```text
前 10 次（special = firstReward <= 0 && times < 10）
  → 30% 提前出核心（isHitForTenThousand(3000)）
否则
  → 幸运值满（luckNum >= 150）必出核心
否则
  → 走掉落表 DropUtils.rand(dropId)
```

`isHitForTenThousand(3000)`：`randomIndex(10000) < 3000` —— **恰好 30%**。

### ⚠ 单抽免费只看日期，十连没有免费

```java
boolean free = prayModel.getFreeTime() < LogicUtils.getTodayZeroTimeStamp();
if (free) prayModel.setFreeTime(getTodayZeroTimeStamp());
else if (!removeResources(oneCostJson)) throw PRAY_ERROR_COST;   // 3902
```

`freeTime` 记的是「上次免费用掉的那天的零点」，初始 `0` → 第一次必然免费。
十连（`prayTenTimes`）**无条件扣费**。

### ⚠ `addLuckNum` 先判后加

```java
if (this.luckNum >= maxLuckNum) { this.luckNum = addNum; this.cycle++; }
else                             { this.luckNum += addNum; }
```

到上限后**下一次**才回落并 `cycle+1`，会短暂留下 `luckNum >= max` 的状态。
而 `doPray` 的判定 `luckNum >= prayDB.getLuckNum()` 正靠这个状态命中保底 ——
逻辑自洽。单测 `add_luck_num_wraps_at_max_on_next_call` 锁这条。

### ⚠ 17071 一次请求发两个包（顺序：先推后回）

```java
MessageUtil.sendToPlayerId(MsgPrayDataResp_17074{...}, player.getId());
return PrayResp_17071{ret=0, rewards};
```

**本轮顺带修了一个框架 bug**：`ws` / `tcp` 的会话循环里推送走 `mpsc`、
回包走 socket 直写，两条通道独立，`select!` 返回回包后径直写回包 →
客户端看到「先回包、后推送」。修法是在写回包前先 `try_recv` 清空已入队的推送。
commit `cd05ab3`。

### 公告栏只保留 5 条

`PrayModule.handleEvent`：`while (noticeList.size() > 5) noticeList.remove(0)`。

### 分支明细

| 模块 | 函数 | 原版位置 | 用例 | 状态 |
|---|---|---|---|---|
| drop | `pick_weight` | `WeightRandomModelUtils:55-70` | `pick_first_item_when_roll_small` / `pick_second_item_at_boundary` / `pick_last_item_at_total` / `insufficient_weight_yields_empty` / `exact_weight_sum_has_no_dummy` / `zero_weight_is_reported` | ✅ |
| drop | `given_drop_id` | `DropUtils:83-93` | `given_drop_matches_nth_pull` / `given_drop_empty_never_matches` | ✅ |
| drop | `formula_value` | `DropUtils:131-163` | `formula1_zero_when_times_not_reached` / `formula1_computes_with_integer_vip_division` / `formula2_needs_two_signs` / `formula2_zero_when_second_sign_not_reached` | ✅ |
| drop | `formula_hit` | `DropUtils:95-102` | `formula_hit_is_strict_less_than` | ✅ |
| drop | `sign_action` | `DropUtils:165-176` | `add_type_1_accumulates_on_success` / `add_type_0_accumulates_on_failure` / `reset_type_1_resets_on_success_only` | ✅ |
| drop | `merge` | `ResourcesJson.mergeResourcesJson` | `merge_sums_same_id` / `merge_empty` | ✅ |
| pray | `is_free` | `PrayMessageHandler:70-74` | `first_pull_is_free` / `same_day_second_pull_costs` / `next_day_is_free_again` | ✅ |
| pray | `PrayTimes` | `:58` | `times_one_is_single` / `any_other_times_is_ten` | ✅ |
| pray | `draw_verdict` | `:100-112` | `special_and_hit_gives_core` / `special_miss_falls_through_to_luck_check` / `luck_full_gives_core` / `luck_over_max_also_gives_core` / `not_special_not_full_uses_drop_table` / `special_takes_priority_over_luck` | ✅ |
| pray | `add_luck_num` | `PrayModel:70-80` | `add_luck_num_accumulates_below_max` / `add_luck_num_wraps_at_max_on_next_call` / `add_luck_num_does_not_wrap_at_max_minus_one` | ✅ |
| pray | `by_open_day` | `PrayDBCache` | `by_open_day_below_first_returns_none` / `by_open_day_matches` / `by_open_day_picks_latest_matching` / `by_open_day_empty_list` | ✅ |
| pray | `push_notice` | `PrayModule:17-32` | `notice_keeps_last_five` | ✅ |
| csv | 三表 + 祈愿表加载 | — | `loads_real_tables` / `loads_real_pray_table` / `weight_sum_distribution_matches_measurement` | ✅ |

### 端到端实测（2026-10-28）

启动日志：

```text
掉落表已装载  drop_one=407 drop_two=762 drop_sign=73
祈愿表已装载  pray_rows=1
服务就绪      handlers=80
```

```text
17072 → 公告栏为空
17071 单抽（首次免费）→ 先推 17074{prayData{freeTime=1793116800000, luckNum=1, cycle=0}}
                       再回 PrayResp_17071{ret=0}
17071 十连（无免费，新号无 263）→ 帧号 1, ErrorResp_1{code=3902}
17071 二次单抽（同一天不免费）  → 帧号 1, ErrorResp_1{code=3902}
```

`freeTime=1793116800000` = 2026-10-28 00:00:00 +08:00（今天东八区零点），
`luckNum=1`（每次 +1），与 `Pray.csv` 的 `everyLuckNum=1` 一致。

### 已知分叉

| 场景 | 原版 | 本工程 | 影响 |
|---|---|---|---|
| `PrayDBCache.getByServerOpenDay` 表空 | `list.get(0)` → `IndexOutOfBoundsException` | 返回 `None` → 回 3901 | 只在畸形配置下分叉（实测表有 1 行） |

---

## A20 `system_open.rs` + `install_system_open.rs`（系统开放底座）

原版：`data_d/game_7010544/sources/bf/game/server/utils/SystemOpenUtils.java`（`isOpen`）
+ `data_d/libs/jb3.common.v1/sources/bf/template/db/SystemOpenDB.java`
+ `data_d/libs/jb3.common.v1/sources/bf/template/cache/SystemOpenDBCache.java`
数据：`G-功能开启表-SystemOpen.csv`（实测 **100 行**）+ PG `bf_server.open_time`

### 为什么先做它

`isOpen` 是 `data_d` 里被引用最多的单点：**60 个文件**调它，其中 22 个在
`message/handler/`。此前一直没接，导致 1015 恒回 `ret=2`、17071 的门禁是空壳
（`let _ = SYSTEM_OPEN_PRAY;`）、17081 / 41001 完全悬空。

### 逐分支

| `unlockType` | 原版位置 | 原版判定 | Rust | 用例 | 状态 |
|---|---|---|---|---|---|
| 1 | `:44` | `player.getLv() >= parseInt(value)` | `f.level >= num` | `level_gate` | ✅ |
| 2 | `:49` | 通关关卡 id | `f.stage_id` | — | ✅ |
| 3 | `:54` | `player.getVipLv()` | `f.vip_lv`（**过渡期恒 0**） | `facts_carry_level_and_open_day_only` | ⚠ |
| 4 | `:60` | `TalentSet.getAllTalent().size()` | `f.talent_count` | `multi_type_is_or` | ✅ |
| 5 | `:66` | `AccountRoom.getOpenId()` | `f.account_room_open_id` | — | ✅ |
| 6 | `:74` | `TraderSet.getTradeNum()` | `f.trade_num` | — | ✅ |
| 7 | `:82` | `ChildrenSet.isHaveChild()` | `f.have_child`（不看 value） | `type_7_and_16_ignore_value` | ✅ |
| 8 | `:88` | `PetSet.getAllPet().size()` | `f.pet_count` | — | ✅ |
| **9** | **无 case** | — | `return None` | `absent_case_types_never_open` | ⚠ |
| 10 | `:96` | `BuildingSet.getBuildingCount()` | `f.building_count` | — | ✅ |
| **11 / 12 / 14 / 23 / 25+** | **无 case** | — | `return None` | 同上 | ⚠ |
| 13 | `:102` | `GsServer.getOpenServerDay()` | `f.open_server_day` | `real_open_time_row_120_opens_on_day_two` | ✅ |
| 15 | `:115` | `HuntingArea.getMaxMapId()` | `f.hunting_max_map_id` | — | ✅ |
| 16 | `:122` | `Homeland.isEnabled()` | `f.homeland_enabled`（不看 value） | `type_16_and_14_reads_flag_not_value` | ✅ |
| 17 | `:128` | `family != null && family.getLv() >= v` | `f.family_lv` | `type_17_needs_family` | ✅ |
| 18 | `:136` | `for (t : values) if (getTalent(t) != null)` | `any_talent_owned`（扫**整个** value） | `type_18_scans_whole_value_list` | ✅ |
| 19 | `:143` | `player.getFortuneLv()` | `f.fortune_lv` | — | ✅ |
| 20 | `:148` | `getTalentNum(values.get(0)) >= 1` | `any_talent_at_level`（取**首值**） | `type_20_uses_first_value_as_talent_level` | ✅ |
| 21 | `:155` | `NpcSpouseModel.getUnlockNpcIds().size()` | `f.npc_spouse_count` | — | ✅ |
| 22 | `:161` | `CloseFriendSet.getCloseFriendNum()` | `f.close_friend_num` | — | ✅ |
| 24 | `:176` | `getLongStat(BuildingEvent)` | `f.building_event_stat` | `type_24_uses_long_stat` | ✅ |

### ⚠ 三个照抄的反直觉行为

**1. `unlockType` 数组是 OR 语义，不是 AND**

```java
for (i = 0; i < unlockTypesSize; i++) {
    switch (unlockType[i]) { case 1: if (...) return true; ... }   // ← 命中就 true
}
return false;
```

CSV 里 `13;4 = 5;5`（27 / 28 行）表示「开服 5 天**或**门客 5 个」都能开。

**2. `unlockType` 常量存在 ≠ `isOpen` 处理了它**

`SystemOpenDBCache` 定义了 `TYPE_1`…`TYPE_305` 共 60 个常量，
但 `isOpen` 的 `switch` 只有 20 个 case。实测 CSV 里 `type=9`（108 行
「子嗣-全部培养」）与 `type=12`（114 行「产业界面-关卡按钮」）都在
**没有 case 的名单里** → 这两行**永远不开放**。不补 case。

**3. `16` 有 case 但 `14` 没有**

容易被 `TYPE_16` 这类常量名误导。以 `isOpen` 的 `switch` 为准。
`17`（家族等级）也有 case，别当成缺失项。

### ⚠ 过渡期只启用 2 个事实

| 事实 | 来源 | 状态 |
|---|---|---|
| `level` | `PlayerBox.level`（登录时从 PG 读） | ✅ 真实 |
| `open_server_day` | PG `bf_server.open_time` | ✅ 真实 |
| 其余 16 项 | 家族 / 产业 / 狩猎 / 门客 / 珍兽 / 知己 / 账房 / 通商 / 子嗣 / 府邸 / 才华 … 未接 | ⬜ 恒默认 |

**没有**用近似字段顶替未接语义。例如 `unlockType=20` 要的是「达到某阶的**门客阶数**」，
而 `PlayerBox.pet_skill_class` 是**技能阶级**，拿它近似会让本该关闭的功能误开。
默认值是「不满足」，只会让功能**晚开**，不会误开。

`unlockType=3`（VIP）受影响最大：23 行用到它，最低门槛 3 级，
所以这些功能对**所有**玩家关闭，直到 VIP 等级落库。

### ⚠ 数据缺口（原版线上也是这个状态）

PG `bf_config` 里 `configId` 23 / 24（渠道祈愿的时段与档位）实测是**空串**，
所以 `cidTimeMap` / `cidChargeMap` 为空 → 17081 / 17082 会恒回
`PRAY_CID_ERROR_UN_OPEN(3951)`。这是原始数据状态，不是移植 bug。

### 已知分叉

| 场景 | 原版 | 本工程 | 影响 |
|---|---|---|---|
| `unlockType` 比 `value` 长 | `values.get(i)` → `IndexOutOfBoundsException` → 整个 `isOpen` 抛出 | 跳过该对，继续判后面的 | 只在畸形配置下分叉（实测 100 行全部等长） |
| `value` 非数字 | `Integer.parseInt` → `NumberFormatException` → 抛出 | 判「不命中」 | 同上 |
| `SystemOpenDB` 为 null | `return false` | 同（空表） | 一致 |

### 端到端

1015 现在走真实模板：查不到 `systemId` → `ret=2`（原版 NPE 被 catch），
查得到且 `name` 末段是数字 → `ret=0`。`name` 末段不是数字（如 `change_word_626`）时
`LogicUtils.getDescribeName` 取末段 `626` → 解析成 626，`ret=0`。

---

## A21 `activity_template.rs` + `install_activity.rs` + `pg_activity.rs`（活动模板底座）

原版：`data_d/game_7010544/sources/bf/game/server/module/activity/ActivityDbTemplate.java`
（239 行）+ `AbsActivityDBTemplate` + `ActivityDbTemplateCache`
数据：PG `bf_activity`（实测 **11 行**）

### 为什么先做它

活动族处理器**全部**都先查 `ActivityDbTemplate`，再调
`isOpen()` / `isBetweenStartAndEndShowTime()` / `isEndShowTime()` 之一。
这一层不接，50+ 个活动处理器（`Cornucopia` / `Wonderland` / `Bridge` /
`ActivityNianbeast` / `GroupBuy` / `Fishing` / `LanternRiddles` / `GmAnnual` …）
全都只能写「占位 ret=501」。

### 六个判定

| 方法 | 原版 | 区间口径 | Rust | 用例 |
|---|---|---|---|---|
| `isOpen` | `:100` `isNowInTime(start, now, end)` | **闭** `start <= now <= end` | `is_open` | `is_open_is_closed_interval` |
| `isShowTime` | `:112` | **闭** `startShow <= now <= endShow` | `is_show_time` | — |
| `isBetweenStartAndEndShowTime` | `:124` | **开** `start < now < endShow` | `is_between_start_and_end_show_time` | `is_between_start_and_end_show_time_is_open_interval` |
| `isEnd` | `:120` | `now > end` | `is_end` | `is_end_and_is_end_show_time_are_strict` |
| `isEndShowTime` | `:108` | `now > endShow` | `is_end_show_time` | 同上 |
| `isGainRewardStage` | `:132` | `end <= now < endShow` | `is_gain_reward_stage` | `gain_reward_stage_is_between_end_and_end_show` |

### ⚠ 四个照抄的反直觉行为

**1. `isOpen` 闭区间 vs `isBetween...` 开区间**

```java
public boolean isOpen() { return UtilsTools.isNowInTime(startTimeLong, now, endTimeLong); }
public boolean isBetweenStartAndEndShowTime() {
    return now > this.startTimeLong && now < this.endShowTimeLong;   // ← 手写 >
}
```

`isNowInTime` 含两端。**正好卡在边界那一毫秒**两者结论相反。

**2. `isEnd` / `isEndShowTime` 都是 `>` 不是 `>=`** —— 结束那一毫秒仍算未结束。

**3. `isOpen()` 不看 `switchStatus`**

实测 `actId=100043242`（星斗森林）`switchStatus=0`，但只要时间在范围内
`isOpen()` 照样 `true`。关闭靠上层别的地方拦，不在模板层。

**4. 缓存键是 `(typeId, actId)` 二元组**

同一个 `actId` 配不同 `typeId` 是两行。上层传错 `actType` 就查不到 → `null`
→ 抛 `ACTIVITY_NO_EXITS(2751)`。

### 当前数据的真实状态

11 行时间全是 `2023-07-20 09:00:00 ~ 2026-07-26 23:59:59`，
**当前时刻全部已结束**。这是原始数据状态，不是移植 bug ——
现有数据下活动族只能验「结束 / 未开放」错误出口，验不到成功路径。

`timeType` 全是 `1`（绝对时间），所以 `timeType = 2`（按开服天数偏移，
格式 `3#10:00:00`）**无数据可验**，本次不实现。

### 已知分叉

| 场景 | 原版 | 本工程 | 影响 |
|---|---|---|---|
| 时间列为 NULL / 非法 | `UtilsTools.getDateTime` 返回 `0L` → `isEnd()` 在 `now > 0` 时**恒真** | 返回 `None` → 所有判定 `false`（不开放） | 只会让活动**晚开**，不会误开 |

### 聚宝盆 38303 / 38304

| cmd | 原版 | 依赖 | 状态 |
|---|---|---|---|
| 38301 开始寻宝 | `cornucopiaActPlayer.startMap(player)` | `CornucopiaActPlayer` 完整实体 + 掉落 | ⬜ |
| 38302 抽奖 | `drawItem(player, pos)` | 同上 + `WeightRandomModelUtils` | ⬜ |
| **38303 自己的记录** | `querySelfRecord()` | `records` 队列（逻辑为零） | ✅ |
| **38304 全服最近记录** | `queryRecentRecords()` | `recentRecords` 队列（逻辑为零） | ✅ |

**38303 不检查 `isOpen()`**：原版调 `getCornucopiaActPlayer(playerId, actId, false)`
—— 第三个参数 `checkOpen = false`，活动结束后仍能查自己的历史记录。
38304 同样只查模板存在性，不查 `isOpen`。

`CornucopiaActCommon.recentRecords` 的上限是 **10** 条（`while (size > 10) poll()`）；
`CornucopiaActPlayer.records` 没有上限。

---

## A22 `travel.rs` + `install_travel.rs` + `csv_travel.rs`（游历 15002 / 15004 / 15009）

> 基准：`data_d/game_7010544/sources/bf/game/server/message/handler/TravelMessageHandler.java`
> + `.../module/travel/Travel.java`、`TravelTempResult*.java`、`TypeTravelHelper.java`、
> `.../utils/TravelEventsDBUtils.java`、`bf/template/db/TravelEventsDB.java`。
> 纯逻辑 26 测试（`travel.rs`）+ 装配 4 测试 + CSV 5 测试。

### 三条消息的入口链

| cmd | 方法 | 前置（按执行顺序） | 失败出口 |
|---|---|---|---|
| 15002 | `travel(Player, MsgTravelReq_15002)` | ① `place != 0 && place != 12` → `1006`<br>② `!isOpen(3)` → `1003`<br>③ `allTimes != 0 && !isOpen(102)` → `1004`<br>④ `place==12 && !isOpen(26)` → `2356`<br>⑤ `place==12 && travelCnt>10 && !isOpen(139)` → `17` | `ErrorResp_1` |
| 15004 | `travel(Player, MsgTravelTenReq_15004)` | ① `!isOpen(133)` → `1003`<br>② `curNum==0 && item22==0` → `2` | `ErrorResp_1` |
| 15009 | `travel(Player, PartnerSexReq_15009)` | ① `sex != 0 && sex != 1` → `2`<br>② `getPartnerSex()==sex` → `2`<br>③ `!isOpen(161)` → `1003`<br>④ `!removeResources(PartnerChangeCost)` → `2` | `ErrorResp_1` |

⚠ **15002 的 ④⑤ 在 `randomEvent()` 里**，晚于 ③。即 `place=12` 且 `allTimes!=0`
时先判 102 再判 26/139。

### `randomEvent()` 的计数规则（最易错处）

```java
long curNum = CounterRecovery.getNum(playerId, 1);   // 体力计数器
if (place == 12) {                                   // 桃花岛：走印章 24
    if (allTimes != 0) {
      if (travelTimes >= 10 && (travelCnt <= 10 || travelTimes >= Math.min(travelCnt, PartnerInviteTime))) break;
      if (!checkResources(FixedPointTravel)) { if (travelTimes == 0) break; throw 1005; }
      ...; travelTimes++;
      if (allTimes == 0 || travelTimes >= curNum) break;
    } else {
      if (travelTimes > 0) break;
      if (!checkResources(FixedPointTravel)) { if (travelTimes == 0) break; throw 1005; }
      ...; travelTimes++;
    }
} else {                                             // 普通：走体力
    if (!checkNum(pid, 1, PhysicalStrength * eventDBs.size() + 1)) {
      if (travelTimes != 0) break; else throw 1001;
    }
    ...; travelTimes++;
}
```

三条照抄出来的结论：

1. **体力分支上限 = `floor((curNum - 1) / PhysicalStrength) + 1`**
   —— 判的是 `PhysicalStrength * 已抽数 + 1`，不是 `PhysicalStrength * (已抽数+1)`。
   实测 `PhysicalStrength = 1` → 上限就是 `curNum`。
2. **`place == 12 && allTimes == 0` 固定只出 1 个**（第二次进来 `travelTimes > 0` 就 break）。
3. **`Math.min(travelCnt, PartnerInviteTime)` 在 `n < 10` 时永远走不到**
   —— 外层 `travelTimes >= 10` 先判。实测 `PartnerInviteTime = 50`，
   所以 `travelCnt` 只在 `n >= 10` 时才真正生效。这条写进了单测
   `plan_fixed_travel_cnt_invite`（首版期望 `(5,false)` 实测 `(10,false)`，按原版改回）。

### `travelTenTimes()`

```java
long curNum = getNum(pid, 1);
long maxItemNeed = curNum == 0 ? 10 : 9;
if (maxItemNeed > item22.getNum()) maxItemNeed = item22.getNum();
if (curNum == 0 && maxItemNeed == 0) throw COMMON_SERVER_BUSY;   // 2
for (i < curNum)     eventDBs.add(randTravel(player, this, 0));
for (i < maxItemNeed * vip.getCounterMax(1)) eventDBs.add(randTravel(player, this, 0));
if (curNum > 0) removeNum(pid, 1, curNum, false);
removeResources(new ResourcesJson(22, maxItemNeed), ...);
```

⚠ **全部 `place = 0`（随机地点）**，不是当前 `place`。
⚠ 末尾的 `15001` 用**旧的** `this.place`（十连不改 `this.place`）—— 与 15002 相反。

### `tryGetSpecialTravelEvent` 的副作用

```java
if (place != 0) return null;
long times = getLongStat(NotPlaceTravelTimes);
if (times >= 100) return null;
addLongStat(NotPlaceTravelTimes, 1);          // ⚠ 先自增
times = getLongStat(NotPlaceTravelTimes);     // ⚠ 再读
for (ResourcesJson c : ConfigDBCache.TravelFirstEventGirl)
    if (times == c.getId()) return cache.get(c.getNum());
return null;
```

即「第 N 次随机游历送第 N 个固定事件」。实测 `TravelFirstEventGirl`
`= 1=9101;2=12401;3=7301;4=12401;5=2101`，这 5 个 id **都在事件表里**。
`NotPlaceTravelTimes >= 100` 之后不再送，但**计数也不再涨**（`return null` 在自增前）。

### `filtrateChild()`

```java
for (MsgTravelEventData e : src)
  for (MsgResources r : e.getMsgResourcesList())
    try { new BigInteger(r.getNum()); } catch (Exception ex) { list.add(e); }
```

把**外层 `e`** 加进结果，每个非法 `num` 加一次（不是内层 `r`）。
实测所有 `num` 都是数字 → 恒空。已照抄（含「按非法项个数重复添加」这一点）。

### 事件结算四分支（`TypeTravelHelper`）

| type | 名称 | 未满进度 | 已满 / 无进度要求 |
|---|---|---|---|
| 1 | 门客 | `addProgressId` + 空列表 | `talent == null && !containNewId` → `addTalentId` + `[(targetId,"1")]`；否则 `addReward(getRewardJson())` |
| 2 | 珍兽 | 同上 | `pet == null && !containNewId` → `addPetId` + `[(targetId,"1")]`；否则 `addGrowth(intimacyJson.num)` + `[intimacyJson]` |
| 3 | 奖励 | **无进度判定** | `addRewardJson(id, getRewardJson())` |
| 4 | 美人 | 同上 | 见下 |

type 4 已满分支：`useMiracle(ID_NPC_INTIMACY)` 非空时 `intimacy = intimacy * param2 / 100`
且 `multiple = param2`（本模块 **恒 0**，奇遇未接）；再判
`beforeTimes < TravelFirstEventOffspring && times >= TravelFirstEventOffspring`
（实测门槛 4）→ `childrenSet.bornByTravel(tid, max(0,intimacy), isFirst)`。

### 事件表实测（`Y-游历事件-TravelEvents.csv`）

84 数据行。`place` 分布 `12×12 / 1,4,5,6,7,10×7 / 2,3,8,9,11×6` —— **没有 `place = 0`**
→ 走权重抽取时永远抽不到，随机游历只能靠 `TravelFirstEventGirl` 命中。
`type` 分布 `2×36 / 1×18 / 3×18 / 4×12`；`unlockType` 分布 `1×46 / 2×20 / 0×18`；
`weightScale` 只用 `1.2 / 0.8 / 1 / 1.3` 四档。

### 本轮明确未接（不是 bug，是依赖未到）

| 未接项 | 原版位置 | 为什么 |
|---|---|---|
| 门客实体落地 | `TalentSet.getTalent` / `addTalentByTravel` / `reCalBuildingSpeedByTravel` | 门客模块未移植 |
| 珍兽实体落地 | `PetSet.getPet` / `addGrowthByTravel` / `refreshCommon` | 只发了 `7001` 推送，不写实体 |
| 美人 / 子女 | `NpcSpouseModel` / `ChildrenSet.bornByTravel` | 两个实体模块未移植 |
| 时装加亲密度 | `FashionModel.getFashionSuitPartAddIntimacy()` | 时装模块未移植 |
| 奇遇加成 | `useMiracle(ID_NPC_INTIMACY)` | 奇遇模块未移植 → `multiple` 恒 0 |
| 事件日志 | `EventLogSendRecord.tour` / `.mansionSex` | 日志表未接 |
| 事件模块通知 | `EventModule`（`ACTIVITY_TASK_10_TRAVEL_TIMES` 等） | 活动模块未移植 |
| 自动游历 | `AutoPlayTravel` | 挂机模块未移植 |

### 已知分叉

| 场景 | 原版 | 本工程 | 影响 |
|---|---|---|---|
| `place=12 && allTimes!=0` 但印章不足且 `travelTimes == 0` | `break` → `eventDBs` 为空 → 随后 `eventDBs.get(size-1)` 抛 `ArrayIndexOutOfBoundsException` | 回 `1005` | 客户端拿到明确错误码，不是 500 |
| `filter()` 结果为空（真实数据下不会，`place != 0` 都有行） | `WeightRandomModelUtils.random()` 抛 `IllegalArgumentException` | 返回 `None` → 该次不产生事件 | 只会少抽，不会多发 |
| `TravelFirstEventGirl` 里的 id 在事件表缺失 | `cache.get(id)` 返回 `null` → 随后 NPE | 退回权重抽取 | 只会在策划改表出错时出现 |
| 十连的 `vip.getCounterMax(1)` | 读玩家 VIP 等级 | 恒按 1（VIP 等级未接） | 高 VIP 少抽；接 VIP 后修正 |

---


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

## A23 `lantern_riddles.rs` + `install_lantern_riddles.rs`（猜灯谜 37603 / 37604 / 37605 / 37607）

> 基准：`data_d/game_7010544/sources/bf/game/server/message/handler/LanternRiddlesMessageHandler.java`
> + `.../module/activity/LanternRiddlesActPlayer.java`、`LanternRiddlesDbTemplateCache.java`、
> `LanternRiddlesActivityCommon.java`、`LanternRiddlesExtendCA.java`。
> 纯逻辑 41 测试（`lantern_riddles.rs`）+ 装配 5 测试（`install_lantern_riddles.rs`）。

### 四条消息的入口链

四个方法的**第一段完全相同**：

```java
int actType = TypeActivity.guessLanternRiddles.getType();            // 50
ActivityDbTemplate dbTemplate =
    ActivityDbTemplateCache.getInstance().getActivityDbTemplate(actType, actId);
if (!dbTemplate.isOpen()) throw new HandlerException(LanguageUtils.COMMON_SERVER_BUSY);   // 2
ActivitySet activitySet = ActivityModule.getInstance().getActivitySet(player.getId(), actType);
LanternRiddlesActPlayer activity = (LanternRiddlesActPlayer) activitySet.getActivity(actId);
```

| cmd | 方法 | 后续前置 | 失败出口 |
|---|---|---|---|
| 37603 | `lanternRiddlesGetSubject` | 无 | `ErrorResp_1` / `2` |
| 37604 | `lanternRiddlesChangeSubject` | ① `id ∉ [1,4]` → `5401`<br>② `lv < identityRestriction` → `33`<br>③ `rounds()==0` → `5400`<br>④ `lanternId != 计算值` → `5402`<br>⑤ 该题 `pick` 已为真 → `5405` | `ErrorResp_1` |
| 37605 | `lanternRiddlesExcludeOption` | ① `lv < identityRestriction` → `33`<br>② `rounds()==0` → `5400`<br>③ `outOfNum >= limit(0)` 或 `oneOutOfNum >= limit(1)` → `5403`<br>④ `lanternId` 不对 → `5406`<br>⑤ 道具 6181 不够 → `6` | `ErrorResp_1` |
| 37607 | `lanternRiddlesGetRwd` | ① `lv < identityRestriction` → `33`<br>② `rounds() != 0`（本轮还没结束）→ `5404`<br>③ `startDay==0 && rounds2()==0` → `5400`<br>④ 该 lun 已领过 → `5407` | `ErrorResp_1` |

### ⚠ 线上数据真实状态：猜灯谜从未上线

| 事实 | 证据 |
|---|---|
| `bf_activity` 里 **没有 `typeId = 50` 的行** | `grep -c` `data/sql/platform.sql` |
| `bf_operate_lantern_riddle` / `bf_operate_riddle_reward` 都是空表 | 同上 |
| `bf_config`（79 行）**没有 `LanternRiddleLimitNum`**，也没有 `LanternRiddleLLayTime` | 同上 |
| `LanternRiddlesExtendCA` 从未构造过 | 上面两条的直接后果 |

⇒ 原版四条消息在 `getActivityDbTemplate(50, actId)` 拿到 **`null`**，
`dbTemplate.isOpen()` **NPE**，被框架兜底成 `COMMON_SERVER_BUSY(2)`。

移植实现：`crate::install_activity::template(50, act_id)` 返回 `None` 时
直接回 `ErrorResp_1{msgId=cmd, errorCode=2}` —— **与原版行为一致**，不是移植 bug。

### 照抄的行为（看着像 bug，但原版就是这样）

1. **`rounds()` 取最后一个匹配槽位，没有 `break`**
   ```java
   for (i...) { if (openActivityTime.get(i) <= now - zero - start) round = i; }
   ```
   → 配置里多个槽位同时满足时取**最后**一个，不是第一个。测试 `rounds_picks_the_last_matching_slot` 固定了这点。

2. **`rounds2()` 判的是 `> 0` 而不是 `>= 0`**
   `rounds2` 忽略答窗，只看开赛：`openActivityTime.get(i) - (now-zero-start) > 0`。

3. **`value_of` 用 `ceil(毫秒/1000.0)`**
   59001ms → ceil 60s → 命中 key=60。不是 `floor`，不是四舍五入。

4. **`isTrue(actId, lanternId, num)` 的前置写成无效表达式**
   ```java
   if (!options.containsKey(A) || !options.containsKey(B) || !options.contains(num - 1)) { ... }
   ```
   实际有效判定只有 `options.get(num-1) == 1`；`num == 1` 时 `contains(0)` 恒假 → 走「题号错误」分支。
   Rust 侧照抄：`num < 1 || num > 4` → `false`？不 —— 照抄 `num-1` 的下标语义，`num=0` 时 `0-1` 为 `-1` 恒不匹配。

5. **`setRwdInfo(lun)` 写 `1` 而不是 `0`**
   ```java
   rwdInfo.put(lun, 1);
   ```
   所以「已领取」标记是 `1`。Rust 侧 `RiddleState::rwd_info: HashMap<i32, i32>` 照抄写 1。

6. **`RoundsInfo(long value, boolean pick)` 构造器把 `isTrue` 硬编码为 `true`**
   这个构造器只在 `excluded()` 路径用（排错排掉的选项），所以排掉的选项 `is_true == true`。
   `getTrueNum(lun)` 数 `isTrue` → **排错会虚增「答对数」**，这是原版真实行为。

7. **`ValueSort` 是 value 降序 + time 降序**
   `Long.compare(o2.getValue(), o1.getValue())`，相等时 `Long.compare(o2.getTime(), o1.getTime())`。

8. **`lun_value` 里排掉的选项仍然占位**
   `lunValue` 每次由 `recompute_lun_value()` 全量重算，不做增量。

### 未接线部分

| 项 | 原因 |
|---|---|
| **37606 `GetRankCsRemoteReq`** | 跨服排名 RPC，需要 `CsServer` |
| `LanternRiddlesDbTemplateCache` 的跨服拉题（`ActivityUtils.sendCross`） | 同上 |
| `LanternRiddlesActivityCommon`（全服 `state` / `lunPlayerIds` / `playIntrgral`） | 需要跨服活动实例 |
| `EventLogLanternRiddle.*`（四条事件日志） | 事件日志模块未接 |
| `sendMails()`（活动结束按排名补发邮件） | 邮件模块未接 |
| `ActivityLanternRiddlesModule.sendPlayerData()` | 活动模块未接 |

题目表 / 选项表 / 奖励表通过 `install_rows` / `install_rewards` / `install_cfgs` 注入，
未注入时用 `EmptyRows` / `EmptyRewards`，`one()` 恒 `None` → 37603 回空题、37604 `true_answer=0`。

### 状态位置

`RiddleState` 放在 `PlayerBox.lantern: HashMap<i32, RiddleState>`（按 `actId` 索引），
不落库 —— 与原版 `LanternRiddlesActPlayer` 一样是活动实例层内存态。

## A24 `ocean_trade.rs` + `install_ocean_trade.rs` + `csv_ocean_trade.rs`（远洋贸易 41001 / 41002 / 41003）

> 基准：`data_d/game_7010544/sources/bf/game/server/message/handler/OceanTradeMessageHandler.java`、
> `.../module/oceanTrade/OceanTradePlayer.java`、`.../OceanTradeService.java`、
> `.../OceanTradeCommonData.java`、`.../OceanTradeTime.java`、`.../OceanTradeServerModule.java`、
> `.../OceanTradeNegotiationCalculator`（`OceanTradePlayer` 内部类）、
> `data_d/libs/jb3.common.v1/sources/bf/template/db/OceanTradeDB.java`、
> `.../OceanTradeDBCache.java`。

### 三条消息的入口链

| cmd | 方法 | 前置（按执行顺序） | 失败出口 |
|---|---|---|---|
| 41001 | `enterOceanTrade(player)` | `isOpen(19)` | `ErrorResp_1` `3001` |
| 41002 | `oceanTradeRankList(player, type)` | **无**（连 `isOpen` 都不判） | 无 |
| 41003 | `negotiate(player, type, num)` | `isOpen(19)` → `seasonId == curSeasonId` → `!isEndGame()` → 结算内 `checkSeasonEndOrNextSeason` / `isOpen(112)` / `num` 越界 | `ErrorResp_1` `3001` / `3004` / `3002` / `3003` |

41002 无前置这一点是照抄：原版排行榜消息**不判系统开放**，41001/41003 才判。

### 照抄的行为（原版即如此，不是移植缺陷）

1. **`toNextCheckpoint` 先比较旧 id 再前移**
   ```java
   int curId = checkpointId;
   checkpointId = cache.getNextCheckpointId(curId);
   if (curId == cache.getLastCheckpointId()) endGame = true;
   ```
   ⇒ 最后一关被**打通的这一刻** `endGame` 才置真，同时 `checkpointId` 已经跳到 `nextId(last) = last`（`ceiling` 无解回原值）。所以 `endGame=true` 时 `checkpointId` **仍等于最后一关**。
2. **`getRate()` 的分母是 `getTotalMoney()`，`getDamage()` 的分子是 `getMoney()`**
   两个不是同一个量：分母含门客/子女加成，分子只是身上银两。
3. **`getCurCostMoney()` 读 builder 上已设的 `costMoney`**
   ⇒ `onWin` / `calNewCost` 里的 `cost = getCurCostMoney().add(...)` 是**累加**语义，每关在上一关的基础上叠加。
4. **`calNewCost` 的 `costMoney < preMoney` 兜底**
   ```java
   costMoney = subtractCost.multiply(rate);
   if (costMoney < preMoney) { costMoney = preMoney; subtractCost = subtractCost.add(ONE); }
   ```
   修正的是 `costMoney`，但 `subtractCost` 也跟着 +1，导致「花的钱」与「扣的血」不再严格 `cost = damage × rate` —— 原版如此。
5. **一键谈判（`type != 0`）不逐关扣钱**
   `onWinQuickly` 里没有 `player.removeMoney`，钱在循环外由 `removeMoney(totalCostMoney)` **一次性**扣；而 `calNewCostQuickly` 依然把每关的 `costMoney` 累加进 builder。
6. **一键的循环上限用 `getMaxCheckpointQuicklySize()`**
   ```java
   if (checkpointSize <= 0 || checkpointSize > getMaxCheckpointQuicklySize()) throw NUM_ILLEGAL;
   ```
   `getMaxCheckpointQuicklySize() = cache.checkpointSize() - cache.getPrevCheckpointId(checkpointId)`，
   `getPrevCheckpointId` 是 `floor(cp-1)`（无解回 **0**），所以它是「从当前关到末尾的关卡数 **加一**」——
   能一键的最多关数比理论值多 1（例：在关卡 1 上 `prev=0`、`size=400` ⇒ 可一次推 400 关，而实际只剩 400 关）。
7. **`refresh()` 的双条件**
   ```java
   if (currentSeasonId != curSeasonId && seasonStartTime != currentSeasonStartTime) { ... }
   ```
   两个条件都用**字段原值**比较，且第二个是 `!=` 而非 `>`；`seasonStartTime > 0` 才判 `intervalDay / OceanTradeSeasonTime > 1` 决定是否清 `progress`。
8. **`addOrUpdate` 的满榜守卫**
   ```java
   if (list.size() >= ActualNumber && list.get(list.size()-1).getCheckpointId() >= checkpointId) return;
   ```
   注意 `>=` —— 与榜尾**同分**也进不去。
9. **排行比较器是双键、次键方向相反**
   ```java
   checkpointId 降序；相同则 updateTime 升序
   ```
10. **`checkSeasonEndOrNextSeason` 让 41003 在赛季末尾必失败**
    `isEnd()` 或 `seasonId != current.id` 都抛 `3004`；而 `41001` 在同一时刻只是把 `seasonId` 设成 0。

### 本轮未接的部分（过渡实现）

| 原版 | 依赖 | 当前处理 |
|---|---|---|
| `player.getTotalMoney()` | `TalentSet.getTotalMoney()` + `ChildrenSet.getTotalMoney()` | `PlayerBox.total_money`，恒 `"0"`；`getRate()` 分母为 0 时退化为 `CheckpointCostInterval[0] = 0.2`（原版会抛 `ArithmeticException`） |
| 排行榜 `OceanTradeService` 全服 `TreeSet` | 跨玩家共享结构 + `PlayerOfflineDataServerModule` | `PlayerBox.ocean_rank: Vec<RankEntry>`（**每个玩家一份**，单实例下等价）；`MsgRankingEntryData` 只填 `key`/`var_str`/`rank`，名字/等级/装扮/家族留空 |
| `LastOceanTrade`（上赛季榜 + `handleTopThree`） | 赛季存档 | 41001 的 `top_three` 取当前榜前 3；41002 `type != 0` 也回当前榜 |
| `bf_ocean_trade_time` / `id_generator` | 赛季表 | `OceanTradeCommonData` 挂在 `PlayerBox.ocean_common`，懒滚动（`initDelay` 语义） |
| `DropUtils.rand(player, dropId)` | `DropOneDB` → `DropTwoDB` 全链路 | 走 `crate::drop::tables()`，只展开一层（`DropOne.id_two[0]` → `DropTwo` 权重），远洋贸易 400 行的 `dropId` 只有 121/122 两个固定组，够用 |
| `EventLogSendRecord` / `addLongStat` | 日志/统计模块 | 跳过 |
| `OceanTradeRewardMailId`（`Config = 147`，值 180） | 邮件模块 | 未接（赛季结束补发奖励的邮件） |

### 数据事实

| 表 | 路径 | 行数 | 备注 |
|---|---|---|---|
| 关卡 | `template/csv/Y-远洋贸易-OceanTrade.csv` | **400** | id 1..400 连续；`cost` 最大 `4680000000000000000`（贴 `i64` 上限 `9.22e18`）⇒ 用 `BigInt` |
| 排名奖励 | `template/csv/Y-远洋贸易排名-OceanTradeRank.csv` | 13 | 未接（赛季结算用） |
| 变量 | `template/csv/B-变量表-Config.csv` | — | `CheckpointCostInterval = 0.2;0.5;0.8;1.2;2`（取 **[0] = 0.2**）、`OceanTradeSeasonTime = 7`、`OceanTradeRankListDisplayQuantity = 500`、`OceanTradeRankListActualNumber = 2000`、`OceanTradeRewardMailId = 180` |

第 1 关 `power=2000000 cost=12250000000 reward=4=5;3=300000 dropId=121`，
第 400 关 `power=200067000000 cost=4680000000000000000 reward=4=5;3=35340000 dropId=128`。

### 大数语义

原版用 `BigInteger` / `BigDecimal`，这里用 `num-bigint::BigInt`：

- `getRate()` = `npcPower / totalMoney`，**5 位小数、`RoundingMode.DOWN`**，再与 0.2 取较大 → 内部存「放大 `10^5` 的整数」`rate_scaled`。
- `getDamageForKill()` = `hpLeft × rate`，**`RoundingMode.UP`**（向上取整到 0 位小数）。
- `calNewCost` 的 `subtractCost` = `preMoney / rate`，**`RoundingMode.DOWN`**。
- `money` 在 `PlayerBox` 里是十进制 `String`（对齐 `Player.java:146 @Column private String money`），落库 `NUMERIC(40,0)`（迁移 `0007_player_money.sql`）。

## A25 `trade.rs` + `install_trade.rs` + `csv_trade.rs`（通商 19002 / 19003 / 19004 / 19005）

> 基准：`data_d/game_7010544/sources/bf/game/server/message/handler/TradeMessageHandler.java`、
> `.../module/trade/Trader.java`（424 行）、`.../TraderSet.java`（143 行）、
> `.../TradeServerModule.java`（87 行）、
> `data_d/game_7010544/sources/bf/game/server/utils/ResourcesConsumeUtils.java`、
> `.../ResourcesUtils.java`、`.../DropUtils.java`、
> `data_d/libs/jb3.common.v1/sources/bf/template/db/ForeignTraderDB.java`、
> `.../cache/ForeignTraderDBCache.java`。

### 四条消息的入口链

| cmd | 方法 | 前置（按执行顺序） | 失败出口 |
|---|---|---|---|
| 19004 | `getTraders(player)` | **无** | 无 |
| 19002 | `tradeByItem(player, req)` | `trader == null` → `2201`；`num <= 0` → `2208`；`num > ItemDB(37).useLimit` → `2`；酒/银两不足 → `2208` | `ErrorResp_1` |
| 19003 | `trade(player, req)` | `all > 0 && !isOpen(106) && 商人数 < 3` → `2202`（**在取 trader 之前**）；`trader == null` → `2201`；`leftTimes <= 0` → `2205`；`nextTradeTime >= now` → `2206`；银两不足 → `2207` | `ErrorResp_1` |
| 19005 | `tradeAll(player)` | 遍历 `1201..=maxTraderId` 全部返回 null → `2210` | `ErrorResp_1` |

### 每条消息都先 `refresh()`

```java
public TraderSet getTraderSet(String playerId) {
    TraderSet traderSet = getAndLoadPetSet(playerId);
    traderSet.refresh();      // ⚠ 无条件刷，每条消息进来都刷
    return traderSet;
}
```

`refresh()` 的判据是 `lastRefreshTime < zeroTime`（今天零点），跨天则重置
`leftTimes` / `money`，并把 `nextTradeTime` 设为**当天零点**（不是 now）。

### 照抄的行为（原版即如此，不是移植缺陷）

1. **两处判 CD 的运算符不一致**
   - `trade(int all)`（19003）：`nextTradeTime >= now` → `2206`（等于当前时刻也拒）
   - `trade(boolean checkCnd, ...)`（19005）：`nextTradeTime > nowTime` → 返回 null（`>`）
2. **`trade(boolean)` 的 `leftTimes--` / `nextTradeTime` 只在 `checkCnd` 真时执行**
   ```java
   if (newMoney >= totalCost) {
       this.money = "0";
       if (checkCnd) { this.leftTimes--; this.nextTradeTime = nowTime + TraderTime*MAX_INDEX; }
       fullReward = true;
   }
   ```
   19005 传 `checkCnd = true`，所以正常路径无差异；`autoPlay` 传 `false` 时次数不动。
3. **`refresh()` 把 `nextTradeTime` 设为今天零点而非 now**
   ```java
   this.nextTradeTime = zeroTime;
   ```
   ⇒ 跨天后第一次通商的时间点被回拨到零点。
4. **`mergeResourcesJson(List)` 把银两输出两次**
   ```java
   for (Long num : numMap.values()) { ... }        // 先数值版
   for (String num : strMap.values()) { ... }      // 再字符串版（仅 MONEY）
   ```
   ⇒ `19002`/`19003`/`19005` 的 `cost`/`reward` 里 id=2 会出现两条。
5. **`toMsgTraderData(Integer... fieldType)` 非空时返回空 builder**
   本项目全传 `new Integer[0]`，永远走填充分支，与线上一致。
6. **19003 的 `2202` 判定是三重与，且在取 trader 之前**
   ```java
   if (req.getAll() > 0 && !SystemOpenUtils.isOpen(playerId, 106)
           && traderSet.getMsgPetData().size() < 3) throw TRADE_ERROR_TRADE_ALL_LIMIT;
   ```
   ⇒ 「一键通商未开放」的报错在「商人不存在」之前。
7. **`trade(int)` 末尾 `dropReward.add(reward)` 是死代码**
   该 `List` 之后既不入包也不返回；Rust 侧直接省略。
8. **19002 回包 `cost` 只放银两**
   `costList` 里有葡萄美酒 `(37, itemNum)`，但回包只 `addCost((2, needMoney))`。
9. **`tradeByItem` 的 `needTimes` 用整数除法**
   `totalCost.divide(everyCost).intValue() * itemNum`，不是 `(totalCost*itemNum)/everyCost`。
10. **`TraderSet.addTrader` 的入库与推送写在 for 循环体内**
    ```java
    for (Integer traderId : traderIds) {
        ...
        DBTraderModule.getInstance().add(arrayList);           // ⚠ 循环内
        if (notify) traderDataUpdateResp(playerId, trader, new Integer[0]); else continue;
    }
    ```
    `addPet` 的同两行在循环外。本项目无 DB 层，「每加一个推一条」由
    `grant_traders` 返回 `Vec` 逐个推送复刻。
11. **19005 的 `reward` 分两次追加**
    ```java
    builder.addAllReward(toMsgAwardBuilder(mergeResourcesJson(rewards)));
    builder.addAllReward(toMsgAwardBuilder(mergeResourcesJson(rewardsMultiple)));
    ```
    第二次可以是空列表，但仍是一次独立的 `addAllReward`。

### 本轮未接的部分（过渡实现）

| 原版 | 依赖 | 当前处理 |
|---|---|---|
| **商人发放**（`ResourcesUtils.addResourcesJsons` 的 `TypeResources.TRADER`(5) 分支） | 通用发奖链路 | **未实现** ⇒ 新号 `traders` 为空，19002/19003 走 `2201`、19005 走 `2210`。与原版「新号 `trader` 表无行」等价。`grant_traders` 已按原版语义写好待接 |
| `PetSet.useMiracle(ID_TRADE_REWARD)` | 门客奇迹 | 恒 null ⇒ `miracleReward` / `multipleRewardList` 不含奇迹加成 |
| `PlayerStatServerModule.addLongStat`（`TradeTimes` / `TradeCompleteTimes`） | 统计模块 | 跳过 |
| `EventModule.notifyEventMonitorAsynchronous` / `EventLogSendRecord.businessman` | 事件/日志 | 跳过 |
| `trader` 表落库（`DBTraderModule`） | PG 仓储 | 过渡期 `PlayerBox.traders`（内存），T9 迁 PG |
| `TraderSet.autoPlay` / `Trader.autoPlay`（`IAutoPlay` 离线播放） | 离线播放框架 | 不接 |
| `Trader.addTimesByItem()`（道具恢复次数） | — | **`data_d` 里零调用点**（只有定义），不接 |
| `TradeServerModule` 的 `HashCacheLruByTime` 缓存层 | 缓存框架 | 用 `MemWorld` 内存态替代，`refresh()` 在每条消息入口手动调 |

### 数据事实

| 表 | 路径 | 行数 | 备注 |
|---|---|---|---|
| 商人 | `template/csv/T-通商-ForeignTrader.csv` | **10**（id `1201..=1210`） | 列 19 个；`dropId` 是 `list;` 型（实测单值 51…60） |
| 变量 | `template/csv/B-变量表-Config.csv` | — | `TraderTime`（id=111）= **3600** 秒 |
| 道具 | `template/csv/D-道具表-Item.csv` | 2674 | `37` = 葡萄美酒，`type=2`，`useLimit=100`；`1201..1210` 是 `type=5`(TRADER) |

```
1201 unlock=6   tradeNum=3 totalCost=25000000      everyCost=500000     everyReward=30000  dropId=51
1202 unlock=18  tradeNum=3 totalCost=500000000     everyCost=10000000   everyReward=60000  dropId=52
1203 unlock=35  tradeNum=3 totalCost=2500000000    everyCost=50000000   everyReward=100000 dropId=53
1204 unlock=55  tradeNum=3 totalCost=7500000000    everyCost=150000000  everyReward=150000 dropId=54
1205 unlock=75  tradeNum=3 totalCost=25000000000   everyCost=500000000  everyReward=200000 dropId=55
1206 unlock=100 tradeNum=3 totalCost=100000000000  everyCost=2000000000 everyReward=250000 dropId=56
1207 unlock=125 tradeNum=3 totalCost=250000000000  everyCost=5000000000 everyReward=300000 dropId=57
1208 unlock=150 tradeNum=3 totalCost=500000000000  everyCost=10000000000 everyReward=400000 dropId=58
1209 unlock=175 tradeNum=3 totalCost=1500000000000 everyCost=30000000000 everyReward=500000 dropId=59
1210 unlock=200 tradeNum=3 totalCost=4000000000000 everyCost=80000000000 everyReward=600000 dropId=60
```

### 常量

- `Coliseum.MAX_INDEX = 1000`（`Coliseum.java:37`）。
- 一次通商 CD = `ConfigDBCache.TraderTime * Coliseum.MAX_INDEX = 3600 * 1000 = 3_600_000` ms
  （`LogicUtils.getNowTime()` 是毫秒，乘完不再 ×1000）。
- 系统开放 id `106` = 通商-一键通商（`G-功能开启表-SystemOpen.csv`）。
- 掉落组 51–60 的二级 id：`5101` / `5201;5202` / `5301` / … / `6001;6002`；
  只有 53 有 `givenDrop=1=5301`。

### 大数语义

`totalCost` 最大 `4000000000000`（4e12），`needMoney = totalCost * itemNum`（itemNum ≤ 100）
最大 `4e14`，仍在 `i64` 内；但为了与 `TraderRow` 的字符串字段保持一致、避免将来调大数值时溢出，
`trade.rs` 内部一律用 `num-bigint::BigInt`（`parse_big` / `div_int`）。

## A1' 新发现：模板默认值与原版偏差（RawTable 接真数据时暴露）

| 项 | 原版（真 CSV 实测） | 当前 Rust 过渡值 | 影响 |
|---|---|---|---|
| `ItemDB.useLimit` | 2674 行道具中仅 **1178 行**填了值，其余为空 | `ASSUMED_USE_LIMIT = 999` | 空值 ⇒ `getUseLimit()==0` ⇒ 任何 `num>=1` 都触发 `num > useLimit` → throw BUSY。即**这类道具原版不可使用**，而当前实现会放行 |
| `GiftDB.useCost` 列名 | CSV 列名是 **`useCost`**（Java 字段 `useCostJson`） | 未接 | 接表时别按 `useCostJson` 找列 |
| `GiftDB` 有消耗的行 | 1066 行中仅 **10 行**填了 `useCost` | 未接 | 绝大多数礼包无消耗，`cost` 分支很少触发 |

结论：`ASSUMED_USE_LIMIT = 999` 是**已知偏差**，接 `RawTable` + `ItemLookup` 时改为按 CSV 取值，空值即 0。
