//! 基于**客户端自己的类**收发包。
//!
//! ## 帧格式（来自 bundle 里的 `lets.WebPacket` / `ByteBuffer`，全大端）
//! ```text
//! int16 magic = 29099 (0x71AB)
//! int32 total   = 10 + body.length
//! int32 cmdId
//! bytes body
//! ```
//!
//! ## 传输层：原版是 WebSocket，不是裸 TCP
//! 原版 `ddj-game` 的端口 10544 是 `com.jery.ngsp.network.websocket.WebSocketServer`——
//! 直接连上去发裸帧会收到 `HTTP/1.1 400`（实测 `bad magic 0x4854` = "HT"）。
//! 必须 `ws://host:port` 完成握手，然后把**同一套 10 字节头 + body** 当作
//! **二进制消息**（`binaryType='arraybuffer'`）收发。
//!
//! 因此本文件默认走 WebSocket；`{ transport: 'tcp' }` 保留给自建服务端做裸帧调试。

'use strict';

const MAGIC = 29099;
const HEADER_LEN = 10;

class Client {
  /**
   * @param {{lets: object}} env boot() 的返回值
   * @param {{transport?: 'ws'|'tcp'}} [opts]
   */
  constructor(env, opts = {}) {
    this.lets = env.lets;
    this.transport = opts.transport || 'ws';
    this.sock = null; // tcp
    this.ws = null; // websocket
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
    const off = buf && buf.byteOffset ? buf.byteOffset : 0;
    const len = buf && buf.length !== undefined ? buf.length : HEADER_LEN + body.length;
    return Buffer.from(ab).subarray(off, off + len);
  }

  /** 拆帧：返回 {cmdId, body, total} 或 null（数据不够）。 */
  static parse(buf) {
    if (buf.length < HEADER_LEN) return null;
    const magic = buf.readInt16BE(0);
    if (magic !== MAGIC) {
      const hint = buf.length >= 4 && buf[0] === 0x48 ? '（看起来是 HTTP 响应：该端口要 WebSocket 握手，用 transport="ws"）' : '';
      throw new Error(`bad magic 0x${magic.toString(16)}${hint}`);
    }
    const total = buf.readInt32BE(2);
    if (buf.length < total) return null;
    return { cmdId: buf.readInt32BE(6), body: buf.subarray(HEADER_LEN, total), total };
  }

  connect(host = '127.0.0.1', port = 10544, timeoutMs = 8000) {
    return this.transport === 'tcp' ? this._connectTcp(host, port, timeoutMs) : this._connectWs(host, port, timeoutMs);
  }

  _connectWs(host, port, timeoutMs) {
    return new Promise((resolve, reject) => {
      const ws = new WebSocket(`ws://${host}:${port}`);
      ws.binaryType = 'arraybuffer';
      const timer = setTimeout(() => {
        try { ws.close(); } catch { /* ignore */ }
        reject(new Error(`WebSocket 连接 ${host}:${port} 超时`));
      }, timeoutMs);
      ws.onopen = () => {
        clearTimeout(timer);
        this.ws = ws;
        resolve(this);
      };
      ws.onerror = (e) => {
        clearTimeout(timer);
        reject(new Error(`WebSocket 错误: ${e.message || e.type}`));
      };
      ws.onmessage = (ev) => this._onData(Buffer.from(ev.data));
      ws.onclose = () => { this.ws = null; };
    });
  }

  _connectTcp(host, port, timeoutMs) {
    const net = require('net');
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

  _write(buf) {
    if (this.ws) this.ws.send(new Uint8Array(buf));
    else if (this.sock) this.sock.write(buf);
    else throw new Error('尚未连接');
  }

  /** 发一帧并等下一帧回包。 */
  async call(cmdId, body = Buffer.alloc(0), timeoutMs = 5000) {
    const wait = new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error(`cmd ${cmdId} 等待回包超时`)), timeoutMs);
      this.waiters.push((f) => {
        clearTimeout(timer);
        resolve(f);
      });
    });
    this._write(this.frame(cmdId, body));
    return wait;
  }

  close() {
    if (this.ws) { try { this.ws.close(); } catch { /* ignore */ } this.ws = null; }
    if (this.sock) { this.sock.destroy(); this.sock = null; }
  }
}

module.exports = { Client, MAGIC, HEADER_LEN };
