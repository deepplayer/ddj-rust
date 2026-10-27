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

当前已知 3 处，**已定策略（2026-10-27）：保留护栏 + 触发时记录，等真实客户端验证后再定**。

| 位置 | 原版 | 当前 Rust | 影响面 |
|---|---|---|---|
| `item.rs` `MAX_COMPOUND_DEPTH = 64` | 死循环 | 达到上限回 `PARAM_ERROR` | 只在畸形表数据下分叉 |
| `sign.rs` `rand_pick` | 池空抛 `IndexOutOfBoundsException` | 提前 `break` | 只在 `randTimes > 奖励数` 时分叉 |
| `item.rs` `mul_or_record` / `add_or_record` | `long` 溢出回绕（负数） | 饱和到 `i64` 边界 | 只在极端数值下分叉 |

### 记录是硬性要求

每处护栏**必须**在触发时调 `crate::divergence::record(...)`：

```rust
crate::divergence::record(
    crate::divergence::Divergence::P1CompoundCycle,
    format!("compoundId={} useItemId={} chain={:?}", id, use_id, visited),
);
```

`detail` 必须带**够定位到具体数据行**的上下文（模板 id / 天数 / 操作数 / 完整路径）。
新增第 4 处分叉时：

1. 在 `divergence.rs` 的 `Divergence` 枚举加变体（`code()` 用 `P4`）；
2. 填 `original()` 与 `ours()`（会进日志）；
3. 加进 `ALL` 与计数器；
4. 在 `docs/AUDIT.md` 的表里加一行，并写清「`detail` 里带什么、怎么用」；
5. 补两个单测：触发时计数 +1、正常路径不计数。

查看方式：`ddj status <service>`（单服务查询会打印三行计数）。

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
- 门禁在 **GitHub Actions** 里跑（见 §5）：commit → push github → 等 `check` + `release` 绿
  → `gh run download` 取回二进制 → 本机验证。本机不跑 clippy/test 全量。

## 5. 构建与验证

**本地一律不编译。** 流程固定为：

```
改代码 → commit → push github → GitHub Actions 编译
      → gh run download 取回二进制 → 本机 bin/ddj 验证
```

### 仓库与远程

| remote | 地址 | 用途 |
|---|---|---|
| `github` | `https://github.com/deepplayer/ddj-rust.git` | **主**：Actions 在此编译 |
| `gitee` | `git@gitee.com:deepplayer/ddj-rust.git` | 历史镜像，可不再推送 |

### 完整命令

```bash
# 1. 提交并推送（触发 Actions）
git add -A && git commit -m "..." && git push github main

# 2. 等 Actions 跑完（release job 约 3 分钟）
gh run list --limit 3
gh run watch              # 或反复 gh run view <id>

# 3. 取回产物（artifact 名带 commit sha）
gh run download <run-id> -D /tmp/gh-art
cp /tmp/gh-art/ddj-<full-sha>/ddj bin/ddj && chmod +x bin/ddj

# 4. 本机验证
sha256sum bin/ddj && ./bin/ddj --version && ./bin/ddj list
```

### 门禁在 Actions 里

`.github/workflows/ci.yml` 有三个 job：

| job | 内容 | 是否阻塞 |
|---|---|---|
| `check` | `cargo xt check-arch` → clippy `--lib --bins -D warnings` → `cargo test --workspace` | **是** |
| `fmt` | `cargo fmt --all -- --check` | 否（`continue-on-error`，移植期未统一格式化） |
| `release` | `cargo build --release -p ddj-cli` + 上传 artifact（保留 14 天） | 是（`needs: check`） |

所以 **push 之后看 `check` 与 `release` 两个 job 是否绿**，`fmt` 红是已知状态。

### 历史教训（web-shell 方案已废弃）

早期用 web-shell 在远端 `/workspace/ddj-rust` 编译，遇到两个问题，这里记录以免重犯：

1. **web-shell 每次请求可能落到不同容器，但 `/workspace` 是共享卷** ——
   不同节点的 `target/` 缓存互相污染，实测 `Exec format error`、
   `target/debug/deps/ddj-*` 变成 `data`、target 涨到 66G。
2. **远端后台构建活不下来** —— `nohup` / `setsid` 都躲不过节点回收，
   两次构建都在中途消失。

现在改用 GitHub Actions：环境干净、每次全新 runner、产物有 artifact 存档，不再有这些问题。

### 本机允许做的事

- `cargo check --workspace`（省时间，只验语法/类型）
- `cargo xt check-arch`（纯静态，秒级）
- `cargo test -p <crate>`（单 crate 纯函数单测，秒级）
- **不允许** `cargo build --release`（内存不足且没必要）

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
