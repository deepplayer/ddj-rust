//! XXTEA + Base64 —— 登录包（cmdId 4）的登录专用封装。
//!
//! 与客户端 `nat_module.js` 内 XXTEA 逐位一致，且与 Rust 侧
//! `crates/protocol/codec/src/xxtea.rs` 用 47 组向量双向对拍通过。
//!
//! 只有 cmdId == 4 走这条加密路径（服务端 `GameServer` 里写死的）。

'use strict';

const DELTA = 0x9e3779b9;
const M = 0xffffffff;
/** 服务端 `XXeta.SIGN_KEY` 与客户端 `LoginCmdId=4` 的密钥，两边完全相同 */
const SIGN_KEY = Buffer.from('05:&()^&*abddzMQ', 'ascii');

const K = new Uint32Array(4);
for (let i = 0; i < 4; i++) K[i] = SIGN_KEY.readUInt32LE(i * 4);

function toWords(data, includeLen) {
  const n = Math.floor((data.length + 3) / 4) + (includeLen ? 1 : 0);
  const v = new Uint32Array(n);
  for (let i = 0; i < data.length; i++) {
    v[i >> 2] = (v[i >> 2] | (data[i] << ((3 & i) << 3))) >>> 0;
  }
  if (includeLen) v[n - 1] = data.length;
  return v;
}

function fromWords(v, includeLen) {
  let n = v.length * 4;
  if (includeLen) {
    const m = v[v.length - 1];
    n -= 4;
    if (m < Math.max(n - 3, 0) || m > n) return null;
    n = m;
  }
  const out = Buffer.alloc(n);
  for (let i = 0; i < n; i++) {
    out[i] = (v[i >> 2] >>> ((3 & i) << 3)) & 0xff;
  }
  return out;
}

function mx(s, y, z, p, e) {
  const a = ((((z >>> 5) ^ ((y << 2) >>> 0)) + (((y >>> 3) ^ ((z << 4) >>> 0)) >>> 0)) >>> 0)
    ^ (((s ^ y) + (K[(3 & p) ^ e] ^ z)) >>> 0);
  return a >>> 0;
}

function enc(v) {
  const n = v.length;
  if (n < 2) return;
  const p = n - 1;
  const q = 6 + Math.floor(52 / n);
  let s = 0;
  let z = v[n - 1];
  for (let r = 0; r < q; r++) {
    s = (s + DELTA) >>> 0;
    const e = (s >>> 2) & 3;
    for (let i = 0; i < p; i++) {
      const y = v[i + 1];
      z = (v[i] + mx(s, y, z, i, e)) >>> 0;
      v[i] = z;
    }
    const y = v[0];
    z = (v[p] + mx(s, y, z, p, e)) >>> 0;
    v[p] = z;
  }
}

function dec(v) {
  const n = v.length;
  if (n < 2) return;
  const p = n - 1;
  const q = 6 + Math.floor(52 / n);
  let s = (q * DELTA) >>> 0;
  let y = v[0];
  while (s) {
    const e = (s >>> 2) & 3;
    for (let i = p; i > 0; i--) {
      const z = v[i - 1];
      y = (v[i] - mx(s, y, z, i, e)) >>> 0;
      v[i] = y;
    }
    const z = v[p];
    y = (v[0] - mx(s, y, z, 0, e)) >>> 0;
    v[0] = y;
    s = (s - DELTA) >>> 0;
  }
}

/** 加密（末尾附长度字） */
function encrypt(data) {
  const v = toWords(data, true);
  enc(v);
  const out = Buffer.alloc(v.length * 4);
  for (let i = 0; i < v.length; i++) out.writeUInt32LE(v[i], i * 4);
  return out;
}

/** 解密（读取末尾长度字并截断）；失败返回 null */
function decrypt(data) {
  const v = toWords(data, false);
  if (v.length < 2) return null;
  dec(v);
  return fromWords(v, true);
}

/** 登录包 body：protobuf → Base64 文本 → XXTEA */
function wrapLogin(protobufBody) {
  return encrypt(Buffer.from(protobufBody.toString('base64'), 'ascii'));
}

/** 登录响应 body：XXTEA 解密 → Base64 解码 */
function unwrapLogin(cipher) {
  const b64 = decrypt(cipher);
  if (!b64) return null;
  return Buffer.from(b64.toString('ascii'), 'base64');
}

module.exports = { encrypt, decrypt, wrapLogin, unwrapLogin, SIGN_KEY };
