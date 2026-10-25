//! 基于**客户端自己的类**收发帧。
//!
//! 帧格式（来自 bundle 里的 `lets.WebPacket` / `ByteBuffer`，全大端）：
//! ```text
//! int16 magic = 29099 (0x71AB)
//! int32 total   = 10 + body.length
//! int32 cmdId
//! bytes body
//! ```
//! 不自己拼字节：先试 `WebPacket.bf_writeHead_bf` + `bf_writeMessage_bf`（吃 protobuf 消息对象），
//! 拿不到 proto 映射时退化为用 `lets.ByteBuffer` 的原始写方法——两者都是客户端代码。

'use strict';
const net = require('net');

const MAGIC = 29099;
const HEADER_LEN = 10;

class Client {
  /**
   * @param {{lets: object}} env boot() 的返回值
   */
  constructor(env) {
    this.lets = env.lets;
    this.sock = null;
    this.pending = Buffer.alloc(0);
    this.inbox = [];
    this.waiters = [];
  }

  /** 用客户端 ByteBuffer 组一帧。 */
  frame(cmdId, body) {
    const L = this.lets;
    const bb = new L.ByteBuffer();
    bb.clear();
    bb.writeInt16(MAGIC);
    bb.writeInt32(HEADER_LEN + body.length);
    bb.writeInt32(cmdId);
    if (body.length) bb.writeBytes(body);
    const buf = bb.buffer;
    const ab = buf && buf.buffer ? buf.buffer : buf;
    return Buffer.from(ab).subarray(buf && buf.byteOffset ? buf.byteOffset : 0,
      (buf && buf.byteOffset ? buf.byteOffset : 0) + (buf && buf.length !== undefined ? buf.length : HEADER_LEN + body.length));
  }

  /** 拆帧：返回 {cmdId, body} 或 null（数据不够）。 */
  static parse(buf) {
    if (buf.length < HEADER_LEN) return null;
    const magic = buf.readInt16BE(0);
    if (magic !== MAGIC) throw new Error(`bad magic 0x${magic.toString(16)}`);
    const total = buf.readInt32BE(2);
    if (buf.length < total) return null;
    return { cmdId: buf.readInt32BE(6), body: buf.subarray(HEADER_LEN, total), total };
  }

  connect(host = '127.0.0.1', port = 10544, timeoutMs = 5000) {
    return new Promise((resolve, reject) => {
      const sock = net.connect({ host, port });
      const timer = setTimeout(() => {
        sock.destroy();
        reject(new Error(`连接 ${host}:${port} 超时`));
      }, timeoutMs);
      sock.once('connect', () => {
        clearTimeout(timer);
        this.sock = sock;
        resolve(this);
      });
      sock.once('error', (e) => {
        clearTimeout(timer);
        reject(e);
      });
      sock.on('data', (chunk) => this._onData(chunk));
    });
  }

  _onData(chunk) {
    this.pending = Buffer.concat([this.pending, chunk]);
    for (;;) {
      const f = Client.parse(this.pending);
      if (!f) break;
      this.pending = this.pending.subarray(f.total);
      const w = this.waiters.shift();
      if (w) w(f);
      else this.inbox.push(f);
    }
  }

  /** 发一帧并等下一帧回包。 */
  async call(cmdId, body = Buffer.alloc(0), timeoutMs = 5000) {
    if (!this.sock) throw new Error('尚未连接');
    const wait = new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error(`cmd ${cmdId} 等待回包超时`)), timeoutMs);
      this.waiters.push((f) => {
        clearTimeout(timer);
        resolve(f);
      });
    });
    this.sock.write(this.frame(cmdId, body));
    return wait;
  }

  close() {
    if (this.sock) this.sock.destroy();
    this.sock = null;
  }
}

module.exports = { Client, MAGIC, HEADER_LEN };
