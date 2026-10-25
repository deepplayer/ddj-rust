//! 消息体编解码：用**客户端自己的 proto 文本**，由标准 protobufjs 解析。
//!
//! 为什么不直接用 bundle 里的 `lets.ProtoBufExt`：它的 `parseMessage(text)` 依赖
//! 一个模块级的消息清单（`{method, cmdId}` 数组），而该模块在懒加载下未被初始化，
//! 会抛 `Cannot read properties of undefined (reading 'forEach')`；
//! 它内部持有的 `Builder` 是 protobufjs 的 Parser 中间态，`lookup` 恒返回 null。
//!
//! 所以分工是：
//! - **帧格式** → 客户端原类 `lets.ByteBuffer` / `lets.WebPacket`（见 client.js）
//! - **消息体**   → 客户端 proto 文本 + protobufjs
//!
//! cmdId 从消息名后缀取（`ItemUseReq_11002` → 11002），这与服务端
//! `MessageUtil.getMessageID` 的规则完全一致。

'use strict';
const fs = require('fs');
const path = require('path');
const protobuf = require('protobufjs');

let cached = null;

/** 客户端 proto 文本的位置：优先环境变量，其次 vendor。 */
function protoPath() {
  return process.env.DDJ_CLIENT_PROTO
    || path.resolve(__dirname, 'vendor/letsproto.proto');
}

/**
 * 加载 proto，返回 { root, type(name), cmdIdOf(name), byCmd(cmdId) }。
 */
function load(file = protoPath()) {
  if (cached) return cached;
  const text = fs.readFileSync(file, 'utf8');
  const parsed = protobuf.parse(text, { keepCase: false });
  const root = parsed.root;

  /** 名字 → cmdId（后缀规则，与服务端 `MessageUtil.getMessageID` 一致） */
  const nameToCmd = new Map();
  const cmdToReq = new Map();
  const cmdToResp = new Map();

  // 直接从 proto 文本索引消息名：不依赖 protobufjs 的内部结构（其 nested 在各版本不一致）
  for (const m of text.matchAll(/^\s*message\s+([A-Za-z0-9_]+)\s*\{/gm)) {
    const name = m[1];
    const cmd = /_(\d+)$/.exec(name);
    if (!cmd) continue;
    const id = Number(cmd[1]);
    nameToCmd.set(name, id);
    if (name.includes('Resp')) cmdToResp.set(id, name);
    else cmdToReq.set(id, name);
  }

  cached = {
    root,
    type: (name) => root.lookupType(name.startsWith('pb.') ? name : `pb.${name}`),
    cmdIdOf: (name) => nameToCmd.get(name),
    reqName: (cmd) => cmdToReq.get(cmd),
    respName: (cmd) => cmdToResp.get(cmd),
    stats: () => ({ messages: nameToCmd.size, reqs: cmdToReq.size, resps: cmdToResp.size }),
  };
  return cached;
}

/** 编码：对象 → Buffer */
function encode(typeName, obj) {
  const t = load().type(typeName);
  return Buffer.from(t.encode(t.create(obj)).finish());
}

/** 解码：Buffer → 普通对象（带字段名，默认值不填充） */
function decode(typeName, buf) {
  const t = load().type(typeName);
  return t.toObject(t.decode(buf), { defaults: false, longs: Number, enums: Number });
}

module.exports = { load, encode, decode, protoPath };
