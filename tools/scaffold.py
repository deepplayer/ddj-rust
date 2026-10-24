#!/usr/bin/env python3
"""生成 ddj-server Rust workspace 的全部 crate 骨架。

用法: python3 tools/scaffold.py
只写 Cargo.toml / lib.rs / main.rs，不覆盖已有业务实现。
"""
from __future__ import annotations

import os
import textwrap
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

COMMON = """\
version.workspace      = true
edition.workspace      = true
rust-version.workspace = true
license.workspace      = true
authors.workspace      = true

[lints]
workspace = true
"""


def manifest(name: str, desc: str, deps: list[str], *, bin: bool = False, extra: str = "") -> str:
    head = f"""\
[package]
name        = "{name}"
description = "{desc}"
{COMMON}"""
    d = "\n".join(deps) if deps else "# 无外部依赖"
    body = f"\n[dependencies]\n{d}\n"
    if bin:
        body += f"""
[[bin]]
name = "{name}"
path = "src/main.rs"
"""
    if extra:
        body += "\n" + extra.strip() + "\n"
    return head + body


CRATES: list[dict] = []


def crate(
    path: str,
    name: str,
    desc: str,
    deps: list[str],
    lib: str | None = None,
    main: str | None = None,
    extra: str = "",
    modules: list[str] | None = None,
) -> None:
    CRATES.append(
        dict(path=path, name=name, desc=desc, deps=deps, lib=lib, main=main,
             extra=extra, modules=modules or [])
    )


# ═══════════════════════════════════════════════════════════════════════════
#  FOUNDATION — 无业务，全栈可依赖
# ═══════════════════════════════════════════════════════════════════════════

crate(
    "foundation/core", "ddj-core", "核心基础类型：强类型 ID / 三层错误 / 可注入时钟 / 可播种随机",
    [
        "thiserror.workspace    = true",
        "serde.workspace        = true",
        "serde_json.workspace   = true",
        "chrono.workspace       = true",
        "uuid.workspace         = true",
        "rand.workspace         = true",
        "bytes.workspace        = true",
        "tracing.workspace      = true",
        "dashmap.workspace      = true",
        "parking_lot.workspace  = true",
    ],
    lib='''\
//! # ddj-core — 核心基础类型
//!
//! 对应 Java：`bf.util.*` + `jery.ngsp.utility.*`
//!
//! 这一层不含任何游戏业务，也不做 IO。所有上层 crate 都允许依赖它。

pub mod error;
pub mod id;
pub mod clock;
pub mod rng;
pub mod result;

pub use clock::{Clock, SystemClock, TestClock};
pub use error::{AppError, DomainError, ErrorKind, ServiceError};
pub use id::{GuildId, PlayerId, ServerId, TemplateId};
pub use result::Result;
pub use rng::{DeterministicRng, RngSource};
''',
    modules=["error", "id", "clock", "rng", "result"],
)

crate(
    "foundation/config", "ddj-config", "分层配置：TOML + 环境变量覆盖 + 热加载",
    [
        "ddj-core.workspace      = true",
        "serde.workspace         = true",
        "toml.workspace          = true",
        "serde_json.workspace    = true",
        "thiserror.workspace     = true",
        "tracing.workspace       = true",
        "parking_lot.workspace   = true",
    ],
    lib='''\
//! # ddj-config — 配置体系
//!
//! 取代 Java 侧 63 个 XML（`serverModuleListConfig.xml` / `gameServerConfig.xml` /
//! `redisConfig.xml` / `jetty_n.xml` / `nginxConfig.xml` / `GameReadWriteLockConfig.xml` …）。
//!
//! ## 加载顺序（后者覆盖前者）
//! 1. 内置默认值（`Default` impl）
//! 2. `config/{service}.toml`
//! 3. `config/{service}.{env}.toml`
//! 4. 环境变量 `DDJ__SECTION__KEY`
//! 5. 启动参数 `--set section.key=value`
//!
//! ## 子模块
//! - [`loader`]  文件 + 环境变量合并
//! - [`schema`]  各服务的配置结构体定义
//! - [`validate`] 启动前校验（端口范围、超时正负、必填项）

pub mod loader;
pub mod schema;
pub mod validate;

pub use loader::{ConfigLoader, Environment};
pub use validate::Validate;
''',
    modules=["loader", "schema", "validate"],
)

crate(
    "foundation/telemetry", "ddj-telemetry", "可观测性：结构化日志 / Prometheus 指标 / 分布式追踪",
    [
        "ddj-config.workspace            = true",
        "tracing.workspace               = true",
        "tracing-subscriber.workspace    = true",
        "metrics.workspace               = true",
        "metrics-exporter-prometheus.workspace = true",
        "serde.workspace                 = true",
        "serde_json.workspace            = true",
        "rand.workspace                  = true",
    ],
    lib='''\
//! # ddj-telemetry — 可观测性三件套
//!
//! 对应 Java：`log4j` + `slf4j`（仅日志）。
//! Rust 侧一次到位：**日志 + 指标 + 追踪**，且三者共享 trace_id。
//!
//! ## 子模块
//! - [`logging`]  `tracing-subscriber` 初始化，JSON / pretty 双模式
//! - [`metrics`]  指标注册表 + `/metrics` Prometheus 端点
//! - [`trace`]    span 传播；跨服 RPC 用 W3C `traceparent` 头
//! - [`audit`]    审计日志：GM 操作、充值、道具增减，独立 sink

pub mod audit;
pub mod logging;
pub mod metrics;
pub mod trace;

pub use logging::LogFormat;
''',
    modules=["audit", "logging", "metrics", "trace"],
)

crate(
    "foundation/runtime", "ddj-runtime", "服务生命周期：模块注册 / 阶段启动 / 优雅停机",
    [
        "ddj-core.workspace       = true",
        "ddj-config.workspace     = true",
        "ddj-telemetry.workspace  = true",
        "tokio.workspace          = true",
        "async-trait.workspace    = true",
        "tracing.workspace        = true",
        "thiserror.workspace      = true",
        "futures.workspace        = true",
    ],
    lib='''\
//! # ddj-runtime — 模块容器与生命周期
//!
//! 对应 Java：`serverModuleListConfig.xml` 驱动的模块装配 + `jery.ngsp` 的
//! `init/start/stop` 钩子。
//!
//! ## 启动阶段（严格顺序）
//! `PreInit` → `Init` → `Start` → `Ready`  （`Stop` 反序）
//!
//! 任一模块 `Init` 失败则**整进程不进入 Start**，避免半启动状态。
//!
//! ## 子模块
//! - [`module`]   `Module` trait + `Phase`
//! - [`registry`] 静态模块注册表（编译期 inventory，无需 XML）
//! - [`lifecycle`] 编排器：拓扑排序 + 超时 + 停机钩子
//! - [`context`]  模块间共享的 `ServiceContext`

pub mod context;
pub mod lifecycle;
pub mod module;
pub mod registry;

pub use context::ServiceContext;
pub use lifecycle::Lifecycle;
pub use module::{Module, Phase};
''',
    modules=["context", "lifecycle", "module", "registry"],
)

# ═══════════════════════════════════════════════════════════════════════════
#  PROTOCOL
# ═══════════════════════════════════════════════════════════════════════════

crate(
    "protocol/codec", "ddj-codec", "客户端帧编解码：magic 0x71AB + 大端长度 + cmdId",
    [
        "ddj-core.workspace    = true",
        "bytes.workspace       = true",
        "tokio-util.workspace  = true",
        "thiserror.workspace   = true",
        "tracing.workspace     = true",
        "tokio.workspace       = true",
    ],
    lib='''\
//! # ddj-codec — 帧编解码
//!
//! 严格复刻 `lets.WebPacket`：
//!
//! ```text
//! ┌────────┬──────────────┬──────────────┬──────────────────┐
//! │ int16  │    int32     │    int32     │                  │
//! │ 0x71AB │ total_length │    cmd_id    │   protobuf body  │
//! │  2 B   │    4 B       │    4 B       │   len = total-10 │
//! └────────┴──────────────┴──────────────┴──────────────────┘
//!            大端 (ByteBuffer.DEFAULT_ENDIAN = BIG_ENDIAN)
//! ```
//!
//! `total_length` **包含** 10 字节头，与原客户端 `bf_writeHead_bf` 一致。
//!
//! ## 限制（与原服务端 `NettyServerHandler` 对齐）
//! - `total_length` > [`MAX_BODY`] → 丢弃并断开
//! - magic 不匹配 → 立即 `close()`
//! - 单连接累计消息 > [`MAX_QUEUE`] → `Channel.close()`

pub mod frame;
pub mod decoder;
pub mod encoder;
pub mod limits;

pub use decoder::FrameDecoder;
pub use encoder::FrameEncoder;
pub use frame::{Frame, HEADER_LEN, MAGIC};

/// 单帧最大字节数（原服务端硬编码 100000）。
pub const MAX_BODY: usize = 100_000;
/// 单连接积压消息上限（原服务端硬编码 100）。
pub const MAX_QUEUE: usize = 100;
''',
    modules=["frame", "decoder", "encoder", "limits"],
)

crate(
    "protocol/pb", "ddj-pb", "protobuf 生成物 + 类型化消息分发",
    [
        "ddj-core.workspace     = true",
        "prost.workspace        = true",
        "bytes.workspace        = true",
        "serde.workspace        = true",
    ],
    extra="[build-dependencies]\nprost-build = \"0.13\"\n",
    lib='''\
//! # ddj-pb — protobuf 消息层
//!
//! 源文件 `letsproto.proto`（1.75 MB / 8856 message / 27718 field / proto2）。
//!
//! ## 为什么生成物不签入仓库
//! `include!` 一个 4.4 MB 的单文件会让 rustc 峰值内存到 ~900 MB。
//! 本机 1.9 GB 会 OOM（实测两次）。
//! 因此 `build.rs` 按 `pub mod` 把生成物拆成 117 个文件写进 `OUT_DIR`，
//! 只有 crate 本身编译时才展开。
//!
//! ## 命名映射（prost 规则，需注意）
//! | proto | Rust |
//! |---|---|
//! | `ItemUseReq_11002` | `item_message::ItemUseReq11002` |
//! | `MsgUseParameter` | `item_message::MsgUseParameter` |
//!
//! 下划线被消除、message 按 proto 文件内前缀归入 module。

pub mod cmd;
pub mod dispatch;

#[allow(missing_docs, clippy::all, clippy::pedantic)]
pub mod gen {
    //! `prost-build` 生成物（扁平导出）。
    include!(concat!(env!("OUT_DIR"), "/letsproto.rs"));
}

pub use dispatch::{MessageKind, TypedMessage};
''',
    modules=["cmd", "dispatch"],
)

crate(
    "protocol/registry", "ddj-registry", "消息注册表：cmdId ↔ Handler 的编译期绑定",
    [
        "ddj-core.workspace      = true",
        "ddj-pb.workspace        = true",
        "ddj-codec.workspace     = true",
        "async-trait.workspace   = true",
        "tracing.workspace       = true",
        "bytes.workspace         = true",
        "dashmap.workspace       = true",
    ],
    lib='''\
//! # ddj-registry — 消息路由表
//!
//! 取代 Java 侧的**反射扫描**：
//!
//! ```java
//! // MessageHandlerManager.loadHandler()
//! scan("bf.game.server.message.handler")       // 扫包
//!   .filter(@LabelMessageHandler)              // 类注解
//!   .flatMap(m -> @LabelMessageMethod)         // 方法注解
//!   .forEach(m -> register(
//!       Integer.parseInt(simpleName.substring(lastIndexOf('_')+1)),  // cmdId 从类名后缀猜
//!       m))
//! ```
//!
//! 这套机制的问题：cmdId 靠**类名后缀猜**、启动时才暴露冲突、IDE 无法跳转、
//! 死代码无法发现。
//!
//! Rust 侧改为 **`#[handler(11002)]` 属性宏 + `inventory` 编译期收集**：
//! - cmdId 是字面量，冲突 = 编译错误
//! - 零反射、零启动扫描
//! - `cargo expand` 可直接看清注册了什么
//!
//! ## 子模块
//! - [`handler`]   `MessageHandler` trait + 请求上下文
//! - [`table`]     `HandlerTable`：cmdId → handler 的只读查找表
//! - [`guard`]     前置守卫：登录态 / 封禁 / 限速 / 白名单（对应原 `msgId 3,4,0,5,6,7,98,11` 免登录表）

pub mod guard;
pub mod handler;
pub mod table;

pub use guard::{Guard, GuardChain, GuardOutcome};
pub use handler::{HandlerCtx, MessageHandler};
pub use table::HandlerTable;

/// 免登录 cmdId 白名单 —— 与原服务端完全一致。
pub const FREE_LOGIN_CMDS: &[i32] = &[0, 3, 4, 5, 6, 7, 11, 98];
''',
    modules=["guard", "handler", "table"],
)

crate(
    "protocol/rpc", "ddj-rpc", "跨服 RPC：Hessian 兼容的二进制调用",
    [
        "ddj-core.workspace      = true",
        "ddj-codec.workspace     = true",
        "ddj-config.workspace    = true",
        "tokio.workspace         = true",
        "bytes.workspace         = true",
        "async-trait.workspace   = true",
        "thiserror.workspace     = true",
        "tracing.workspace       = true",
        "dashmap.workspace       = true",
    ],
    lib='''\
//! # ddj-rpc — 跨服通信
//!
//! 原系统：`hessian-4.0.60` + `RemoteAbsHandler`，跨服端口 4544 / 4106。
//! 两端都是自己的代码，格式**自洽即可**，不必兼容 Hessian。
//!
//! 因此 Rust 侧重新定义一套简洁的二进制 RPC：
//!
//! ```text
//! ┌────────┬──────────┬──────────┬───────────┬──────────┬─────────┐
//! │ u8 ver │ u32 seq  │ u32 rpc  │ u16 flags │ u32 len  │ payload │
//! └────────┴──────────┴──────────┴───────────┴──────────┴─────────┘
//! ```
//!
//! ## 子模块
//! - [`wire`]    报文结构与编解码
//! - [`client`]  连接池 + 超时 + 重试 + 熔断
//! - [`server`]  服务端 accept + 请求分发
//! - [`service`] 服务接口定义（`PlayerRemote` / `GuildRemote` / `BattleRemote`）

pub mod client;
pub mod server;
pub mod service;
pub mod wire;

pub use client::RpcClient;
pub use server::RpcServer;
''',
    modules=["client", "server", "service", "wire"],
)

# ═══════════════════════════════════════════════════════════════════════════
#  NET
# ═══════════════════════════════════════════════════════════════════════════

crate(
    "net/session", "ddj-session", "会话状态机：连接 / 登录 / 心跳 / 踢号",
    [
        "ddj-core.workspace      = true",
        "ddj-config.workspace    = true",
        "ddj-pb.workspace        = true",
        "ddj-codec.workspace     = true",
        "tokio.workspace         = true",
        "futures.workspace       = true",
        "dashmap.workspace       = true",
        "parking_lot.workspace   = true",
        "tracing.workspace       = true",
        "chrono.workspace        = true",
        "uuid.workspace          = true",
        "rand.workspace          = true",
    ],
    lib='''\
//! # ddj-session — 会话
//!
//! 对应 Java：`ChannelPlayer` + `Player` 的绑定关系，以及 100 条消息积压上限。
//!
//! ## 状态机
//! ```text
//!     Connected ──login──▶ Authenticated ──enterGame──▶ Playing
//!         │                     │                          │
//!         └──timeout────────────┴──kick────────────────────┘
//! ```
//!
//! ## 子模块
//! - [`state`]   `SessionState` 枚举 + 合法迁移表
//! - [`session`] `Session` 结构：id / 连接句柄 / 玩家引用 / 心跳
//! - [`manager`] 全服会话表：在线查询、按玩家踢号、容量限制

pub mod manager;
pub mod session;
pub mod state;

pub use manager::SessionManager;
pub use session::Session;
pub use state::SessionState;
''',
    modules=["manager", "session", "state"],
)

crate(
    "net/tcp", "ddj-tcp", "TCP 长连接服务：accept / 读写循环 / 背压",
    [
        "ddj-core.workspace      = true",
        "ddj-config.workspace    = true",
        "ddj-codec.workspace     = true",
        "ddj-session.workspace   = true",
        "ddj-telemetry.workspace = true",
        "tokio.workspace         = true",
        "tokio-util.workspace    = true",
        "futures.workspace       = true",
        "bytes.workspace         = true",
        "tracing.workspace       = true",
    ],
    lib='''\
//! # ddj-tcp — 传输层
//!
//! 对应 Java：`netty-all-4.1.17`。
//!
//! ## tokio 映射
//! | Netty | tokio |
//! |---|---|
//! | `ChannelPipeline` | `Decoder` + `Encoder` + 任务 |
//! | `IdleStateHandler` | `tokio::time::timeout` |
//! | `ChannelGroup` | `SessionManager` |
//! | `WriteBufferWaterMark` | `mpsc` 有界通道（背压） |
//!
//! ## 子模块
//! - [`listener`]  `TcpListener` + accept 循环 + 连接计数
//! - [`conn`]      单连接读写驱动
//! - [`pipeline`]  `Frame` → `TypedMessage` → handler → 回包

pub mod conn;
pub mod listener;
pub mod pipeline;

pub use listener::TcpServer;
''',
    modules=["conn", "listener", "pipeline"],
)

crate(
    "net/http", "ddj-http", "HTTP 层：GM 后台 / 健康检查 / metrics",
    [
        "ddj-core.workspace      = true",
        "ddj-config.workspace    = true",
        "ddj-telemetry.workspace = true",
        "axum.workspace          = true",
        "tower.workspace         = true",
        "tower-http.workspace    = true",
        "tokio.workspace         = true",
        "serde.workspace         = true",
        "serde_json.workspace    = true",
        "tracing.workspace       = true",
    ],
    lib='''\
//! # ddj-http — HTTP 服务
//!
//! 对应 Java：`jetty-runner-7.6`（端口 1544 的 GM 后台）+ `data/web` 的 11 个 PHP。
//!
//! ## 路由分组
//! | 组 | 路径前缀 | 说明 |
//! |---|---|---|
//! | 运维 | `/healthz` `/readyz` `/metrics` | 无鉴权，仅内网 |
//! | GM | `/gm/v1/*` | JWT + 角色 RBAC |
//! | SDK | `/sdk/v1/*` | 兼容原 `i.php` / `l.php` 的加密契约 |
//! | 回调 | `/callback/v1/*` | 充值 / CDK |
//!
//! ## 子模块
//! - [`router`]    路由装配
//! - [`middleware`] 追踪 / 限流 / 鉴权 / 错误映射
//! - [`error`]     `AppError` → HTTP 响应

pub mod error;
pub mod middleware;
pub mod router;

pub use router::build_router;
''',
    modules=["error", "middleware", "router"],
)

# ═══════════════════════════════════════════════════════════════════════════
#  DATA
# ═══════════════════════════════════════════════════════════════════════════

crate(
    "data/db", "ddj-db", "MySQL 访问层：sqlx 连接池 + 事务 + 分库路由",
    [
        "ddj-core.workspace       = true",
        "ddj-config.workspace     = true",
        "ddj-telemetry.workspace  = true",
        "sqlx.workspace           = true",
        "tokio.workspace          = true",
        "async-trait.workspace    = true",
        "thiserror.workspace      = true",
        "tracing.workspace        = true",
        "chrono.workspace         = true",
    ],
    lib='''\
//! # ddj-db — 数据访问
//!
//! 对应 Java：`mysql-connector-java` + `druid` + `proxool` + `jery.ngsp.db.JDBCRepository`。
//!
//! ## 库划分（源自 `data/`）
//! | 库 | 表数 | 用途 |
//! |---|---|---|
//! | `platform` | 277 | 账号 / 区服 / 充值 / CDK |
//! | `game_7010544` | 173 | 角色 / 道具 / 邮件 / 公会 |
//! | `cross_7010106` | — | 跨服 |
//! | `world7010002` | — | 世界 |
//!
//! ## 子模块
//! - [`pool`]       连接池构建（每库一个 `MySqlPool`）
//! - [`tx`]         事务包装 `UnitOfWork`
//! - [`shard`]      分服路由：`ServerId → 库名`
//! - [`migration`]  内嵌 sqlx migrations

pub mod migration;
pub mod pool;
pub mod shard;
pub mod tx;

pub use pool::DbPools;
pub use shard::ShardRouter;
pub use tx::UnitOfWork;
''',
    modules=["migration", "pool", "shard", "tx"],
)

crate(
    "data/cache", "ddj-cache", "Redis 访问层 + 分布式锁",
    [
        "ddj-core.workspace       = true",
        "ddj-config.workspace     = true",
        "redis.workspace          = true",
        "tokio.workspace          = true",
        "async-trait.workspace    = true",
        "thiserror.workspace      = true",
        "tracing.workspace        = true",
        "uuid.workspace           = true",
        "chrono.workspace         = true",
    ],
    lib='''\
//! # ddj-cache — 缓存与锁
//!
//! 对应 Java：`jedis-2.9.0` + `DistributedLock`（`GameReadWriteLockConfig.xml`）。
//!
//! ## 子模块
//! - [`conn`]   `ConnectionManager` 自动重连
//! - [`kv`]     带 TTL 的读写 + 序列化策略
//! - [`lock`]   `SET NX PX` + Lua 释放；带 fencing token
//! - [`rank`]   ZSet 排行榜原语（对应原系统 rank 模块）
//!
//! > 原系统 `GameReadWriteLockConfig.xml` 只有 `lockIsWrite` / `defaultWriteTime`
//! > 两项，说明全局读写锁是"一把大锁 + 超时"。Rust 侧改为**按键分段锁**，
//! > 不同玩家不再互相阻塞。

pub mod conn;
pub mod kv;
pub mod lock;
pub mod rank;

pub use conn::RedisPool;
pub use lock::DistributedLock;
''',
    modules=["conn", "kv", "lock", "rank"],
)

crate(
    "data/template", "ddj-template", "CSV 模板表：445 表 / 45 万行的 typed 加载",
    [
        "ddj-core.workspace       = true",
        "ddj-config.workspace     = true",
        "csv.workspace            = true",
        "serde.workspace          = true",
        "thiserror.workspace      = true",
        "tracing.workspace        = true",
        "dashmap.workspace        = true",
        "parking_lot.workspace    = true",
        "indexmap.workspace       = true",
    ],
    lib='''\
//! # ddj-template — 配置表
//!
//! 对应 Java：`bf.template.db.*`（450 个 `XxxDB` class）+ `jery.ngsp.template.TemplateFactory`。
//!
//! ## CSV 结构（前 6 行是 schema，第 7 行起是数据）
//! ```text
//! 1  AmberWelfareDB                 ← 实体类名
//! 2  PK                             ← 主键标记
//! 3  3,3,3,3,3,3                    ← 列宽
//! 4  int,string,int,int,int,"list;,="  ← 类型
//! 5  id,cid,type,day,payValue,reward   ← 字段名
//! 6  ID,oppoCid,类型,签到天数,...       ← 中文注释
//! 7+ 1,1001,1,1,0,"1;2,3"           ← 数据
//! ```
//!
//! ## 类型 token 全集（实测 445 表只有 8 种）
//! `int 1752 / string 937 / list 681 / bool 6 / long 5 / double 1`
//! 外加 2 处手滑 `sting`（按 `string` 处理并告警）。
//!
//! ## `list;,=` 语义
//! 分隔符 `;`、键值符 `=`。原 `dbInit()` 字节码做两件事：
//! `reward → rewardJson`、`cid 按 ; 切分 → oppoCids`。
//!
//! ## 子模块
//! - [`raw`]      CSV 解析：schema 抽取 + 行迭代
//! - [`schema`]   类型 token → Rust 类型映射
//! - [`loader`]   全量加载 + 校验（主键唯一 / 外键存在 / 引用完整性）
//! - [`store`]    `TemplateStore`：按表名取 `Arc<Table<T>>`

pub mod loader;
pub mod raw;
pub mod schema;
pub mod store;

pub use loader::TemplateLoader;
pub use store::TemplateStore;
''',
    modules=["loader", "raw", "schema", "store"],
)

crate(
    "data/repo", "ddj-repo", "仓储层：实体 ↔ 表映射，领域层只依赖 trait",
    [
        "ddj-core.workspace       = true",
        "ddj-db.workspace         = true",
        "ddj-cache.workspace      = true",
        "ddj-template.workspace   = true",
        "sqlx.workspace           = true",
        "async-trait.workspace    = true",
        "tokio.workspace          = true",
        "serde.workspace          = true",
        "chrono.workspace         = true",
        "thiserror.workspace      = true",
    ],
    lib='''\
//! # ddj-repo — 仓储实现
//!
//! 对应 Java：`bf.database.*`（639 个类）。
//!
//! ## 依赖倒置
//! 领域层只依赖 **trait**（本 crate 定义），实现放在 `impl` 子模块。
//! 单元测试用内存实现，不需要数据库。
//!
//! ## 子模块
//! - [`traits`]  `PlayerRepo` / `ItemRepo` / `MailRepo` / `GuildRepo` …
//! - [`mysql`]   sqlx 实现
//! - [`memory`]  测试用内存实现
//! - [`entity`]  表行结构体（`#[derive(sqlx::FromRow)]`）

pub mod entity;
pub mod memory;
pub mod mysql;
pub mod traits;

pub use traits::{GuildRepo, ItemRepo, MailRepo, PlayerRepo};
''',
    modules=["entity", "memory", "mysql", "traits"],
)

# ═══════════════════════════════════════════════════════════════════════════
#  DOMAIN
# ═══════════════════════════════════════════════════════════════════════════

DOMAIN_DEPS = [
    "ddj-core.workspace       = true",
    "ddj-repo.workspace       = true",
    "ddj-template.workspace   = true",
    "async-trait.workspace    = true",
    "tokio.workspace          = true",
    "serde.workspace          = true",
    "chrono.workspace         = true",
    "thiserror.workspace      = true",
    "tracing.workspace        = true",
]

for mod_, zh in [
    ("player", "角色：等级 / 经验 / VIP / 属性 / 在线状态"),
    ("item", "道具：背包 / 使用 / 消耗 / 合成 / 限次校验"),
    ("mail", "邮件：系统邮件 / 附件 / 领奖 / 过期清理"),
    ("economy", "经济：货币 / 充值 / 消耗流水 / 反作弊"),
    ("guild", "公会：创建 / 申请 / 职位 / 捐献 / 公会战"),
    ("rank", "排行：榜单快照 / 实时更新 / 结算"),
    ("activity", "活动：开启 / 进度 / 兑换 / 签到"),
    ("battle", "战斗：战报 / 校验 / 回放"),
]:
    crate(
        f"domain/{mod_}", f"ddj-{mod_}", zh, DOMAIN_DEPS,
        lib=f'''\
//! # ddj-{mod_} — {zh}
//!
//! 纯领域层：不依赖 tokio 网络、不直接碰 DB（只通过 `ddj-repo` 的 trait）。
//! 所有状态变更通过聚合根方法完成，便于单元测试与不变量校验。
//!
//! ## 子模块
//! - [`model`]     聚合根与值对象
//! - [`service`]   领域服务（跨聚合的编排）
//! - [`event`]     领域事件（用于副作用解耦）
//! - [`rule`]      不变量与业务规则校验

pub mod event;
pub mod model;
pub mod rule;
pub mod service;
''',
        modules=["event", "model", "rule", "service"],
    )

# ═══════════════════════════════════════════════════════════════════════════
#  APP
# ═══════════════════════════════════════════════════════════════════════════

crate(
    "app/game-core", "ddj-game-core", "应用装配：把 foundation/protocol/net/data/domain 接起来",
    [
        "ddj-core.workspace       = true",
        "ddj-config.workspace     = true",
        "ddj-telemetry.workspace  = true",
        "ddj-runtime.workspace    = true",
        "ddj-codec.workspace      = true",
        "ddj-pb.workspace         = true",
        "ddj-registry.workspace   = true",
        "ddj-rpc.workspace        = true",
        "ddj-session.workspace    = true",
        "ddj-tcp.workspace        = true",
        "ddj-http.workspace       = true",
        "ddj-db.workspace         = true",
        "ddj-cache.workspace      = true",
        "ddj-template.workspace   = true",
        "ddj-repo.workspace       = true",
        "ddj-player.workspace     = true",
        "ddj-item.workspace       = true",
        "ddj-mail.workspace       = true",
        "ddj-economy.workspace    = true",
        "ddj-guild.workspace      = true",
        "ddj-rank.workspace       = true",
        "ddj-activity.workspace   = true",
        "ddj-battle.workspace     = true",
        "tokio.workspace          = true",
        "anyhow.workspace         = true",
        "tracing.workspace        = true",
    ],
    lib='''\
//! # ddj-game-core — 应用装配层
//!
//! 这一层把下层全部接起来，**6 个可执行服务共用**，每个服务只写自己的 `main.rs`
//! 选择要启动哪些模块。
//!
//! ## 对应关系
//! | 本 crate 模块 | Java 侧 |
//! |---|---|
//! | [`bootstrap`] | `GameServer` / `PlatformServer` / `CrossServer` 的 `main` |
//! | [`modules`]   | `serverModuleListConfig.xml` 列出的模块 |
//! | [`wiring`]    | 手写 DI（原系统靠 XML + 反射） |
//!
//! ## 为什么不用 DI 框架
//! Rust 的 trait object + 显式构造足够清晰，且编译期就能发现缺失依赖。
//! 大型 DI 容器在 Rust 里收益远小于成本。

pub mod bootstrap;
pub mod modules;
pub mod wiring;

pub use bootstrap::{Bootstrap, ServiceKind};
''',
    modules=["bootstrap", "modules", "wiring"],
)

crate(
    "app/game-handler", "ddj-game-handler", "103 个 Handler / 1705 个方法的 Rust 落地处",
    [
        "ddj-core.workspace       = true",
        "ddj-pb.workspace         = true",
        "ddj-registry.workspace   = true",
        "ddj-session.workspace    = true",
        "ddj-player.workspace     = true",
        "ddj-item.workspace       = true",
        "ddj-mail.workspace       = true",
        "ddj-economy.workspace    = true",
        "ddj-guild.workspace      = true",
        "ddj-rank.workspace       = true",
        "ddj-activity.workspace   = true",
        "ddj-battle.workspace     = true",
        "async-trait.workspace    = true",
        "tokio.workspace          = true",
        "tracing.workspace        = true",
        "bytes.workspace          = true",
    ],
    lib='''\
//! # ddj-game-handler — 消息处理器
//!
//! 对应 Java：`bf.game.server.message.handler.*`（103 个类 / 1705 个方法）。
//!
//! ## 目录布局（按原包结构一一对应）
//! ```text
//! src/
//!   login.rs         ← LoginMessageHandler       (msgId 3/4/5/6/7/98/11)
//!   item.rs          ← ItemMessageHandler        (11002 ItemUseReq …)
//!   player.rs        ← PlayerMessageHandler
//!   mail.rs          ← MailMessageHandler
//!   guild.rs         ← GuildMessageHandler
//!   rank.rs          ← RankMessageHandler
//!   activity.rs      ← ActivityMessageHandler
//!   battle.rs        ← BattleMessageHandler
//!   ...
//! ```
//!
//! ## 注册方式
//! ```ignore
//! #[handler(11002)]
//! async fn item_use(ctx: &HandlerCtx, req: ItemUseReq11002) -> Result<ItemUseResp11002> {
//!     ctx.player().items().use_item(req).await
//! }
//! ```
//! `#[handler]` 宏展开为 `inventory::submit!`，`HandlerTable` 在启动时收集，
//! cmdId 冲突直接编译失败。
//!
//! ## 返回值约定（复刻原框架）
//! - 返回 `Message` → 框架自动回包
//! - 返回 `()` → handler 自己发包（推送场景）

pub mod macros;

// 以下模块按原包结构 1:1 对应，逐个填充
pub mod account;
pub mod activity;
pub mod battle;
pub mod chat;
pub mod friend;
pub mod gm;
pub mod guild;
pub mod item;
pub mod login;
pub mod mail;
pub mod player;
pub mod rank;
pub mod shop;
pub mod task;
''',
    modules=[
        "account", "activity", "battle", "chat", "friend", "gm", "guild",
        "item", "login", "mail", "player", "rank", "shop", "task",
    ],
)

# ── 6 个可执行服务 ─────────────────────────────────────────────────────
SERVICES = [
    ("svc-platform", "platform", 10001, "平台服：账号 / 登录 / 区服列表 / 充值 / CDK",
     "PlatformServer", ["ddj-http.workspace = true", "ddj-db.workspace = true", "ddj-cache.workspace = true"]),
    ("svc-game", "game", 10544, "游戏服：主逻辑，TCP 长连 + 103 Handler",
     "GameServer", ["ddj-tcp.workspace = true", "ddj-game-handler.workspace = true"]),
    ("svc-cross", "cross", 4544, "跨服服：Hessian RPC + 跨服玩法",
     "CrossServer", ["ddj-rpc.workspace = true", "ddj-tcp.workspace = true"]),
    ("svc-battle", "battle", 12345, "战斗服：战报校验 / 回放存储",
     "BattleServer", ["ddj-http.workspace = true", "ddj-battle.workspace = true"]),
    ("svc-hall", "hall", 20001, "大厅服：http 网关 / 区服聚合",
     "HallServer", ["ddj-http.workspace = true"]),
    ("svc-world", "world", 11544, "世界服：地图 / 分线 / 世界 Boss",
     "WorldServer", ["ddj-tcp.workspace = true"]),
    ("svc-gm", "gm", 1544, "GM 后台：取代 jetty 1544 + data/web 的 11 个 PHP",
     "GmServer", ["ddj-http.workspace = true"]),
]

for dirname, short, port, desc, java, extra in SERVICES:
    deps = [
        "ddj-core.workspace       = true",
        "ddj-config.workspace     = true",
        "ddj-telemetry.workspace  = true",
        "ddj-runtime.workspace    = true",
        "ddj-game-core.workspace  = true",
        *extra,
        "tokio.workspace          = true",
        "anyhow.workspace         = true",
        "tracing.workspace        = true",
    ]
    crate(
        f"app/{dirname}", f"ddj-svc-{short}", f"{desc}（Java: {java}，端口 {port}）",
        deps,
        main=f'''\
//! # ddj-svc-{short} — {desc}
//!
//! 对应 Java 进程：`{java}`（{java.replace("Server", "").lower()} 端口 {port}）
//!
//! 本文件只做三件事：读配置 → 装配 → 启动。
//! 所有逻辑都在 `ddj-game-core` 与各领域 crate 里，方便多服务复用。

use anyhow::Result;
use ddj_core::ServiceKind;
use ddj_game_core::Bootstrap;

#[tokio::main]
async fn main() -> Result<()> {{
    Bootstrap::new(ServiceKind::{java.replace("Server", "")})
        .with_config_dir("config")
        .run()
        .await
}}
''',
    )

# ═══════════════════════════════════════════════════════════════════════════
#  写入
# ═══════════════════════════════════════════════════════════════════════════


def write() -> None:
    for c in CRATES:
        d = ROOT / "crates" / c["path"]
        (d / "src").mkdir(parents=True, exist_ok=True)
        is_bin = c["main"] is not None
        (d / "Cargo.toml").write_text(
            manifest(c["name"], c["desc"], c["deps"], bin=is_bin, extra=c["extra"]),
            encoding="utf-8",
        )
        lib = c["lib"]
        if lib:
            (d / "src" / "lib.rs").write_text(lib, encoding="utf-8")
        elif not is_bin:
            (d / "src" / "lib.rs").write_text(
                f"//! # {c['name']} — {c['desc']}\n", encoding="utf-8")
        if is_bin:
            (d / "src" / "main.rs").write_text(c["main"], encoding="utf-8")

        for m in c["modules"]:
            if m == "macros":
                continue
            f = d / "src" / f"{m}.rs"
            if not f.exists():
                f.write_text(
                    f"//! TODO: {c['name']}::{m}\n", encoding="utf-8")

    print(f"wrote {len(CRATES)} crates")


if __name__ == "__main__":
    write()
