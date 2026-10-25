# 差分测试记录：原版 Java 服务端 vs Rust 移植

原版用根目录 `docker-compose.yml` 起（`db`+`redis`+`platform`+`game`，全部 `network_mode: host`）。
无头客户端 `node run.js ...` 打两边，比字节、比错误码、比副作用。

## 环境准备（踩过的坑）

| 项 | 结论 |
|---|---|
| 起哪些服务 | 只起 `db redis platform game` 就够（`cross`/`web` 可省内存）；`game` 约 365 MB RSS，MySQL 41 MB |
| 端口冲突 | 原版 `game` 占 10544，必须先停 Rust 版 `ddj`；原版 `redis` 占 6379，需先删掉 Rust 版的 `ddj-redis` 容器 |
| 白名单 | `platform.bf_white_openid(openId, createTime)` 加行后才跳过版本/维护/注册开关/token/sign 校验；**改完必须重启 platform 和 game**（`isWhite` 有缓存） |
| 语言 | 原版 `LANGUAGE_TYPE_LIST` 只有 **zh_CN / en**；不带 `lan` 会走 `getDefaultLanguageType()`，若不在列表里 `PlayerNameMgr.map.get()` 返回 null → 登录 NPE 且**不回包**（客户端只看到超时） |
| 创角 | `CreatePlayerReq_6` **必须带 `lan`**：`WxMsgSecCheck.checkDirtyWord` 会 `languageType.getDesc()`，lan 缺省 → null → NPE → 被 catch 成 `ret=2`（服务器繁忙），且 `player` 表零行 |

## 已确认的行为差异（Rust 需要对齐）

### 1. 传输层：原版是 **WebSocket**，不是裸 TCP ★最重要

`ddj-game` 的 10544 是 `com.jery.ngsp.network.websocket.WebSocketServer`。
裸 TCP 直连发帧会收到 `HTTP/1.1 400`——客户端表现为 `bad magic 0x4854`（`0x4854` = `"HT"`）。

正确形态：`ws://host:10544` 握手，`binaryType='arraybuffer'`，**同一套 10 字节头 + body 当二进制消息**收发。

> 影响：Rust 端目前只做 TCP accept，真实客户端（含现在的无头客户端）连不上。要么在 `ddj-tcp` 前面加 WebSocket 升级握手，要么换成 WS 编解码层。

### 2. 登录 `ok=false` 要带 10 个随机候选名

原版：
```json
{"ret":0,"ok":false,"names":["瞿天罡","荀浩波","严丞魁","昝永怡","公西正浩","东方松烟","孔淇","江嘉德","牧英朗","牧幻翠"]}
```
名字来自 `PlayerNameMgr.randomNames(languageType)`：`W_玩家名字_PlayerName.csv` 的姓 × 名笛卡尔积（实测 zh_CN 可用名 **306000** 个）。

Rust 现状：`LoginResp_4` 只回了 `ret=0, ok=false`，**没有 names** → 客户端创角页没有推荐名。

### 3. 「游客关闭 + 无 token + 非白名单」→ 回 **ret=1（维护中）**

`WorkTaskPlayerSignIn.run()`：
```java
if (!(isTourists() || white || (token != null && !token.isEmpty()))) {
    sendRet(COMMON_SERVER_WEIHU);   // 1，不是 106
    return;
}
```
语义上是「维护中」，非常容易被误判成真的维护。Rust 侧要照抄这个码。

### 4. 登录失败路径不发包

`PlayerNameMgr.randomNames` 在 `map.get(lang)` 为 null 时 NPE，异常从 work task 逃出、**没有任何回包**。这是原版缺陷，Rust 不需要复刻，但差分脚本要能把「超时」和「回了错码」区分开。

## 复现命令

```bash
# 1) 原版
cd /home/ubuntu/code/ddj
docker compose up -d db redis platform game
docker exec ddj-db mysql -uroot -p"$DB_PASSWORD" platform -e \
  "insert ignore into bf_white_openid(openId,createTime) values('orig-ws-001',0)"
docker restart ddj-platform && sleep 12 && docker restart ddj-game

# 2) 无头客户端（WS 传输是默认值）
cd rust/e2e/jsclient
export DDJ_CLIENT_ASSETS=../../../work/mod/assets/assets
node run.js flow --open-id orig-ws-001 --name 原版甲

# 3) 换到 Rust 端（先停原版 game 释放 10544）
node run.js login --open-id open-001 --transport tcp
```

## 待办

- [ ] Rust 端补 WebSocket 升级（否则真客户端无法连接）
- [ ] 登录 `ok=false` 返回 10 个随机候选名（含 306000 名的生成规则）
- [ ] 「非游客 + 无 token」回 `ret=1`
- [ ] 原版创角在 `lan` 缺失时的 NPE 不复刻，但差分脚本需标记为「原版已知缺陷」
- [ ] 原版 `RedisPlayerPushTask` / `PlayerOfflineDataServerModule` 每 100 ms 刷错误日志，属该手工端环境缺 Redis 排行数据，与协议无关
