# 移植进度（切换上下文先读这里）

> 最后更新：2026-10-27 20:40
> 基准：`../data_d/game_7010544/sources/`（反编译）。规则见 [`../AGENTS.md`](../AGENTS.md)。
> 逐分支台账见 [`AUDIT.md`](AUDIT.md)；里程碑见 [`PLAN.md`](PLAN.md)。

---

## 一、一句话现状

骨架（协议/传输/配置/DB/装配/单二进制）已完成且可跑；**业务层 1681 条消息做了 60 条（3.6%）**，
其中真正落库的只有 4 条，其余是进程内 `MemWorld`（重启即丢）。

```
cargo check  --workspace --all-targets                      0 error / 0 warning
cargo clippy --workspace --exclude ddj-pb --lib --bins -D warnings   通过
cargo test   --workspace                                    525 passed / 0 failed
cargo xt check-arch                                         4 条规则通过
```

代码量：27 crate / 约 180 个 `.rs` / 约 25 000 行。

## 二、已接线消息（60 / 1681）

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

## 三、未接线（占位，`ret=501`）

`account` `activity` `battle` `chat` `friend` `gm` `guild` `player` `rank` `shop` `task`
—— 11 个模块，各 27 行占位。

`PlayerMessageHandler`（27 条）当前进度 **14/27**，剩余 13 条：

已接：1002 / 1003 / 1004 / 1008 / 1009 / 1010 / 1012 / 1015 / 1016 / 1018 / 1019 /
1020 / 1022 / 1024 / 1100 / 1101 / 1102 / 1104 / 1107 / 1110 / 1115

未接（依赖外部模块或平台侧）：

| cmdId | 方法 | 缺什么 |
|---|---|---|
| 1112 等 | — | 已由 `PlayerStateStore` 覆盖（1100/1103/1111/1112） |
| 1151 | `PhoneBindStateSyncRemoteReq` | 平台侧手机绑定状态 |
| 其余 | `EngineMessageHandler` 的 16/17/18 已在 `handshake.rs` | — |

> `1100/1103/1111/1112` 走 `PlayerStateStore`，`1008/1020` 走 PG，
> 其余走 `MemWorld`。

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

## 五、未决问题（需要决策）

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
| commit | `10def38`（feat(pet): 接升星 7003 / 技能升阶 7006） |
| Actions run | `40504677931`，check 2m17s / release 2m15s |
| sha256 | `656c526e72c616ef57d58eeb01649c05a9e5e9fb1c72ce191b05a6c452758aae` |
| 大小 | 16 329 424 字节 |
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
node e2e/smoke/view_smoke.mjs
```

## 八、最近提交

```
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
