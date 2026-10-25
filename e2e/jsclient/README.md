# 无头客户端（JS）— 用真实客户端代码测服务端

跑**反编译出来的客户端 bundle**，让它像真玩家一样连服务端、收发包。不是重写协议：
帧格式由客户端自己的 `lets.ByteBuffer` / `lets.WebPacket` 完成，消息体由客户端自己的
`letsproto.proto` 文本编码。这样"服务端是否与客户端一致"这个问题才真的有证明力。

## 为什么这么分

| 环节 | 用什么 | 原因 |
|---|---|---|
| 帧头 `int16 magic + int32 total + int32 cmdId` | `lets.ByteBuffer`（客户端原类） | 端序/偏移一旦自己写就等于自己批改自己 |
| 消息体 protobuf | 客户端 `letsproto.proto` 文本 + protobufjs | 见下方"为什么不用 `lets.ProtoBufExt`" |
| 登录包（cmd 4） | `xxtea.js`（与客户端/服务端逐位一致） | 只有 cmd 4 走 XXTEA+Base64，20 行足够，且已 47 组向量对拍 |

### 为什么不用 `lets.ProtoBufExt`

`lets.ProtoBufExt.parseMessage(text)` 内部依赖一个模块级的 `{method, cmdId}` 清单，
而该模块在懒加载下没被初始化，调用必抛
`Cannot read properties of undefined (reading 'forEach')`；
它持有的 `Builder` 是 protobufjs 的 Parser 中间态，`lookup` 恒返回 `null`。
所以消息体改走"同一个 proto 文本 + 标准 protobufjs"，cmdId 由消息名后缀取
（`ItemUseReq_11002` → 11002），与服务端 `MessageUtil.getMessageID` 规则一致。

## 准备

```bash
# 1) bundle 不在仓库里（2.5 MB，从 APK 解出即可）
export DDJ_CLIENT_ASSETS=/path/to/apk/解出的/assets/assets
# 目录下应有 main/index.js（如用子包再加 subpackage1..3）

# 2) 依赖
npm install
```

`vendor/letsproto.proto`（1.5 MB）已入库，来自
`assets/resources/import/0f/0f543d2e-…json` 的 `[5][0][2]`（cc.TextAsset）。

## 用法

```bash
node run.js login      --open-id open-001   # 默认 WS 传输
node run.js login      --open-id open-001 --transport tcp   # 裸帧，给自建服务端调试
node run.js create     --open-id open-001 --name 测试角色
node run.js handshake  --open-id open-001          # 登录 + cmd16 挑战回执
node run.js item-use   --open-id open-001 --template 400001 --num 1
node run.js flow       --open-id open-001 --name 测试角色   # 登录→(创角)→登录→握手
```

常用参数：`--host` / `--port`（默认 `127.0.0.1:10544`）、`--server-id`（默认 `7010544`）、
`--channel-id`、`--version`、`--password`（跳过本地推导，直接给定握手密码）。

## 已验证（对真实服务端，非 mock）

`node run.js flow --open-id open-js-003 --name 无头测试丙`：

```text
客户端 bundle 已加载：lets 导出 110 个
已连接 127.0.0.1:10544
LoginResp_4 = {"ret":0,"ok":true,...,"loginvar":1968}
LoginResp_4 = {"ret":0,"ok":true,...,"loginvar":2025}
握手通过（无回包 = 原版语义）pwd = 03287fcce194dbd958c2ec5b33705912
```

| 检查点 | 结果 |
|---|---|
| bundle 在 Node 里能加载 | ✅ `lets` 导出 110 个 |
| 帧头由客户端类生成 | ✅ 与 `work/build_packet.py` 逐字节一致 |
| `ItemUseReq_11002{400001,3,{talentId:12345}}` 编码 | ✅ `0881b51810031a0308d960`… 实测 `0881b51810031a0308b960`，与历史抓包完全相同 |
| 登录 cmd 4（XXTEA+Base64 请求） | ✅ `ret=0 ok=false`（无角色） |
| 创角 cmd 6 | ✅ `ret=0` |
| 二次登录 | ✅ `ok=true` + `loginvar` 挑战种子 |
| 握手 cmd 16（客户端算法推密码） | ✅ 通过；**原版不回包**，等超时即成功 |
| 道具 cmd 11002 | ⏳ 服务端尚未接线该 cmdId（无回包），见下 |

## 传输层（重要）

原版 10544 是 **WebSocket** 服务（`com.jery.ngsp.network.websocket.WebSocketServer`），裸 TCP 直连会返回 HTTP 400。
所以 `client.js` 默认走 `ws://`，帧内容不变（同样 10 字节头 + body，作为二进制消息）。
Rust 端目前只做 TCP accept —— 差分结果见 `DIFF.md`。

## 已知边界

1. **未接线的 cmdId 静默无回包**。`pipeline` 对未注册 cmdId 返回 `Ok(None)` 且只打 debug 日志。
   目前 `HandlerTable` 里只有 login / create / 16,17,18 / 1103 / 1111 / 1112，
   背包(`11002`等)、签到、琥珀、邮件都还是纯函数未接线 —— 这条 harness 就是接下来验证接线的手段。
2. 登录响应是**明文 protobuf**（实测 `08 00 10 00`），服务端只解密请求方向。
   `login()` 里先按明文解，失败才回退 XXTEA。
3. `vendor/assets/` 不入库；不带 `DDJ_CLIENT_ASSETS` 时回退到 `vendor/assets` 并给出明确报错。

## 目录

```text
boot.js     在 node:vm 里加载 bundle（Proxy 兜底 cc，window/self/globalThis 对齐）
client.js   Client：帧组装（客户端 ByteBuffer）+ TCP + 分帧 + call/超时
proto.js    proto 文本 → protobufjs root；按后缀建 cmdId↔消息名索引
xxtea.js    XXTEA + Base64（仅 cmd 4）
run.js      CLI：login / create / handshake / item-use / flow
vendor/letsproto.proto   客户端协议 schema（入库）
```
