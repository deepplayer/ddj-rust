# 移植执行计划（已定稿）

> 目标：把 Java 服务端（103 个 handler / 1680 个 `@LabelMessageMethod`）完整移植到 Rust，
> 逻辑与原版**字节级一致**（允许架构/性能优化，不允许改语义），并把 GM 后台用 Vue3 + Rust 重写。

## 已确认的决策

| 项 | 决定 |
|---|---|
| GM 端口 | 默认 `3099`，**可配置**（`config/gm.toml` 的 `net.http_listen`），业务端口一律不碰 |
| GM 前端 | Vue 3 + Vite + TS，**不用 Element Plus 等 UI 框架**，自建轻量组件 |
| GM 后端 | Rust（axum），PHP 全部退役，原 PHP 源码只作行为对照 |
| GM 权限 | 单账号 + token，操作写审计表 |
| 客户端测试 | **暂不做**，全部移植完成后统一做差分与端到端测试 |
| 验收线 | 每条分支可追溯：原版源码行号 → Rust 实现 → 单测名 |

前端依赖（2026-10 最新稳定版，实测 npm registry）：

```text
vue 3.5.44 · vite 8.3.5 · typescript 7.0.2 · vue-router 5.4.0 · pinia 4.1.0
```

> TS 7 / Vite 8 / Router 5 / Pinia 4 都是较新的大版本，若构建期出现生态不兼容，
> 按 `typescript@5.9` / `vite@7` 回退并在本文档记录原因。

## 阶段与批次

### A. 已实现全量审计（13 个文件 / 33 个 cmdId）

逐文件对照 `data_d/` 里的反编译源码，把每条分支写进 `AUDIT.md` 台账，
发现偏离就改 Rust（不改原版行为）。

| # | 文件 | cmdId / 内容 |
|---|---|---|
| A1 | `item.rs` | 11002·11003·11005·11006·11008 + 11004/11007 推送 |
| A2 | `player.rs` | 6 创角·1002 改名·1003 升级·1100 存档·1008/1020 看他人 |
| A3 | `sign.rs` | 16002·16003·16004 |
| A4 | `mail.rs` | 10001·10002·10003·10004 |
| A5 | `amber.rs` | 17091~17095 |
| A6 | `player_misc.rs` | 1009·1010·1015·1024 |
| A7 | `login.rs` + `loginvar.rs` + `handshake.rs` | 4·16·17·18 + 挑战密码 |
| A8 | `player_state.rs` + `push.rs` + `handlers.rs` | 存档门面 + 推送 + 装配 |

### B. 未移植补齐（约 1647 个 cmdId）

| 批次 | 内容 | 约计 |
|---|---|---|
| B1 | WebSocket 传输层（照 `jery.ngsp.network.api` 源码） | — |
| B2 | 把已实现的 25 个 cmdId 接进 `HandlerTable` | 25 |
| B3 | `PlayerMessageHandler` 剩余 | 15 |
| B4 | 门客 29 / 宠物 26 / 家族 32 / 交易 26 | 113 |
| B5 | 活动族（`GmActivity*` 五类 341 + 宴席 48 + 秘境 47 + 贪吃 42 + 系统活动 37 + 节日约 177） | 约 692 |
| B6 | 其余（战斗/跨服/排行/世界/聊天/商店/任务…） | 约 827 |

B5 实现顺序按「服务端默认开启」优先：宴席 → 贪吃 → 系统活动 → 限时节日。

---

## B'. 接下来的移植目标（2026-10-28 定，按顺序做）

> 每行 = **一个目标 = 一次 commit**。做完把「进度」打勾，并把 commit 写进 `PROGRESS.md`。
> 选题原则：① 不依赖跨服 RPC（`CsServer`）；② 依赖的实体模块越少越好；
> ③ 优先吃完某个 handler，避免到处留半截。
> 当前 **82 / 1681**（不含进行中的 Travel）。

| # | 目标 | cmdId | 前置依赖 | 风险 | 进度 |
|---|---|---|---|---|---|
| T1 | **Travel 游历**（进行中） | 15002 / 15004 / 15009 | `travel.rs` 26 测试 ✅；`install_travel.rs` 已写待编译 | 低 | 🚧 |
| T2 | **LanternRiddles 灯谜** | 37603-37607 | 无（纯配置驱动） | 低 | ⬜ |
| T3 | **OceanTrade 海贸** | 41001-41003 | `activity_template` 已就位 | 中（跨天重置） | ⬜ |
| T4 | **Trade 通商** | 19002-19005 | 无 | 中（要 `TraderSet`） | ⬜ |
| T5 | **Cornucopia 抽奖落地** | 38301 / 38302 | 38303/38304 已接 | 中（要新表） | ⬜ |
| T6 | **Blessing 祝福** | 33021 / 33022 | 需 `CsServer` 跨服 | **高 → 推迟** | ⬜ |
| T7 | **Mission 任务** | 9003/9004/9006/9101 | 需 `TaskDBCache` + `PlayerMission` | 高（两个新实体） | ⬜ |
| T8 | **Ranking 排行** | 5 条 | `ranking/` 2464 行 / 14 文件，重 `CsServer` 耦合 | **高 → 推迟** | ⬜ |
| T9 | **MemWorld → PG 迁移** | — | 7 个业务组仍是进程内 | 中（重启丢数据） | ⬜ |
| T10 | **PetMessageHandler 收尾** | 剩余 | 需 `PetYard` / `PetMissionServerModule` / `PetRentServerModule` / `PetSeniorSet` | 高（4 个新实体） | ⬜ |

> T2-T5 是**同一规模**（3-5 条、配置驱动、不碰跨服），做完约 +18 条 → **100 / 1681**。
> T6 / T8 要等 `ddj-rpc` 铺开，本批不做。
> T9 应尽早做：现在 82 条绝大多数挂在 `MemWorld`，重启即丢，越晚迁成本越高。

### 每个目标的固定动作清单

1. 读 `data_d/.../<X>MessageHandler.java` 及其调用的模块，事实先写进 `AUDIT.md`
2. 纯逻辑 `crates/app/game-handler/src/<x>.rs`（无 IO、带单测、注释标 Java 出处）
3. 装配 `crates/app/game-handler/src/install_<x>.rs`（解码 → 调纯函数 → 改状态 → 编码）
4. `crates/data/.../csv_<x>.rs`（需要真实 CSV 时）
5. `cargo test -p ddj-game-handler` + `cargo run -p xtask -- check-arch`
6. `git commit` → `git push github main` → 等 GitHub Actions
7. `gh run download` 取回 `bin/ddj` → 写 `e2e/smoke/<x>_smoke.mjs` → 实跑
8. 更新 `PROGRESS.md`（消息数 / 测试数 / 构建记录 / 冒烟结果）+ `AUDIT.md`

### C. GM 后台（Rust API + Vue3）

| 子项 | 内容 |
|---|---|
| C1 | 清点 `data/web` 11 个 PHP 的行为，整理成 API 契约（路径、入参、返回码、副作用） |
| C2 | Rust GM 服务：`crates/app/svc-gm` 起 axum，`/api/*`，静态托管 Vue 产物 |
| C3 | Vue3 界面：登录 / 玩家查询与改档 / 发放邮件与道具 / 充值 / CDK 管理 / 审计日志 |
| C4 | 自建组件：表格（排序分页）、表单、弹窗、Toast、深色模式（CSS 变量） |
| C5 | 鉴权与审计：token + 操作留痕表 |
| C6 | 部署：同机不同端口；`gm.toml` 可改端口与数据库连接 |

### D. 验收基建（全部移植完成后再跑）

差分矩阵、CI 门禁、一键对照报告。客户端测试（无头 JS 客户端）按决策推迟到 M6。

## 里程碑

| 里程碑 | 内容 | 完成标志 |
|---|---|---|
| M1 | B1 传输层 + A 全量审计 | 审计台账填满；TCP/WS 双传输可用 |
| M2 | B2 接线现有 25 个实现 | 33 个 cmdId 全部在线 |
| M3 | B3 + B4 | 约 128 个 cmdId 上线 |
| M4 | B5 活动族 | 约 692 个 cmdId |
| M5 | C GM 后台 | 3099 端口可用，接口与 PHP 行为一致 |
| M6 | B6 + D 统一测试 | 差分全绿 + 部署脚本 |

## 约束

- 本地不编译（1.9 GB 内存），一律远端 `/workspace/ddj-rust` 编译测试；代码走 gitee 同步。
- 每批提交信息要写清「原版依据」，便于回溯。
- 原版缺陷照抄（例如「模板缺行 → NPE → ret=2」这类），在注释里标明是复刻缺陷而非本意。
