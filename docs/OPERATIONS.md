# 运维手册

单二进制 `ddj` 的运行、守护、接回控制台与排障。

---

## 1. 为什么是一个二进制

原服务端是 6 个 JVM 进程 + 1 个 PHP 后台，靠 `start.sh` 里 6 行 `java -jar -Xms.. -Xmx..` 拉起。
现在这 7 个"服务"只是同一个可执行文件的 7 个**服务模块**（`ServiceKind`），
进程边界变成**部署选择**，不是代码结构：

| 想要 | 怎么做 |
|---|---|
| 一个模块一进程（推荐，故障隔离） | 分别 `ddj start game`、`ddj start platform` … |
| 本地起全套 | `for s in platform game cross hall battle world gm; do ddj start $s; done` |
| 交给 systemd / docker / k8s | 用 `ddj run <service>`（前台不 fork） |

**`run` / `start` 只接受单个服务，这是刻意的**：一进程多服务会让崩溃面、
日志、CPU 配额全部混在一起，排障成本远大于省下的几个进程。
多实例由进程管理器负责（systemd 模板 unit / docker compose / k8s Deployment）。

好处仍然是单二进制：配置解析、协议编解码、静态表加载、连接池、日志栈
只有一份实现，改一次 7 个服务同时生效；部署粒度交给运维决定。

管理类命令（`stop` / `status`）支持 `all` 与逗号列表，一次操作一批。

---

## 2. 运行模式

### 2.1 前台（`ddj run`）

```bash
ddj run game
```

- 不 fork、不写 pid 文件、日志走 stdout。
- **专门给进程管理器用**：systemd `Type=simple`、docker `CMD`、k8s `command`。
- `SIGTERM` / `SIGINT` → 走优雅停机（关监听 → 停模块 → 关连接池）。

### 2.2 后台守护（`ddj start`）

```bash
ddj start game          # 每个模块一个守护进程；全套见 §7
```

做的事：

1. `fork()` 出子进程；
2. 子进程 `setsid()` → **新建会话，成为会话首进程，脱离控制终端**；
3. `chdir("/")`、`umask(0o027)`、stdin/stdout/stderr 全部重定向到日志文件；
4. 写 `<run_dir>/<service>.pid`，监听 `<run_dir>/<service>.sock`（Unix socket，`0600`）；
5. 父进程立即返回，打印 pid / 日志路径 / 接回方式。

关键点在于第 2 步：因为有了自己的会话、且没有控制终端，
**登录 shell 收到 `SIGHUP` 时不会波及它** —— SSH 断线、终端关掉、
甚至换一台机器连进来，进程都还在。

验证：

```bash
PID=$(cat /tmp/ddj-$(id -u)/game.pid)
ps -o pid,ppid,sid,tty,stat,cmd -p $PID
#    PID    PPID     SID TT       STAT CMD
#  39219       1   39219 ?        Ssl  ddj start game
```

- `PPID=1` —— 已过继给 init，与启动它的 shell 无父子关系；
- `SID=自己的 PID` —— 自己是会话首进程，没有控制终端；
- `TT= ?` —— 没有控制终端，`SIGHUP` 无处可达。

### 2.3 运行目录

| 项 | 路径 | 说明 |
|---|---|---|
| 默认 run 目录 | `/tmp/ddj-<uid>` | 免 root 可写 |
| 覆盖 | `DDJ_RUN_DIR=/var/run/ddj ddj start game` | 生产建议放 `/var/run` |
| pid | `<run_dir>/<service>.pid` | `0600` |
| 控制 socket | `<run_dir>/<service>.sock` | `srwx r-x ---` |
| 日志 | `<run_dir>/<service>.log` | 追加写 |

> `/tmp` 在部分发行版会被 `systemd-tmpfiles` 定期清理。生产上务必
> `DDJ_RUN_DIR=/var/run/ddj`，或在 `/etc/tmpfiles.d/` 里加保留规则。

---

## 3. 接回控制台（`ddj attach`）

```bash
ddj attach game
```

- 连上 Unix socket，**先回放最近 2000 行**（`--tail N` 可调），再实时跟流；
- **`Ctrl-C` 只是断开查看，不会停服务** —— 这一点和 `docker attach` 不同，
  比 `docker attach` 安全（后者 `Ctrl-C` 会杀进程）；
- 换终端、换 SSH 会话、隔几小时再来，行为一致。

实现要点：守护进程把每条日志同时写进
①日志文件 ②内存环形缓冲（2000 行）③广播给所有已 attach 的客户端。
所以 attach 不是 `tail -f` 日志文件 —— 日志被轮转/清空也不影响接回。

---

## 4. 命令一览

```text
ddj run     <service>     前台运行（不 fork），给 systemd/docker 用
ddj start   <service>     后台守护（setsid）
ddj stop    <service|all> 优雅停机：控制 socket → 等待 → 超时 SIGKILL
ddj restart <service>     stop + start
ddj status  <service|all> 单服务/全部：pid / 运行时长 / 在线 / 处理器 / 配置表
ddj list                  全部服务一览
ddj logs    <service>     打印日志；-f 跟踪，-n 指定行数
ddj attach  <service>     接回控制台（回放 + 实时），Ctrl-C 仅断开
```

`<service>` 取值：`game` `platform` `cross` `hall` `battle` `world` `gm`。
`stop` / `status` 额外接受 `all` 与逗号列表（如 `game,platform`）；
`stop all` 会**跳过没在运行的服务**，不视为失败。

状态列含义：

| 列 | 来源 |
|---|---|
| PID | pid 文件 + `kill(pid, 0)` 活性校验 |
| 运行时长 | pid 文件 mtime 起算（不依赖 `/proc`） |
| 在线 / 处理器 / 配置表 | 控制 socket 的 `status` 请求实时回报 |

---

## 5. 配置

```text
config/<env>/<service>.toml   ← 环境覆盖（如 config/dev/game.toml）
config/<service>.toml         ← 服务基线
```

优先级（低 → 高）：**内置默认 → `config/<service>.toml` → `config/<env>/<service>.toml` → 环境变量**。

环境变量用双下划线表示层级：

```bash
export DDJ__DB__GAME_URL='mysql://root:pw@127.0.0.1:3306/game_7010544?ssl-mode=DISABLED'
export DDJ__TEMPLATE__DIR=/srv/ddj/csv
export DDJ__TELEMETRY__LOG_FORMAT=pretty
```

环境选择：`DDJ_ENV=dev|staging|prod`（默认 `dev`）。

### 5.1 MySQL 连接串必须带 `ssl-mode=DISABLED`

**MySQL 5.7 开了内部 SSL，但与 sqlx 默认（rustls + TLS1.3）握手不兼容**，
表现为启动即失败：

```text
连接数据库失败: error communicating with database:
received fatal alert: HandshakeFailure: received fatal alert: HandshakeFailure
```

两种解法，任选其一：

1. 连接串加 `?ssl-mode=DISABLED`（本项目 `config/*.toml` 已默认这样写）；
2. 服务端升级到 MySQL 8 并确认 TLS 版本，或改用 Unix socket 连接。

内网/同机部署推荐方案 1，简单且无额外开销。

---

## 6. 排障

### 6.1 `ddj start` 报了 pid，但 `ddj list` 显示"未运行"

守护进程启动早期就失败并已自动清理 pid。看日志：

```bash
tail -50 /tmp/ddj-$(id -u)/game.log
```

因为子进程的 stdout/stderr 全部重定向到日志文件，**日志里一定有原因**
（启动失败会显式 `ERROR 守护进程启动失败 error=...`）。

### 6.2 `ddj stop` 报"10 秒内未退出"

| 现象 | 原因 | 处理 |
|---|---|---|
| 停不掉，日志也没有"收到控制连接停止指令" | 控制 socket 连不上（权限/路径不一致） | 确认 `ddj stop` 与 `ddj start` 用同一个 `DDJ_RUN_DIR` |
| 日志有"收到…停止指令"，但卡在"服务停止中" | 某模块 `stop()` 挂住 | 看该模块日志；超时后 `ddj stop` 会补 `SIGKILL` |
| pid 文件在、进程不在 | 强杀或 OOM 后残留 | `ddj stop` 会自动清理残留 pid |

### 6.3 状态显示"进程在 / 控制通道断开"

进程活着但 socket 不应答 —— 通常是启动过程中被 `SIGSTOP`、
或日志写入阻塞。`ddj attach` 可能也接不上，此时直接看日志文件。

---

## 7. 生产部署示例

### 7.1 systemd

```ini
# /etc/systemd/system/ddj-game.service
[Unit]
Description=ddj game server
After=network-online.target mysql.service redis.service
Wants=network-online.target

[Service]
Type=simple
User=ddj
WorkingDirectory=/srv/ddj
Environment=DDJ_ENV=prod
Environment=DDJ_RUN_DIR=/var/run/ddj
ExecStart=/srv/ddj/bin/ddj run game
Restart=on-failure
RestartSec=3
LimitNOFILE=65535
KillSignal=SIGTERM
TimeoutStopSec=30

[Install]
WantedBy=multi-user.target
```

用 `run` 而不是 `start`：systemd 自己就是守护进程管理器，
不需要再套一层 fork（套了反而 systemd 追踪不到真实 pid）。

### 7.2 docker

```dockerfile
FROM debian:bookworm-slim
COPY bin/ddj /usr/local/bin/ddj
COPY config /srv/ddj/config
COPY csv    /srv/ddj/csv
WORKDIR /srv/ddj
ENV DDJ_ENV=prod DDJ_RUN_DIR=/var/run/ddj
# 单容器跑全部；要拆就一个服务一个副本，改 CMD 参数即可
CMD ["ddj", "run", "all"]
```

同样用 `run`：容器里 PID 1 必须是真正的服务进程，才能正确收到 `SIGTERM`。

---

## 8. 与旧栈的对应

| 旧 | 新 |
|---|---|
| `start.sh` 里 6 行 `java -jar -Xms.. -Xmx..` | 6 个 `ddj run <svc>`（systemd 模板 unit） |
| `nohup ... &` + `kill <pid>` | `ddj start` / `ddj stop` |
| `tail -f logs/game.log` | `ddj attach game`（回放 + 实时，不怕轮转） |
| 改堆参数重启 | 无 JVM；内存靠连接池与背压控制 |
| 6 份 `applicationContext.xml` | 1 份 `config/<service>.toml` |
