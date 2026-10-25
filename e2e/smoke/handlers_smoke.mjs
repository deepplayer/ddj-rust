#!/usr/bin/env node
// WebSocket 冒烟：登录 → 创角 → 再登录
//
// 用途：验证传输层（WS）+ 协议编解码（XXTEA/Base64/protobuf）+ 登录/创角两条链路。
// 对齐原版：10544 只讲 WebSocket；只有 msgId 4 走 XXTEA(SIGN_KEY) + Base64。
//
// 用法：node e2e/smoke/ws_smoke.mjs [ws://127.0.0.1:10544/] [openId] [角色名]

const URL = process.argv[2] ?? 'ws://127.0.0.1:10544/';
const OPEN_ID = process.argv[3] ?? 'ws-smoke-1';
const NAME = process.argv[4] ?? 'WS冒烟角色';
const SERVER_ID = 7010544;

// ── protobuf 手写编码（只用到 string / int32 / bool） ──────────────────
const varint = (n) => { const o = []; let v = n >>> 0; while (v > 0x7f) { o.push((v & 0x7f) | 0x80); v >>>= 7; } o.push(v); return o; };
const tag = (f, w) => varint((f << 3) | w);
const str = (f, s) => { const b = [...Buffer.from(s, 'utf8')]; return [...tag(f, 2), ...varint(b.length), ...b]; };
const i32 = (f, n) => [...tag(f, 0), ...varint(n)];
const rdVarint = (b, i) => { let v = 0, s = 0; for (;;) { const c = b[i++]; v |= (c & 0x7f) << s; if (!(c & 0x80)) return [v >>> 0, i]; s += 7; } };
const parse = (b) => { const out = {}; let i = 0; while (i < b.length) { const [k, j] = rdVarint(b, i); i = j; const f = k >>> 3, w = k & 7; if (w === 0) { const [v, j2] = rdVarint(b, i); i = j2; out[f] = v; } else if (w === 2) { const [n, j2] = rdVarint(b, i); i = j2; out[f] = b.subarray(i, i + n); i += n; } else throw new Error('wire ' + w); } return out; };

// ── XXTEA（对齐 Java XXeta / 客户端 nat_module.js） ────────────────────
const SIGN_KEY = [48, 53, 58, 38, 40, 41, 94, 38, 42, 97, 98, 100, 100, 122, 77, 81];
const DELTA = 0x9E3779B9;
const toWords = (buf, withLen) => {
  const n = Math.ceil(buf.length / 4) + (withLen ? 1 : 0);
  const v = new Uint32Array(n);
  for (let i = 0; i < buf.length; i++) v[i >> 2] |= buf[i] << ((3 & i) << 3);
  if (withLen) v[n - 1] = buf.length;
  return v;
};
const fromWords = (v, withLen) => {
  let n = v.length << 2;
  if (withLen) { const m = v[v.length - 1]; n -= 4; if (m < n - 3 || m > n) throw new Error('bad len word'); n = m; }
  const out = Buffer.alloc(v.length * 4);
  v.forEach((w, i) => out.writeUInt32LE(w >>> 0, i * 4));
  return out.subarray(0, n);
};
const mx = (sum, y, z, p, e, k) =>
  ((((z >>> 5) ^ (y << 2)) + ((y >>> 3) ^ (z << 4))) ^ ((sum ^ y) + (k[(3 & p) ^ e] ^ z)));
const xxteaEncrypt = (data, key) => {
  const v = toWords(data, true), k = toWords(Buffer.from(key), false), n = v.length;
  if (n < 2) return data;
  let sum = 0, z = v[n - 1];
  for (let r = 0, rounds = 6 + Math.floor(52 / n); r < rounds; r++) {
    sum = (sum + DELTA) >>> 0;
    const e = (sum >>> 2) & 3;
    for (let p = 0; p < n - 1; p++) { const y = v[p + 1]; z = v[p] = (v[p] + mx(sum, y, z, p, e, k)) >>> 0; }
    const y = v[0]; z = v[n - 1] = (v[n - 1] + mx(sum, y, z, n - 1, e, k)) >>> 0;
  }
  return fromWords(v, false);
};

// ── 帧 ────────────────────────────────────────────────────────────────

// ── 单连接客户端 ───────────────────────────────────────────────────────
// 关键：会话绑定（player_id）挂在**连接**上，登录与后续命令必须同一条 ws。
class Client {
  #q = new Map();
  #ws;
  async connect() {
    this.#ws = new WebSocket(URL);
    this.#ws.binaryType = 'arraybuffer';
    await new Promise((res, rej) => { this.#ws.onopen = res; this.#ws.onerror = rej; });
    this.#ws.onmessage = (e) => {
      const b = Buffer.from(e.data);
      const cmd = b.readInt32BE(6);
      const head = parse(b.subarray(10));
      // 原版框架的错误出口是 ErrorResp_1（帧号 1），body.msgId 才是请求号。
      // 客户端必须按 msgId 派发，否则会把 msgId 当成业务 ret。
      const key = cmd === 1 ? head[1] : cmd;
      const slot = this.#q.get(key);
      if (slot) { this.#q.delete(key); slot({ cmd, head }); }
    };
    return this;
  }
  call(cmdId, body = Buffer.alloc(0)) {
    return new Promise((resolve, reject) => {
      this.#q.set(cmdId, resolve);
      this.#ws.send(frame(cmdId, body));
      setTimeout(() => { if (this.#q.delete(cmdId)) reject(new Error('超时 cmd=' + cmdId)); }, 8000);
    });
  }
  close() { this.#ws.close(); }
}

const frame = (cmdId, body) => {
  const b = Buffer.alloc(10 + body.length);
  b.writeUInt16BE(29099, 0); b.writeInt32BE(10 + body.length, 2); b.writeInt32BE(cmdId, 6);
  Buffer.from(body).copy(b, 10);
  return b;
};

const loginBody = () => Buffer.from([
  ...str(1, OPEN_ID), ...i32(2, SERVER_ID), ...str(3, 'tok'),
  ...i32(11, 1), ...i32(14, 1), ...str(42, 'zh_CN'), ...i32(31, 0),
]);
const createBody = () => Buffer.from([
  ...str(1, OPEN_ID), ...i32(2, SERVER_ID), ...str(3, NAME), ...str(4, '1'),
  ...i32(5, 1), ...str(6, String(Math.floor(Date.now() / 1000))),
  ...str(7, 'tok'), ...str(8, 'x'), ...i32(10, 1), ...i32(11, 1), ...str(32, 'zh_CN'),
]);

// 每条只求「收到回包」——过渡期数据源未接，ret 值不是断言目标。
const CASES = [
  ['11002 背包使用', 11002, Buffer.from([...i32(1, 400001), ...i32(2, 1)])],
  ['11003 合成',     11003, Buffer.from([...i32(1, 400001), ...i32(2, 1)])],
  ['11005 时效使用', 11005, Buffer.from([...i32(1, 900001), ...i32(2, 1)])],
  ['11006 一键使用', 11006, Buffer.from([...i32(1, 900001)])],
  ['11008 过期确认', 11008, Buffer.from([...i32(1, 900001)])],
  ['16002 签到',     16002, Buffer.alloc(0)],
  ['16003 领宝箱',   16003, Buffer.from([...i32(1, 1)])],
  ['16004 拉签到',   16004, Buffer.alloc(0)],
  ['10001 拉邮件',   10001, Buffer.alloc(0)],
  ['10002 标记已读', 10002, Buffer.from([...str(1, '1')])],
  ['10004 删邮件',   10004, Buffer.from([...str(1, '1')])],
  ['1002 改名',       1002, Buffer.from([...str(1, '测试改名')])],
  ['1003 升级',       1003, Buffer.alloc(0)],
  ['17091 琥珀数据',  17091, Buffer.alloc(0)],
  ['17092 琥珀签到',  17092, Buffer.alloc(0)],
  ['17093 琥珀专属',  17093, Buffer.alloc(0)],
  ['17094 充值豪礼',  17094, Buffer.from([...i32(1, 1)])],
  ['17095 琥珀VIP',   17095, Buffer.alloc(0)],
  ['17097 HeyTapVIP', 17097, Buffer.alloc(0)],
];

const main = async () => {
  const c = await new Client().connect();
  const enc = xxteaEncrypt(Buffer.from(loginBody().toString('base64'), 'binary'), SIGN_KEY);
  let r = await c.call(4, enc);
  if (!r.head[2]) {
    r = await c.call(6, createBody());
    console.log('创角 cmd=%d ret=%d', r.cmd, r.head[1]);
    const again = xxteaEncrypt(Buffer.from(loginBody().toString('base64'), 'binary'), SIGN_KEY);
    r = await c.call(4, again);
  }
  console.log('登录 cmd=%d ret=%d ok=%d', r.cmd, r.head[1], r.head[2] ?? 0);

  let pass = 0;
  for (const [name, cmd, body] of CASES) {
    try {
      const got = await c.call(cmd, body);
      console.log('  ✅ %s → cmd=%d f1=%d f2=%d', name, got.cmd, got.head[1] ?? -1, got.head[2] ?? -1);
      pass++;
    } catch (e) {
      console.log('  ❌ %s → %s', name, e.message);
    }
  }
  c.close();
  console.log('\n%s %d/%d 有回包', pass === CASES.length ? '✅' : '⚠️', pass, CASES.length);
  if (pass !== CASES.length) process.exit(1);
};

main().catch((e) => { console.error('❌', e.message); process.exit(1); });
