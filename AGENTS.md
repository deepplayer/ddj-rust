# AGENTS.md — 给协作者/后续会话的操作约束

> 本文件是**硬约束**，不是建议。每次接手这个仓库先读这里，再读
> [`docs/PROGRESS.md`](docs/PROGRESS.md)（当前进度 / 下一步）。

---

## 1. 移植来源与唯一基准

| 层 | 路径 | 地位 |
|---|---|---|
| 原始服务端构建产物 | `../data/` | **只读参考**，不修改 |
| 反编译 Java 源码 | `../data_d/<service>/sources/**/*.java` | **移植的唯一基准** |
| Rust 实现 | `crates/**/*.rs`（本仓库） | 待移植/已移植的代码 |

**规则：Rust 代码必须能一一对应回 `data_d/` 的具体类与方法。**

- 每个函数/模块顶部注释必须写明对应的 Java 全限定类名 + 方法名。
- 分支顺序不能调换（很多分支有隐式依赖，比如 `sign.boxReward` 必须先判天数再判重复）。
- 数值语义按 Java 走（`long` / `int` 的截断、`(int)` 强转、`==` 对 `Integer` 的拆箱）。

## 2. 原版 bug 必须照抄，只允许标注

**不修 bug。** 原版行为是什么，Rust 就实现什么。

可以做的只有两件事：

1. **注释标注**：在代码处写清「原版这里会 NPE / 死循环 / 错误码用错消息类型」，并给出
   `data_d` 的文件名 + 行号。
2. **记入台账**：在 [`docs/AUDIT.md`](docs/AUDIT.md) 的对应表格里登记一行。

不允许做的：

- 改成「更合理」的行为。
- 加校验把原版的异常路径变成正常返回。
- 把原版的错误码「修正」成看起来正确的那个。
- 给 `RetCode` 补原版没有的枚举值。

### 已经确认的「原版 bug」，必须保持原样

| 位置 | 原版行为 | 为什么不能修 |
|---|---|---|
| `ItemMessageHandler.itemUseReq` 外层 catch | 无条件发 `ItemUseResp_11002{ret=2}`，把 `USE_COST_RESOURCES_NO(26)` 遮罩成 2 | 玩家看到的就是 2，改成 26 会与线上不符 |
| `ItemMessageHandler.itemEffectiveOneKeyUseReq` catch | 同上，且**消息类型是 11002 而不是 11006** | 复制粘贴 bug，客户端按 11002 解析 |
| `ItemSet.itemCompound` 链式追溯 | 无环检测，`compoundItem` 成环时死循环 | 见 §3 例外条款 |
| `ItemSet.itemEffectiveOneKeyUse` | 循环外只校验**单份** `useCost`，不是累计值 | 逻辑缺陷，但影响放行条件 |
| `GiftDBUtils.checkForUsing(null, player)` | 首行 `giftDB.getUseLimit()` → NPE → ret=2 | 1608 条道具原版就是「不能用」 |
| 1101/1102 的 catch | 回的是 `PlayerUseGiftKeyResp_1009`，不是自己的 Resp | 复制粘贴 bug |

## 3. 唯一的例外：会让进程不可用的东西

只有一种情况允许偏离原版：**原版行为会导致 Rust 进程挂死/panic/无限占用**，
而 Java 那边是靠别的机制兜住的（例如 JVM 线程池超时、外层 catch）。

必须在三处同时标注：
1. 代码注释写 `// ⚠ 偏离原版（原因）`；
2. `docs/AUDIT.md` 的「已知架构性差异」表登记；
3. commit message 里单列一段。

当前已知 3 处，均待裁决：

| 位置 | 原版 | 当前 Rust | 影响面 |
|---|---|---|---|
| `item.rs` `MAX_COMPOUND_DEPTH = 64` | 死循环 | 达到上限回 `PARAM_ERROR` | 只在畸形表数据下分叉 |
| `sign.rs` `rand_pick` | 池空抛 `IndexOutOfBoundsException` | 提前 `break` | 只在 `randTimes > 奖励数` 时分叉 |
| `item.rs` 各处 `saturating_*` | `long` 乘法回绕（负数） | 饱和到 `i64::MAX` | 只在极端数值下分叉 |

## 4. 每完成一个目标就提交

- **一个目标一个 commit**，不要攒。目标 = 一条消息接线 / 一次审计 / 一个 bug 修复。
- commit message 格式：

```text
<type>(<scope>): <一句话做了什么>

<为什么：原版依据，给出 data_d 的文件名:行号>

<验证：跑了什么命令，结果如何>
<偏离：若有，单列一段说明>
```

- `type`：`feat`（接线新消息）/ `fix`（修正偏离）/ `test` / `docs` / `refactor` / `chore`
- 提交前必须跑过：
  ```bash
  cargo check --workspace --all-targets
  cargo clippy --workspace --exclude ddj-pb --lib --bins -- -D warnings
  cargo test --workspace
  ```

## 5. 构建与验证

**本机内存有限，不要在本地跑全量 release。** 本地只跑 `cargo check` /
`cargo test -p <crate>`，release 交给远端 `/workspace/ddj-rust`。

```bash
# 只编骨架（跳过 4.6 MB 的 proto 生成，省 ~900 MB 内存）
DDJ_PB_STUB=1 cargo check --workspace

# 本地单测（纯函数，不依赖 DB/网络）
cargo test -p ddj-game-handler

# 分层依赖校验
cargo xt check-arch

# 远端 CI（拉 gitee/main → clippy → test → release）
bash scripts/ci.sh
```

> `target/debug/incremental` 会涨到 11 GB 级别，磁盘紧时
> `rm -rf target/debug/incremental` 再 `CARGO_INCREMENTAL=0` 重跑。

## 6. 代码组织

```
crates/app/game-handler/src/
  <模块>.rs          纯业务逻辑，无 IO，依赖靠 trait 注入 —— 单测覆盖全部分支
  install_<模块>.rs  接线：闭包里解码 → 调纯函数 → 编码回包，注册进 HandlerTable
  runtime.rs         FnHandler / AsyncHandler / MemWorld（过渡期内存态）
  handlers.rs        总注册入口（register）
```

- **纯函数与接线分离是硬性的**：纯函数不能出现 `sqlx` / `reqwest` / 会话。
- 每个 `lib.rs` 顶部标注对应的 Java 类。
- `cargo xt check-arch` 强制分层：`foundation` 不认识上层、`domain` 不碰网络与 DB、
  `data` 不含业务、`protocol` 不含领域类型。

## 7. 消息号与协议

- 帧格式：`magic(0x71AB, i16) + total(i32) + cmdId(i32) + body`，**全大端**，头 10 字节。
- **只有 cmdId == 4（登录）** 走 XXTEA + Base64，其余全程明文。
- 免登录白名单：`[0, 3, 4, 5, 6, 7, 11, 98]`。
- `HandlerException` 的框架出口是 **`ErrorResp_1{msgId, errorCode}`**，帧头 cmdId 是 `1`，
  body 里的 `msgId` 才是请求号。用 `Reply::with_cmd(1, ...)` 表达。
- 服务端**主动推送**（11004/11007/16001/17096 等）走 `SessionControl::push`，
  不要塞进 `Reply` —— 那是两条独立消息。

## 8. 文档

| 文件 | 内容 | 更新时机 |
|---|---|---|
| `AGENTS.md`（本文） | 硬约束 | 规则变化时 |
| `docs/PROGRESS.md` | 当前进度、下一个目标、未决问题 | **每个 commit 后** |
| `docs/AUDIT.md` | 逐分支对照台账（原版位置 → Rust → 单测） | 每审计完一条消息 |
| `docs/PLAN.md` | 里程碑与批次 | 里程碑变化时 |
| `docs/ARCHITECTURE.md` | 分层、启动流程、扩展点 | 架构变化时 |
| `docs/OPERATIONS.md` | 运维手册 | 部署相关变化时 |
| `docs/PG-MIGRATION.md` | MySQL → PG 对照 | 迁移相关变化时 |

**切换上下文前必须更新 `docs/PROGRESS.md`**，写清「做到哪、下一步是什么、卡在哪」。
