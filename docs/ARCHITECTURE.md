# ddj-rs 架构总览

> 《我是大东家》Java 服务端（6 个进程 / 5034 类 / 277+173 表）的 Rust 重构工程。
> 本文件描述**骨架层**：目录、依赖方向、启动流程、扩展点。

---

## 1. 分层

```
┌──────────────────────────────────────────────────────────────┐
│  app/        ddj-cli（唯一二进制：run/start/stop/attach）      │  1 个可执行
│              game-core（装配）  game-handler（103 Handler）     │
├──────────────────────────────────────────────────────────────┤
│  domain/     player item economy mail guild rank activity battle │  纯业务，无 IO
├──────────────────────────────────────────────────────────────┤
│  protocol/   codec（帧）  pb（消息）  registry（分发）  rpc（跨服）│
├──────────────────────────────────────────────────────────────┤
│  net/        tcp  session  http                                │  接入层
├──────────────────────────────────────────────────────────────┤
│  data/       db  cache  template  repo                          │  持久化
├──────────────────────────────────────────────────────────────┤
│  foundation/ core  config  runtime  telemetry                   │  地基
└──────────────────────────────────────────────────────────────┘
```

**依赖只能向下**。`cargo xt dep-check` 会验证。

---

## 2. crate 清单（32 个）

| 层 | crate | 对应 Java | 行数 |
|---|---|---|---|
| foundation | `ddj-core` | `bf.util.*` / `jery.ngsp.utility.*` | 316 |
| | `ddj-config` | 63 个 XML → TOML | 436 |
| | `ddj-runtime` | `serverModuleListConfig.xml` + 生命周期 | 298 |
| | `ddj-telemetry` | log4j + 自定义 metrics | 187 |
| protocol | `ddj-codec` | `lets.WebPacket`（0x71AB 帧） | 361 |
| | `ddj-pb` | protobuf 8856 message | 94 |
| | `ddj-registry` | `MessageHandlerManager` 反射扫描 | 242 |
| | `ddj-rpc` | Hessian 跨服 | — |
| net | `ddj-tcp` | Netty 4.1.17 pipeline | 164 |
| | `ddj-session` | `ChannelPlayer` 绑定 | — |
| | `ddj-http` | jetty 1544 + 11 个 PHP | — |
| data | `ddj-db` | sqlx + PostgreSQL | 197 |
| | `ddj-cache` | jedis-2.9.0 | 215 |
| | `ddj-template` | CSV 配置表加载 | 453 |
| | `ddj-repo` | `JDBCRepository` | 322 |
| domain | `ddj-player` | `Player`（2000 行 POJO） | — |
| | `ddj-item` | `Item` / `ItemSet` | — |
| | `ddj-economy` | 充值 / 订单 / 流水 | — |
| | `ddj-mail` | `Mail` | — |
| | `ddj-guild` | `Guild` | — |
| | `ddj-rank` | `RankList` | — |
| | `ddj-activity` | 签到 / 七日 / 限时 | — |
| | `ddj-battle` | 战报 / 校验 | — |
| app | `ddj-game-core` | 各 `XxxServer.main()` | — |
| | `ddj-game-handler` | 103 个 `*MessageHandler` | — |
| | `ddj-cli` | 6 个 `start.sh` 启动脚本 → 1 个 `ddj` 二进制 | 18×1 |

---

## 3. 与原系统的关键差异

| 主题 | 原 Java | 本工程 | 理由 |
|---|---|---|---|
| 协议注册 | 类名后缀 `parseInt` + 反射 | `inventory` 编译期收集 | 配置里有代码里没有 → 链接期暴露 |
| 配置 | 63 个 XML | TOML + 环境覆盖 + 校验 | 改一个端口不用翻 63 个文件 |
| 跨服 | Hessian（Java 专有） | 原帧头 + protobuf | 跨语言、可观测 |
| DB | 3 个连接池共存 | sqlx 单池 + 编译期 SQL 校验 | — |
| DI | XML + 反射 | 显式结构体 | 缺依赖编译不通过 |
| Handler 返回值 | `Message` → 自动回包 / `void` → 自己发 | `Option<Resp>` 显式 | 语义清晰 |
| 状态机 | 隐式（`== null` 判断散落 103 处） | `SessionState` 枚举 + 迁移表 | 可测 |
| 错误 | `ret != 0` 裸码 | `AppError` 三层 + `code()` 映射 | 保留兼容码 |

---

## 4. 启动流程（`Bootstrap::run`）

```
读配置（TOML → env 覆盖 → 校验）
  ↓
init telemetry（JSON 日志 / Prometheus）
  ↓
PreInit   db.ping  cache.connect
  ↓
Init      template.scan   registry.install
  ↓
Start     session  scheduler  metrics
  ↓
Ready     tcp.listen  http.listen  rpc.listen
  ↓
等待 SIGINT / SIGTERM
  ↓
反序 stop（每模块幂等）
```

任一阶段失败 → **整进程退出**，不留半启动状态。
对应原 `serverModuleListConfig.xml` 的顺序，但依赖由 `ModuleKind::requires()` 拓扑排序得出，
不再依赖 XML 书写顺序。

---

## 5. 协议栈

```
字节流
  ↓ ddj-codec::parse        magic(2) len(4) cmd(4) = 10 字节头，大端
Frame { cmd_id, body }
  ↓ ddj-registry::guards    登录白名单 [0,3,4,5,6,7,11,98]
  ↓ ddj-registry::HandlerTable
ErasedHandler::handle_raw → Option<Bytes>
  ↓ ddj-codec::encode
字节流
```

**加密**：仅 `cmd_id == 4`（登录）走 XXTEA + Base64，密钥与原服务端逐字节相同。
其余消息全程明文 —— 这是原设计，非本工程遗漏。

---

## 6. 目录约定

```
crates/<层>/<模块>/
  Cargo.toml
  src/
    lib.rs        模块文档 + 子模块声明 + 重导出
    <领域词>.rs   一个文件一个概念
```

- 每个 `lib.rs` 顶部写**对应 Java 类**，方便回归对照。
- 每个 crate 末尾 `#[cfg(test)]` 里放纯函数单测，不依赖 DB/网络。

---

## 7. 扩展点

新增一条协议消息：

1. `work/letsproto.proto` 里加 message（或从原 proto 里找现成的）
2. `crates/protocol/pb/src/cmd.rs` 加常量
3. `crates/app/game-handler/src/<模块>.rs` 写 `pub fn` + `handle!` 宏注册
4. 领域逻辑写到对应 `crates/domain/*`

新增一个服务：

1. `ServiceKind` 加一个变体 + `modules()` 返回其模块清单
2. `config/<name>.toml` 加一份基线配置
3. 完成。**不需要新增 crate、不需要改 `main.rs`** —— 二进制已经能 `ddj run <name>`。

这是单二进制带来的实际收益：旧结构下加服务要新增工程 + 新启动脚本，
现在只是枚举加一个变体。

---

## 8. 当前状态

| 项 | 状态 |
|---|---|
| workspace 可编译 | ✅ **32 crate / 136 文件 / 8510 行，`cargo check --workspace` 0 error** |
| clippy | ✅ **`--all-targets` 0 warning** |
| 格式 | ✅ `cargo fmt --check` 无差异 |
| 单测 | ✅ **59 个测试全绿**（纯函数，不依赖 DB/网络） |
| 真实配置表 | ✅ **445 张 CSV / 78215 行全部解析，0 警告** |
| 7 份服务配置 | ✅ **全部按 schema 加载通过（`deny_unknown_fields` 校验）** |
| protobuf 真实生成 | ⏸ 默认关闭（`DDJ_GEN_PB=1` 开启，生成物 4.4 MB，需 ≥8 GB 内存） |
| DB schema | ⏸ 待把 `platform.sql` 拆成 sqlx migration |
| 业务逻辑 | ⏸ 3224 个 Java 业务类待逐一重写（主要工作量） |

### 已验证的关键事实

以下不是推测，是在真实数据/真实编译产物上验证过的：

1. **帧格式**：`0x71AB + len(i32) + cmd(i32)`，大端，头 10 字节；
   用真实抓包 `71ab00000015 00002afa 08 81b518 10 03 1a 03 08 b960` 解码通过，且编码回环逐字节一致。
2. **登录加密**：只有 `cmd_id == 4` 走 XXTEA + Base64，密钥与原服务端逐字节相同。
3. **登录白名单**：`FREE_LOGIN_CMDS = [0, 3, 4, 5, 6, 7, 11, 98]`，与原服务端一致。
4. **CSV 真实格式**：UTF-8 无 BOM / CRLF / 前 6 行 schema；`list` 有 8 种写法
   （`list;` `list;,=` `list;,` `list=` `list|,;` `list|` `list#` `list`）。
5. **配置深合并**：`config/game.toml` + `config/game.dev.toml` 覆盖生效，未写字段保留默认值。

### 骨架的边界

这个工程**只有结构，没有玩法**。现在能做的事：

- 读懂重构方案（分层、依赖方向、协议栈、启动流程）
- 新增一条协议 / 一个服务的完整路径（见 §7，有明确步骤）
- 拿真实 CSV 验证配置表加载链路

现在**不能**做的事：真正跑起来接客户端 —— 因为 103 个 Handler 与 3224 个业务类还是空的。
