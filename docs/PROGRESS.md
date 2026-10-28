# 移植进度（切换上下文先读这里）

> 最后更新：2026-10-29 04:10
> 基准：`../data_d/game_7010544/sources/`（反编译）。规则见 [`../AGENTS.md`](../AGENTS.md)。
> 逐分支台账见 [`AUDIT.md`](AUDIT.md)；里程碑见 [`PLAN.md`](PLAN.md)。

---

## 一、一句话现状

骨架（协议/传输/配置/DB/装配/单二进制）已完成且可跑；**业务层 1681 条消息做了 96 条（5.7%）**，
其中真正落库的只有 4 条，其余是进程内 `MemWorld`（重启即丢）。

最近两轮补的底座（不增加消息数但解锁后续一大批）：
- `SystemOpenUtils.isOpen` —— `data_d` 里 60 个文件调它
- `ActivityDbTemplate` 时间判定 —— 50+ 个活动处理器的入口条件
- `TravelEvents` 权重抽取 + 四类事件结算（15002 / 15004 / 15009）
- `S-身份表-Status` 真实等级表 —— 1003 升级从此可走通

```
cargo check  --workspace --all-targets                      0 error / 0 warning
cargo clippy --workspace --exclude ddj-pb --lib --bins -D warnings   通过
cargo test   --workspace                                    862 passed / 0 failed
cargo xt check-arch                                         4 条规则通过
```

代码量：27 crate / 约 180 个 `.rs` / 约 25 000 行。

## 二、已接线消息（96 / 1681）

| 模块 | cmdId | 数据源 | 备注 |
|---|---|---|---|
| 登录 | 4 | **PG** | XXTEA + Base64；`WorkTaskPlayerSignIn` 分支已审 |
| 创角 | 6 | **PG** | `CreatePlayerReq_6` |
| 握手 | 16 / 17 / 18 | 内存 | `HandshakeStore`（挑战密码校验） |
| 引导 | 1103 | `PlayerStateStore` | |
| 缓存 | 1111 / 1112 | `PlayerStateStore` | |
| 看他人 | 1008 / 1020 | **PG** | 跨服分支回 `BUSY(2)`（`ddj-rpc` 未铺） |
| 道具 | 11002 / 11003 / 11005 / 11006 / 11008 | `MemWorld` + 真实 CSV | 见 §四 |
| 角色属性 | 1002 / 1003 | `MemWorld` | 名字表 / 等级表未接 |
| 角色周边 | 1009 / 1010 / 1015 / 1024 | `MemWorld` | |
| 日志 | 1016 / 1101 / 1102 | `MemWorld` | 成功后不回包 |
| 邮件 | 10001 / 10002 / 10003 / 10004 | `MemWorld` | 模板表未接 |
| 签到 | 16002 / 16003 / 16004 | `MemWorld` + 真实 CSV | 16001 推送未发（缺 push） |
| 琥珀 | 17091 / 17092 / 17093 / 17094 / 17095 / 17097 | `MemWorld` + 真实 CSV | 17096 推送未发 |
| 存档 | 1100 | `MemWorld` | 零校验零回包 |
| 宣言 | 1018 / 1019 | `MemWorld` | 1019 本服双发包（原版 bug） |
| 平台信息 | 1022 / 1107 / 1110 | `PlatformSnapshot` | 只读，静态注入 |
| 连接标记 | 1004 | `MemWorld.bor` | 不回包 |
| 绑定奖励 | 1115 | `MemWorld` | 24h 新号加成 |
| VVIP 奖励 | 1012 | `MemWorld` | ⚠ 跨年领不到（原版 bug 照抄） |
| 广告奖励 | 1104 | `MemWorld` | `PLAYER_ERROR_20(120)` 上限 |
| 心跳 | 3 | 无 | 免登录；回 `serverTime` |
| 取随机名 | 5 | 真实 CSV | 免登录；**306000 名字池**已端到端验证 |
| 排队信息 | 7 | 无 | 免登录；空队列 → `nowIndex=1,maxIndex=1` |
| 测试消息 | 11 | 无 | 免登录；**三个包**（11 + 3 + 3）已端到端验证 |
| 门客皮肤 | 7023 / 7024 / 7025 | `MemWorld` | 三条分支序各不相同，已端到端验证错误码 |
| 珍兽评级 | 7026 / 7029 | `MemWorld` | ⚠ **两条错误出口不同**，已端到端验证 |
| 宠物互动 | 7002 / 7030 | `MemWorld` + `counter` | 新增计数器恢复模块；7030 循环次数是 `times × 上限` |
| 升星/升阶 | 7003 / 7006 | `MemWorld` | 7003 用**当前**星级门槛；7006 判**宠物总数** |
| 技能升级 | 7005 | `MemWorld` | ⚠ 皮肤扣道具、普通扣**互动经验** |
| 二级密码 | 17351-17355 | `MemWorld` | ⚠ **17352 唯一失败也走 Resp** |
| 防骗手册 | 17151 / 17152 / 17153 | `MemWorld` + 真实 CSV | ⚠ 17152 越界**不发包**；17151 三种失败码在 Resp 里 |
| 计数器恢复 | 8001 / 8002 | `MemWorld` + 真实 CSV | ⚠ 8002 查不到**不发包**；`time=-1` 表示停表 |
| 客户端特约 | 1109 | 无状态 | 原版是空壳：恒回 `{ret=0, msgId}`，`data` 永不设置 |
| 保存桌面 | 17301 / 17302 / 17303 | `MemWorld` + 真实 CSV | ⚠ 渠道配置实测为空 → `eligible` 恒 false，17302/17303 恒回 4731（原版状态）|
| 渠道好评 | 17101 / 17102 / 17103 | `MemWorld` + 真实 CSV | ⚠ **三条都不走 `ErrorResp_1`**；17102 渠道不符回 `ret=0` |
| 焚香祈愿 | 17071 / 17072 | `MemWorld` + 真实 CSV | ⚠ 单抽首日免费、十连不免费；**先推 17074 再回 17071** |
| 系统开放 | 1015（改造） | **真实 CSV** + PG | 查不到 `systemId` → `ret=2`（原版 NPE） |
| 聚宝盆记录 | 38303 / 38304 | `MemWorld` + PG `bf_activity` | ⚠ 两条都**不检查 `isOpen`**；38304 恒空列表 |
| 游历 | 15002 / 15004 / 15009 | **真实 CSV** + `MemWorld` | 先推 15001 再回包；事件 1/2/4 的实体落地未接 |
| 升级 | 1003（改造） | **真实 CSV** | 原 `MemLevels` 每级 100 经验 → 改读 `S-身份表-Status.csv` |
| 猜灯谜 | 37603 / 37604 / 37605 / 37607 | `MemWorld` + 注入题表 | ⚠ 线上 `bf_activity` 无 `typeId=50` → 四条恒回 2（原版 NPE）|
| 远洋贸易 | 41001 / 41002 / 41003 | **真实 CSV** + `MemWorld` | ⚠ 41002 **不判** `isOpen(19)`；`BigInt` 全链路（末关 cost 4.68e18）|
| 通商 | 19002 / 19003 / 19004 / 19005 | **真实 CSV** + `MemWorld` | ⚠ 商人发放链路未接 → 新号恒 `2201`/`2210`（与原版新号一致）|

### 构建产物（2026-10-28 22:59 实测）

```text
commit  2e6a609
run     40706835559  1 门禁   arch ✓ clippy ✓ rustfmt 红（已知不阻塞）  1m47s
        40706834936  3 test   867 passed / 0 failed                    2m56s
        40706834554  2 build  release ✓                                 4m04s
sha256  b40abbae5f40a944768355b7c098fe625678af4283ffd0862bc593541a063e98
启动    handlers=92
e2e     travel ✅  lantern ✅  pray ✅  anticheat ✅  counter ✅
        shortcut ✅  favourable ✅  system_open ✅  ocean_trade ✅
```

> **CI 已拆分（commit `2e6a609`）**：原 `ci.yml` 一个 workflow 带 `check`/`fmt`/`release`
> 三个 job 且用 `needs:` 串联，现在拆成四个单职责 workflow 并行触发：
> `1-gate.yml`（arch / clippy / fmt）、`2-build.yml`、`3-test.yml`、`4-e2e.yml`（仅手动）。
> 本机不再跑任何 cargo 任务 —— 只写代码、跑 `bin/ddj`、跑 `e2e/smoke/*.mjs`。
> 详见 `AGENTS.md` §5。

> ⚠ **装配事故备忘**：`install_travel` 的装配行曾被 `07519e2` 误删，
> `install_travel.rs` 自身编过、单测全绿，只有 e2e 冒烟报超时才暴露。
> 现已加 `handlers::tests::register_wires_every_ported_module`：
> 把已接线的 24 个 cmdId 钉成断言，漏装配在 `cargo test` 阶段就红。

## 三、未接线（占位，`ret=501`）

`account` `activity` `battle` `chat` `friend` `gm` `guild` `player` `rank` `shop` `task`
—— 11 个模块，各 27 行占位。

### 已就位但还没被用的底座（下一批可以直接接）

| 底座 | 位置 | 解锁什么 |
|---|---|---|
| `SystemOpenUtils.isOpen` | `system_open.rs` / `install_system_open.rs` | 60 个文件的开门判定 |
| `ActivityDbTemplate` 时间判定 | `activity_template.rs` / `install_activity.rs` | 50+ 活动处理器的入口 |
| `DropUtils.rand` 底座 | `drop.rs` | 一切走掉落表的玩法 |
| `CounterRecovery` 计数器 | `counter.rs` | 一切「每日次数」玩法 |
| `LanternRiddles` 灯谜骨架 | `lantern_riddles.rs` | 37606 跨服排名接上后即可完整 |

`PlayerMessageHandler`（27 条）**27/27 已全部接线**：

| 分组 | cmdId | 实现位置 |
|---|---|---|
| 角色属性 | 1002 / 1003 / 1024 | `install_player.rs`（`MemWorld`）|
| 连接标记 | 1004 | `install_bind.rs`（`bor`）|
| 看他人 | 1008 / 1020 | `install_view.rs`（**PG**）|
| 角色周边 | 1009 / 1010 / 1015 | `install_player.rs` / `install_misc.rs` |
| 日志 | 1016 / 1101 / 1102 | `install_log.rs`（成功后不回包）|
| 宣言 | 1018 / 1019 | `install_declare.rs` |
| VVIP / 广告 | 1012 / 1104 / 1115 | `install_vip.rs` / `install_bind.rs` |
| 平台信息 | 1022 / 1107 / 1110 | `install_platform.rs`（只读）|
| 存档回写 | 1100 | `install_state.rs` |
| 引导 | 1103 | `handlers.rs` → `GuideHandler`（`PlayerStateStore`）|
| 缓存 | 1111 / 1112 | `handlers.rs` → `CacheSaveHandler` / `CacheGetHandler` |
| 握手 | 16 / 17 / 18 | `handshake.rs` → `HandshakeHandler`（免登录白名单）|

> `1100/1103/1111/1112` 走 `PlayerStateStore`，`1008/1020` 走 PG，其余走 `MemWorld`。
> 16/17/18 不属于 `PlayerMessageHandler`，是 `EngineMessageHandler` 的三条，
> 由 `handshake.rs` 实现（挑战密码校验）。

活动族最大：`data_d/.../module/activity/` 下 **1512 个 Java 文件**，一行未动。

## 四、道具模块（11002/11003/11005/11006/11008）细节

已按原版逐分支对齐，本轮修了 6 处偏离（commit `47c33d7`）：

1. 11003 成功后补 `addItem` 产出（`ItemSet.java:384`），仅 `type=ITEM(2)` 且 `num>0` 才加。
2. 11002/11003/11005/11006 补 `ItemUpdateResp_11004` / `ItemEffectiveUpdateResp_11007` 推送。
3. 11005/11006 的失败分支改成发 `ItemUseResp_11002`（原版复制粘贴 bug）。
4. 11006 成功时无条件 `setMsgUseResult`。
5. 11006 用掉的条目只把 `num` 扣到 0，**不从 map 删除**；只有过期条目才 `iterator.remove()`。
6. `amber` 17093/17094 补 VIP 门禁；17097 补 `checkHeyTapVip`；`amberVip` 改为状态驱动。

### 模板接入情况

| 数据 | 来源 | 状态 |
|---|---|---|
| `ItemDB.useLimit` / `type` | `D-道具表-Item.csv` | ✅ 真实 CSV |
| `ItemDB.compoundItem` | 同上（`list;` → `12=10`） | ✅ 新增 |
| `GiftDB.useCost` | `D-道具使用效果-Gift.csv`（列名是 `useCost`） | ✅ 真实 CSV |
| 签到奖励 / 宝箱 | `M-每日签到表` / `M-每日签到宝箱表` | ✅ 真实 CSV |
| 琥珀 31 行 | `H-琥珀福利-AmberWelfare.csv` | ✅ 真实 CSV |
| 邮件模板 | `MailTemplateDB` | ⬜ 未接（1115 用 `mail_template_id != 0` 近似） |
| 宣言长度限制 | `ConfigDB.MammonDeclaration*Limit` | ⚠ 常量 1 / 60（`declare.rs`） |
| 绑定奖励 | `ConfigDBCache.TwBindReward` / `HighReward` | ⬜ 未接（`BindRewardTemplates` 注入点已留） |
| 平台配置 | `bf_word_config` / `bf_wechat_official_account` / `switchPayment` | ⬜ 未接（`PlatformSnapshot` 注入点已留） |
| 等级表 | `S-身份表-Status.csv` | ⬜ 未接 |
| 名字表 | `W_玩家名字_PlayerName.csv` | ✅ 真实 CSV，306000 名 × 3 语言 |
| 系统开放表 | `G-功能开启表-SystemOpen.csv`（100 行） | ✅ 真实 CSV |
| 开服时间 | PG `bf_server.open_time` | ✅ 真实 PG |
| 活动模板 | PG `bf_activity`（11 行） | ✅ 真实 PG |
| VIP 等级 | `player.vip`（登录时读到 `PlayerRow.vip`，还没写进 `PlayerBox`） | ⬜ 未接 → `isOpen` 的 `unlockType=3` 对所有人关闭 |

## 五、下一步（按解锁收益排序）

1. **`ActivitySet` 活动实例层** —— 现在只有模板（时间判定），还没有
   「某玩家在某活动下的状态」。有了它，`Cornucopia` / `Wonderland` / `Bridge` /
   `Nianbeast` / `GroupBuy` / `Fishing` 的只读查询（`xxxInfo` / `xxxRecord`）
   可以一次接一大片。
2. **`PlayerBox.vip`** —— 从 `PlayerRow.vip` 写进去，`SystemOpen` 的
   `unlockType=3`（23 行）立刻生效。
3. **`Travel`（15002 / 15004 / 15009）** —— 1384 行模块，但 `place` / `allTimes`
   的开放判定已就位（`isOpen(3)` = 1 级、`isOpen(102)` = 3 级 VIP）。
4. **`OceanTrade`（41001-41003）** —— 1360 行，`isOpen(19)` 已就位（开服 10 天）。

## 六、未决问题（需要决策）

### 5.1 三处偏离原版 —— 已定策略（2026-10-27）

**决策：保留护栏 + 触发时记录，等真实客户端验证后再定是否改成完全照抄。**

| 位置 | 原版 | 当前 | 何时分叉 |
|---|---|---|---|
| `item.rs` `MAX_COMPOUND_DEPTH=64` | 死循环 | 回 `PARAM_ERROR` | 畸形表数据（`compoundItem` 成环） |
| `sign.rs` `rand_pick` | 抛 `IndexOutOfBounds` | 提前 `break` | `randTimes > 奖励条数` |
| `item.rs` `mul_or_record` / `add_or_record` | `long` 回绕成负数 | 饱和到 `i64` 边界 | 极端数值 |

记录机制在 `crates/app/game-handler/src/divergence.rs`：

- 触发 → 计数器 +1 + `WARN target=ddj::divergence`，带 `code` / `original` / `ours` / `detail`
- `ddj status <service>` 单服务查询会打印三行计数（全 0 打「正常」）
- `detail` 够定位到具体 CSV 行：P1 带完整 `chain`，P2 带 `signed_day`，P3 带两个操作数

真实客户端连上后的处置流程见 `docs/AUDIT.md`「待裁决偏离」§ 真实客户端连上后的处置流程。

### 5.2 `MemWorld` 换 `PG`

背包/签到/邮件/琥珀/角色周边/日志 7 组仍在进程内存态。换 PG 需要：
1. 建表（`item` / `mail` / `sign` / `amber` 等，`0002_platform.sql` 里已有平台侧，游戏侧需补）；
2. 实现 `ddj_repo` 的对应 `Repo`；
3. 把 `install_*.rs` 里的 `MemWorld` 换成仓储 trait 对象。

### 5.3 推送通道刚打通，还没接线的地方

- `16004` 的 `MsgSignResp_16001` 推送（原版一条请求发两个包）
- `17095` 的 `AmberVipSyncResp_17096` 推送（原版异步任务里发）
- `17093`/`17094` 的 `EventLogPropertyAmber`（只记日志，无推送）

## 六、下一步建议顺序

1. **裁决 §5.1 的三处偏离** —— 影响后续所有数值逻辑的写法基准。
2. **补完 `PlayerMessageHandler` 剩余 15 条** —— 单一文件、逻辑集中、收益快。
   其中 1012（VIP 奖励）依赖 `VipServerModule`，1104（广告奖励）依赖
   `PlayerAdvertising`，这两块要先建模块才能接。
3. **`FamilyMessageHandler`(33) / `TalentMessageHandler`(30) / `PetMessageHandler`** —— 完成 M3 的一角。
4. **活动族试点 1 个**（如 `FeastMessageHandler` 49 条）—— 验证活动族的通用装配模式，再铺开。

## 六点五、构建与产物（2026-10-27 更新）

**规则：本地不编译，一律 GitHub Actions 编译后取回。**

```
改代码 → commit → push github → Actions 编译
      → gh run download 取回 → 本机 bin/ddj 验证
```

仓库：`https://github.com/deepplayer/ddj-rust.git`（`github` remote 是主；
`gitee` 仅历史镜像，不再推送）。

```bash
git push github main
gh run list --limit 3                     # 等 check + release 绿
gh run download <run-id> -D /tmp/gh-art
cp /tmp/gh-art/ddj-<full-sha>/ddj bin/ddj && chmod +x bin/ddj
sha256sum bin/ddj && ./bin/ddj --version
```

Actions 三个 job：`check`（check-arch + clippy + test，阻塞）、
`fmt`（仅报告，移植期未统一格式化，红是已知状态）、
`release`（`needs: check`，上传 artifact，保留 14 天）。

### 已废弃：web-shell 远端构建

早期用 web-shell 在 `/workspace/ddj-rust` 编译，两个坑：

1. **每次请求可能落到不同容器，但 `/workspace` 是共享卷** ——
   跨节点 `target/` 缓存互相污染，实测 `Exec format error`、
   `target/debug/deps/ddj-*` 变成 `data`、target 涨到 66G。
2. **后台构建活不下来** —— `nohup` / `setsid` 都躲不过节点回收，两次中途消失。

`scripts/remote_release.sh` 保留作参考，不再是主流程。

### 最近一次成功构建

| 项 | 值 |
|---|---|
| commit | `cd05ab3`（fix(net): 推送帧与回包的写出顺序改为「推送在前」）|
| Actions run | `40558075752`，共 3m49s |
| sha256 | `e2258ebc00a45f8836d06c2e5c0b610a0ab4ea89a1e3e28829f47df791cca669` |
| 验证 | `./bin/ddj --version` → `ddj 0.1.0`；`./bin/ddj list` 列出 7 个服务 |

> 同一份源码在 Actions 上产出同一字节（验证过两次：`2766e83` 与 `702ac6f`
> 只差文档，sha256 完全一致）。

## 六点六、分叉记录（真实客户端排障用）

三处「偏离原版」的护栏在触发时都会记录，方便真实客户端连上来后定位问题。

```bash
# 1. 看计数（单服务查询才会打印明细）
./bin/ddj status game
# 输出示例（有触发时）：
# 偏离原版护栏（AGENTS.md §3）：
#   P1 ×2  原版：ItemSet.itemCompound 的 while(true) 无环检测，会永久循环卡住玩家线程
#   定位：日志搜 target=ddj::divergence，详见 docs/AUDIT.md「待裁决偏离」

# 2. 看具体上下文
grep 'ddj::divergence' /tmp/ddj-$(id -u)/game.log | tail
```

`detail` 字段的定位用法：

| 分叉 | detail 带什么 | 怎么查 |
|---|---|---|
| P1 | `compoundId` / `useItemId` / 完整 `chain` 路径 | 拿 chain 去 `D-道具表-Item.csv` 查这几行的 `compoundItem` 列 |
| P2 | `signed_day` / `rand_times` / `pool_len` / `drawn` | 拿 `signed_day` 去 `M-每日签到表-DailySign.csv` 看该行奖励条数 |
| P3 | 两个操作数 `a` / `b` | 判断是表数据异常还是客户端伪造超大数量 |

## 六点七、端到端验证记录（2026-10-27）

首次跑通「真实二进制 + 真实 DB + 真实 CSV」的完整链路。

### 环境

```bash
docker start ddj-pg ddj-redis     # postgres:16-alpine → 127.0.0.1:55432
                                  # redis:7.2 → 127.0.0.1:6379
DDJ_RUN_DIR=/tmp/ddj-run ./bin/ddj run game
```

启动日志关键行：

```
配置表已扫描  tables=445 rows=78215 warnings=0 errors=0
道具模板已装载 item_rows=2674 gift_rows=1066
签到模板已装载 day_rows=21 box_rows=4
琥珀模板已装载 sign_in=7 charges=23 has_exclusive=true
名字池已装载   zh_cn=306000 en=306000 vi_vn=306000
服务就绪      handlers=49 templates=0
WebSocket 监听已启动 addr=0.0.0.0:10544
```

### 实测结果

| 消息 | 请求 | 回包 |
|---|---|---|
| cmd 5 取随机名 | `lan="zh_CN"` | `ret=0`，10 个真实中文名 |
| cmd 3 心跳 | 空 body | `ret=0` |
| cmd 7 排队信息 | 空 body | `ret=0, nowIndex=1, maxIndex=1` |
| cmd 11 测试消息 | 空 body | **3 个包**：`cmd=11` + `cmd=3` ×2 |
| cmd 7023 激活皮肤 | `petId=100, skinId=1` | `ErrorResp_1{msgId=7023, errorCode=2}`（模板未注入→原版 NPE） |
| cmd 7025 卸下皮肤 | `petId=100, skinId=0` | `ErrorResp_1{msgId=7025, errorCode=405}`（门客不存在） |
| cmd 7026 评级升级 | `petId=100` | `ErrorResp_1{msgId=7026, code=3601}`（帧号 **1**） |
| cmd 7029 评级互换 | `petId1=100, petId2=101` | `MsgPetRateLvReplaceResp_7029{ret=3601}`（帧号 **7029**） |
| cmd 7002 宠物互动 | `all=1` | `ErrorResp_1{msgId=7002, code=901}`（未获得宠物） |
| cmd 7030 批量互动 | `num=10` | `ErrorResp_1{msgId=7030, code=901}`（未获得宠物） |
| cmd 7003 升星 | `petId=100` | `ErrorResp_1{msgId=7003, code=901}` |
| cmd 7006 技能升阶 | `petId=100, skillId=1001` | `ErrorResp_1{msgId=7006, code=901}` |
| cmd 7005 技能升级 | `petId=100, skillId=1001` | `ErrorResp_1{msgId=7005, code=901}` |

### 二级密码专项（5 条全链路）

| cmd | 请求 | 帧号 | 回包 |
|---|---|---|---|
| 17355 | 空 | 17355 | `ret=0` + 初始状态 |
| 17351 | 短密码 `123` | **1** | `ErrorResp_1{code=9501}` |
| 17351 | `123456` | 17351 | `ret=0, haveSetPsw=true` |
| 17352 | 错密码 | **17352** | `ret=9505` ← **唯一失败走 Resp** |
| 17352 | 正确密码 | 17352 | `ret=0` |
| 17353 | `enabled=0` | 17353 | `ret=0, enabled=false` |
| 17354 | `type=1` | 17354 | `ret=0, type=1` |
| 17355 | 空 | 17355 | `ret=0` + 更新后状态 |

### 防骗手册专项（3 条全链路，2026-10-27）

真实配置 `B-变量表-Config.csv:568` 给了 4 组奖励，所以 `index 0..3` 合法。

| cmd | 请求 | 帧号 | 回包 |
|---|---|---|---|
| 17151 | `index=4`（越界） | **17151** | `ret=29`（PARAM_ERROR） |
| 17151 | `index=0`（未阅读） | **17151** | `ret=6451`（ANTICHEAT_READ） |
| 17152 | `index=4`（越界） | — | **无回包**（原版 `return`） |
| 17152 | `index=0` | 17152 | `ret=0` |
| 17151 | `index=0`（已阅读） | 17151 | `ret=0` + 发奖 |
| 17151 | `index=0`（再领） | 17151 | `ret=1501`（REPEATED） |
| 17153 | 空 | 17153 | `info{reward=[0], read=[0]}` |

### 计数器恢复专项（2 条全链路，2026-10-27）

启动日志新增两行：

```text
防骗手册模板已装载  groups=4
计数器模板已装载    counter_rows=92 vip_levels=18
服务就绪            handlers=71
```

| cmd | 请求 | 回包 |
|---|---|---|
| 8001 | 空 | 91 条（92 模板 - 跳过 `templateId=5` 子嗣） |
| 8001 | — | 2 号 `num=3`（VIP0 上限）`cdTime=-1`（满值停表） |
| 8001 | — | 不含 5 号 |
| 8002 | `templateId=2` | `ret=0`，单条 `templateId=2 num=3 cdTime=-1` |
| 8002 | `templateId=999999` | **无回包**（原版没有 else） |

### 保存桌面专项（3 条，2026-10-28）

创角时带 `channelId=2` / `cid="testcid"`（`CreatePlayerReq_6` 字段 11 / 12），
登录后读回，实测结果：

| cmd | 请求 | 帧号 | 回包 |
|---|---|---|---|
| 17301 | 空 | 17301 | `ret=0` + `shortcutData{channelId=2, cid="testcid", state=0, eligible=false}` |
| 17302 | 空 | **1** | `ErrorResp_1{msgId=17302, code=4731}`（渠道不符） |
| 17303 | 空 | **1** | `ErrorResp_1{msgId=17303, code=4731}`（渠道不符，**不是 4732**） |
| 1109 | `msgId="1", data='{"a":1}'` | 1109 | `{ret=0, msgId="1"}`，**`data` 未设置** |

`channelId=2` / `cid="testcid"` 能原样回显，说明
创角落库 → 登录读回 → `MemWorld` 回填这条链是通的。

`eligible` 恒 false 的原因：`bf_config.configId=59`（保存桌面的渠道白名单）实测是**空串**
（`data/sql/platform.sql:562`），所以 `shortcutConfigMap` 为空。
这是原版线上数据的真实状态，不是移植 bug。等运营配了渠道才有意义。

### 渠道好评专项（3 条，2026-10-28）

启动日志新增一行 `好评奖励已装载 count=4`，`handlers=78`。

| cmd | 请求 | 帧号 | 回包 |
|---|---|---|---|
| 17101 | 空 | 17101 | `ret=0, lastCloseTime=0, 列表空, 无 URL` |
| 17102 | `id=1` | **17102** | `ret=0`（渠道不符直接返回，**不是错误码**） |
| 17103 | `id=1, star=5, evaluation="good"` | **17103** | `ret=3941`（渠道没开放好评系统） |

三条都**不走 `ErrorResp_1`** —— 原版三个方法的签名都没有 `throws HandlerException`。
这是与 17301-17303（走 `ErrorResp_1`）最大的区别。

17102 在渠道不符时回 `ret=0` 而不是错误码，客户端会当成"没发生过"。

### 焚香祈愿专项（2 条，2026-10-28）

启动日志新增：

```text
掉落表已装载  drop_one=407 drop_two=762 drop_sign=73
祈愿表已装载  pray_rows=1
服务就绪      handlers=80
```

| cmd | 请求 | 帧序 | 回包 |
|---|---|---|---|
| 17072 | 空 | — | 公告栏空 |
| 17071 | `times=1`（首次免费） | **17074 → 17071** | 先推 `PrayData{freeTime=1793116800000, luckNum=1, cycle=0}`，再回 `ret=0` |
| 17071 | `times=10` | — | 帧号 1，`ErrorResp_1{code=3902}`（十连无免费，新号无 263）|
| 17071 | `times=1`（同一天第二次） | — | 帧号 1，`ErrorResp_1{code=3902}` |

### ⚠ 顺带修了框架 bug：推送与回包的写出顺序

原实现里推送帧走 `mpsc`、回包走 socket 直写，两条通道独立，
`select!` 拿到回包后径直写 → 客户端看到「先回包、后推送」，与原版相反。

实测帧序（修前）：`[4, 6, 4, 17071, 17074]`
实测帧序（修后）：`[4, 6, 4, 17074, 17071]` ✅

修法：写回包前先 `try_recv` 清空**已入队**的推送帧。
影响所有「handler 里既推又回」的消息（17071、16004 等）。commit `cd05ab3`。

### 本轮打通的数据链：`channelId` / `cid`

这两列之前完全没落库，导致所有按渠道判定的消息都没法实现。现在全线打通：

```text
CreatePlayerReq_6.channelId(11) / .cid(12)
  → NewPlayerRow → player 表（0006 迁移）
  → PlayerRow → Account → LoginOutcome::Accepted
  → MemWorld.PlayerBox.channel_id / .cid
  → 17301/17302/17303 的 is_eligible
```

**已端到端验证**（2026-10-28）：创角写 `channelId=2` / `cid="testcid"`，
登录后 17301 回包里这两个值原样出现。

`LoginHandler` 之前拿不到 `MemWorld`，本轮加 `with_world`。
**后续 17101-17103（渠道好评）、17081（渠道卡池）、15002（游历）都靠这条链。**

> 探针坑 1：proto2 的 `repeated int32` 默认 **不打包**（`packed="false"`），
> 每个元素独立带 tag。按 packed 解析会读到空列表（`AntiCheatInfo` 踩过）。
> 探针坑 2：protobuf 的 `int64` 负数（如 `cdTime=-1`）是 10 字节 varint，
> JS 的 32 位位移会溢出成 `4294967295`，必须用 `BigInt` 读。

cmd 5 实收：

```json
["郭嘉德","权浩言","巫马玉宇","利冰凡","鲜于乐章",
 "梅玉轩","钟离英喆","盛宜民","吕宜民","封宏放"]
```

全部来自 306000 池子（250 姓氏 × 1224 名字），与 `e2e/jsclient/DIFF.md`
记录的「实测 zh_CN 可用名 306000 个」吻合。

### 分叉计数器

```
$ ./bin/ddj status game
game  771651  15s  0  49  0  运行中

偏离原版护栏：全部未触发（正常）
```

### 注意

- `templates=0` 是 `TemplateStore` 的计数（只扫描未装载），
  各模块的 CSV 是独立装载的，看上面那几行日志为准。
- game 服**必须**有 PG + Redis 才能起来；缺任一个会卡在 PreInit 阶段
  （不报错、不退出，表现为「只有一条『服务启动中』日志然后静默」）。

## 七、常用命令

```bash
# 本机只做静态校验（不编译 release）
DDJ_PB_STUB=1 cargo check --workspace
cargo test -p ddj-game-handler
cargo xt check-arch

# 构建与取回
git push github main && gh run list --limit 3
gh run download <run-id> -D /tmp/gh-art

# 磁盘紧时
rm -rf target/debug/incremental && CARGO_INCREMENTAL=0 cargo test -p ddj-game-handler

# 冒烟（需先起服务）
node e2e/smoke/handlers_smoke.mjs ws://127.0.0.1:10544/
node e2e/smoke/anticheat_smoke.mjs          # 防骗手册 7 条断言
node e2e/smoke/counter_smoke.mjs            # 计数器恢复 6 条断言
node e2e/smoke/shortcut_smoke.mjs           # 保存桌面 + 1109，4 条断言
node e2e/smoke/favourable_smoke.mjs         # 渠道好评 3 条断言
node e2e/smoke/pray_smoke.mjs               # 焚香祈愿 4 条断言
node e2e/smoke/view_smoke.mjs
```

## 八、最近提交

```
cd05ab3 fix(net): 推送帧与回包的写出顺序改为「推送在前」
e07f733 feat(drop,pray): 接掉落底座 + 焚香祈愿 17071/17072
7ecd12c feat(favourable): 接渠道好评 17101/17102/17103
94f3f65 docs: 补 17301-17303/1109 端到端实测 + 保存桌面冒烟脚本
fd23e8f feat(shortcut): 接保存桌面 17301-17303 + 打通 channelId/cid 数据链
95881e5 feat(special): 接 1109 客户端特约消息（原版是空壳，照抄）
4b51414 feat(counter): 接计数器恢复 8001/8002 + 按反编译逐方法重写 counter.rs
5f3236f feat(anticheat): 接真实配置 AntiCheatReward + 全链路冒烟
1149ed7 feat(anticheat): 接防骗手册 17151 领奖 / 17152 已读 / 17153 查询
99f4f1c feat(psw): 接二级密码 17351-17355 五条
e526e0d feat(pet): 接技能升级 7005（皮肤 / 普通两条分支）
10def38 feat(pet): 接升星 7003 / 技能升阶 7006
4fb7070 feat(pet): 接宠物互动 7002 / 7030，新增计数器恢复模块
c9b14ae feat(pet): 接珍兽评级 7026 升级 / 7029 互换
ad22e5d feat(pet): 接门客皮肤 7023 激活 / 7024 升级 / 7025 更换
019ea79 feat(engine): 接 7 排队信息 / 11 测试消息
0a2705e feat(engine): 接 3 心跳 / 5 取随机名（含 306000 名字池）
d7fbafa fix(test): 分叉计数器断言改为「有增长」，修并行竞态
2d43c57 feat(vip): 接 1012 VVIP 微信奖励 / 1104 广告奖励
e9ce6ae feat(divergence): 三处偏离原版护栏加可定位记录
eb4bfb4 feat(bind): 接 1004 连接标记 / 1115 绑定奖励
a2e5a13 feat(platform): 接 1022 客服文案 / 1107 公众号信息 / 1110 切换支付
264a562 feat(declare): 接 1018 设置宣言 / 1019 查他人宣言
d0d25b0 docs(progress): 记录 run 40480638459 与构建可复现性
702ac6f docs: 构建流程改为 GitHub Actions（废弃 web-shell 远端编译）
47c33d7 fix(item,amber): 修 HEAD 编译 + 6 处与原版反编译不一致的行为
bc29929 feat(log): 1016 福利领奖 + 1101/1102 纯日志三条（成功后不回包）
130e9e0 test(e2e): 1008/1020 看他人专项（真角色 ret=0 / 不存在 ret=2）
26446b8 feat(view): PgViewRepo 接 PG + 1008/1020 入表
093e4c4 test(e2e): 琥珀专项冒烟（真奖励 219×1 + 4702/4704/4705 分支）
```
