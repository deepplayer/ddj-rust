# ddj-rs

《我是大东家》服务端 Rust 重写工程。

原服务端：**6 个 Java 进程 / 5034 个类 / 450 张表 / Netty + Hessian + Jetty**。
本工程：**32 个 crate / 1 个可执行文件 `ddj`**。

```text
app/        ddj-cli（唯一二进制：run/start/stop/attach/list/logs/restart）
            game-core（装配）   game-handler（103 Handler）
domain/     player item economy mail guild rank activity battle
protocol/   codec  pb  registry  rpc
net/        tcp  session  http
data/       db  cache  template  repo
foundation/ core  config  runtime  telemetry
```

**依赖只能向下**，`cargo xt check-arch` 强制校验（4 条规则）：
`foundation` 不认识任何上层；`domain` 不碰网络与 DB；`data` 不含业务；
`protocol` 不含领域类型。

## 一个二进制管全部服务

7 个服务不是 7 个程序，而是同一个 `ddj` 的 7 个**服务模块**：

```bash
ddj run   game      # 前台运行（systemd / docker / k8s 用这个）
ddj start game      # 后台守护，脱离终端与会话
ddj attach game     # 从任意终端接回控制台（回放 + 实时），Ctrl-C 只退出查看
ddj status game     # 单服务状态
ddj list            # 全部服务一览
ddj logs -f game    # 跟踪日志
ddj stop  game      # 优雅停机（也支持 stop all）
```

`start` 之后进程 `PPID=1`、`SID=自己`、`TTY=?` —— **SSH 掉线、关终端、
换机器都杀不掉它**；新会话 `ddj attach game` 即可接回。
重活细节见 [docs/OPERATIONS.md](docs/OPERATIONS.md)。

详见 [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)。

## 验收状态

```
cargo check  --workspace --all-targets   0 error
cargo clippy --workspace --all-targets   0 warning
cargo fmt    --all -- --check            无差异
cargo test   --workspace                 59 passed / 0 failed
cargo xt check-arch                      4 条规则通过
cargo test -p ddj-config --test all_services   7 份配置全部加载通过
```

代码量：**32 crate / 136 文件 / 8510 行**，每个 crate 内部按
`lib.rs（文档+重导出） + 一个领域一个文件` 组织，`lib.rs` 顶部注明对应的 Java 类。

---

## 快速开始

```bash
# 1. 编译检查（本机 1.9 GB 内存，jobs=1，约 8 分钟）
cargo check --workspace

# 2. 静态检查
cargo xt check-arch      # 分层依赖
cargo xt stats           # 代码量统计

# 3. 单测（纯函数，不需要 DB/网络）
cargo test -p ddj-item -p ddj-player -p ddj-rank -p ddj-mail -p ddj-guild -p ddj-battle
```

## 跑起来需要什么

| 依赖 | 版本 | 用途 | 是否必需 |
|---|---|---|---|
| PostgreSQL | 15 / 16 | 账号库 + 游戏库 | 必需 |
| Redis | 5.0+ | 缓存 / 排行榜 / 跨进程锁 | Web 端可无，游戏服必需 |
| protoc | 3.x | 编译期生成 proto（`DDJ_GEN_PB=1` 时才用） | 可选 |
| Rust | 1.99.1 | 见 `rust-toolchain.toml` | 必需 |

数据库准备（schema 由迁移自动建，不用手工导 SQL）：

```bash
createdb -U game platform
createdb -U game game_7010544
# 首次启动时 sqlx 自动跑 crates/data/db/migrations
```

密码不进配置文件，用环境变量注入：

```bash
export DDJ__DB__PLATFORM_URL='postgres://game:PASS@127.0.0.1:5432/platform'
export DDJ__DB__GAME_URL='postgres://game:PASS@127.0.0.1:5432/game_7010544'
export DDJ__REDIS__URL='redis://127.0.0.1:6379/0'
```

## 启动

```bash
cargo run -p ddj-cli -- run game        # 前台，10544
cargo run -p ddj-cli -- start game      # 后台守护
cargo run -p ddj-cli -- list            # 全部模块状态
```

日志格式由 `config/*.toml` 的 `[telemetry] log_format` 控制（`json` / `pretty`）。

## 工程命令

| 命令 | 作用 |
|---|---|
| `cargo xt gen-cmds` | 从 `work/letsproto.proto` 提取 2986 个 cmdId → `ddj-pb/src/cmd.rs` |
| `cargo xt gen-tables` | 从 `data/**/template/*.csv` 生成 626 张表 struct |
| `cargo xt gen-proto` | 统计 proto 规模（真实生成在 build.rs） |
| `cargo xt check-arch` | 校验分层依赖方向 |
| `cargo xt stats` | 各层文件数 / 行数 |

## 协议

客户端帧格式（与原服务端逐字节一致）：

```text
┌────────┬──────────────┬──────────────┬─────────────────┐
│ magic  │   length     │    cmdId     │      body       │
│ 0x71AB │ 含头 10 字节 │    int32     │    protobuf     │
│ int16  │    int32     │              │                 │
└────────┴──────────────┴──────────────┴─────────────────┘
                   全部大端（BIG_ENDIAN）
```

- 只有 `cmdId == 4`（登录）加密：XXTEA + Base64，密钥与客户端一致
- 单连接未完成消息上限 100 条，超限断开（复刻 Netty handler）
- 单帧 body 上限 100000 字节
- 免登录白名单：`0, 3, 4, 5, 6, 7, 11, 98`

## 目录约定

每个 `lib.rs` 顶部标注**对应的 Java 类名**，方便回归对照：

```rust
//! # ddj-item — 道具
//!
//! 对应 Java：`bf.game.server.domain.item.Item` + `ItemSet`
```

## 当前进度

| 项 | 结果 |
|---|---|
| `cargo check --workspace` | **0 error** |
| `cargo clippy --workspace --all-targets` | **0 warning** |
| `cargo fmt --check` | **无差异** |
| `cargo test --workspace` | **59 passed / 0 failed** |
| 分层依赖校验 | **4 条规则全部通过** |
| 真实 CSV 表 | **445 张 / 78215 行，全部解析，0 警告** |
| 真实抓包回环 | **逐字节一致** |

骨架可编译、可测试，**业务层待填**（103 Handler + 3224 个业务类）。
详见 [docs/ARCHITECTURE.md § 8](docs/ARCHITECTURE.md)。

设计决策记录在 [docs/adr/](docs/adr/)：
- [0001 为什么用 Rust 重写](docs/adr/0001-why-rewrite-in-rust.md)
- [0002 协议注册改编译期收集](docs/adr/0002-protocol-registry-at-compile-time.md)
- [0003 CSV 作为配置表唯一真相源](docs/adr/0003-csv-as-source-of-truth.md)
