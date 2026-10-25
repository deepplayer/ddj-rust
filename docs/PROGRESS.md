# 移植进度（切换上下文先读这里）

> 最后更新：2026-10-25 21:35
> 基准：`../data_d/game_7010544/sources/`（反编译）。规则见 [`../AGENTS.md`](../AGENTS.md)。
> 逐分支台账见 [`AUDIT.md`](AUDIT.md)；里程碑见 [`PLAN.md`](PLAN.md)。

---

## 一、一句话现状

骨架（协议/传输/配置/DB/装配/单二进制）已完成且可跑；**业务层 1681 条消息只做了 38 条（2.3%）**，
其中真正落库的只有 4 条，其余是进程内 `MemWorld`（重启即丢）。

```
cargo check  --workspace --all-targets                      0 error / 0 warning
cargo clippy --workspace --exclude ddj-pb --lib --bins -D warnings   通过
cargo test   --workspace                                    357 passed / 0 failed
cargo xt check-arch                                         4 条规则通过
```

代码量：27 crate / 174 个 `.rs` / 23 487 行。

## 二、已接线消息（38 / 1681）

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

## 三、未接线（占位，`ret=501`）

`account` `activity` `battle` `chat` `friend` `gm` `guild` `player` `rank` `shop` `task`
—— 11 个模块，各 27 行占位。

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
| 邮件模板 | `MailTemplateDB` | ⬜ 未接 |
| 等级表 | `S-身份表-Status.csv` | ⬜ 未接 |
| 名字表 | `W_玩家名字_PlayerName.csv`（306000 名） | ⬜ 未接 |

## 五、未决问题（需要决策）

### 5.1 三处偏离原版，待裁决

| 位置 | 原版 | 当前 | 何时分叉 |
|---|---|---|---|
| `item.rs` `MAX_COMPOUND_DEPTH=64` | 死循环 | 回 `PARAM_ERROR` | 畸形表数据（`compoundItem` 成环） |
| `sign.rs` `rand_pick` | 抛 `IndexOutOfBounds` | 提前 `break` | `randTimes > 奖励条数` |
| `item.rs` 各处 `saturating_*` | `long` 回绕成负数 | 饱和到 `i64::MAX` | 极端数值 |

这三处是「原版靠 JVM/外层 catch 兜住，Rust 会挂死」的情形，按 `AGENTS.md` §3 需明确裁决：
**照抄**（那就得自己模拟 panic / 回绕）还是**保留护栏**。

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
2. **补完 `PlayerMessageHandler` 剩余消息**（27 条中已做 6 条）—— 单一文件、逻辑集中、收益快。
3. **`FamilyMessageHandler`(33) / `TalentMessageHandler`(30) / `PetMessageHandler`** —— 完成 M3 的一角。
4. **活动族试点 1 个**（如 `FeastMessageHandler` 49 条）—— 验证活动族的通用装配模式，再铺开。

## 七、常用命令

```bash
# 本地（内存有限，别跑 release）
DDJ_PB_STUB=1 cargo check --workspace
cargo test -p ddj-game-handler
cargo xt check-arch

# 磁盘紧时
rm -rf target/debug/incremental && CARGO_INCREMENTAL=0 cargo test -p ddj-game-handler

# 冒烟（需先起服务）
node e2e/smoke/handlers_smoke.mjs ws://127.0.0.1:10544/
node e2e/smoke/view_smoke.mjs
```

## 八、最近提交

```
47c33d7 fix(item,amber): 修 HEAD 编译 + 6 处与原版反编译不一致的行为
bc29929 feat(log): 1016 福利领奖 + 1101/1102 纯日志三条（成功后不回包）
130e9e0 test(e2e): 1008/1020 看他人专项（真角色 ret=0 / 不存在 ret=2）
26446b8 feat(view): PgViewRepo 接 PG + 1008/1020 入表
093e4c4 test(e2e): 琥珀专项冒烟（真奖励 219×1 + 4702/4704/4705 分支）
```
