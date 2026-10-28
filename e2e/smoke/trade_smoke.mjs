#!/usr/bin/env node
// 通商 19002/19003/19004/19005 冒烟。
//
// 真实配置（`T-通商-ForeignTrader.csv` 10 行 1201..1210，`TraderTime=3600` 秒）：
//   - 1201：tradeNum=3 / totalCost=25000000 / everyCost=500000 / everyReward=30000 / dropId=51
//   - 1210：tradeNum=3 / totalCost=4000000000000 / everyCost=80000000000 / everyReward=600000
//   - 一次通商 CD = TraderTime * Coliseum.MAX_INDEX = 3600 * 1000 = 3_600_000 ms（1 小时）
//
// ⚠ 新号的 `traders` 是空的 —— 原版也一样（`trader` 表里没有行）。
// 商人只能由 `ResourcesUtils.addResourcesJsons` 的 `TypeResources.TRADER`(5) 分支发放
// （VIP/活动奖励里的 `1201=40` 这类项），而通用发奖链路在全项目尚未实现（AUDIT A25）。
// 所以本冒烟验证的是「无商人时四条消息各自的错误码」+「19004 空列表」。
//
// 断言（`TradeMessageHandler:22-74`）：
//   1. 19004 → ret=0，msgTrader 空
//   2. 19002 id=1201 num=1 → 2201（商人不存在）
//   3. 19003 id=1201 all=0 → 2201（商人不存在）
//   4. 19003 id=1201 all=1 → 2202（一键通商未开放：106 未开 且 商人数 < 3）
//   5. 19005 → 2210（没有商人可以通商）
//
// 用法：node e2e/smoke/trade_smoke.mjs [ws://127.0.0.1:10544/]

const URL = process.argv[2] ?? 'ws://127.0.0.1:10544/';
const OPEN_ID = 'tr-' + Date.now();
const NAME = 'TR' + (Date.now() % 1000000);
const SERVER_ID = 7010544;

const varint = (n) => { const o = []; let v = n >>> 0; while (v > 0x7f) { o.push((v & 0x7f) | 0x80); v >>>= 7; } o.push(v); return o; };
const tag = (f, w) => varint((f << 3) | w);
const str = (f, s) => { const b = [...Buffer.from(s, 'utf8')]; return [...tag(f, 2), ...varint(b.length), ...b]; };
const i32 = (f, n) => [...tag(f, 0), ...varint(n)];
const rdVarint = (b, i) => { let v = 0, s = 0; for (;;) { const c = b[i++]; v |= (c & 0x7f) << s; if (!(c & 0x80)) return [v >>> 0, i]; s += 7; } };
const parse = (b) => {
  const out = {}; let i = 0;
  while (i < b.length) {
    const [k, j] = rdVarint(b, i); i = j;
    const f = k >>> 3, w = k & 7;
    if (w === 0) { const [v, j2] = readI64(b, i); i = j2; out[f] = v; }
    else if (w === 2) { const [n, j2] = rdVarint(b, i); i = j2; out[f] = b.subarray(i, i + n); i += n; }
    else if (w === 5) { out[f] = b.readUInt32LE(i); i += 4; }
    else if (w === 1) { i += 8; }
    else throw new Error('wire ' + w);
  }
  return out;
};
// 逐段提取某个 repeated 嵌套消息字段（parse() 对重复 tag 会互相覆盖）。
const messages = (body, field) => {
  if (!body) return [];
  const out = []; let i = 0;
  while (i < body.length) {
    const [k, j] = rdVarint(body, i); i = j;
    const f = k >>> 3, w = k & 7;
    if (w === 0) { const [, j2] = rdVarint(body, i); i = j2; }
    else if (w === 2) {
      const [n, j2] = rdVarint(body, i); i = j2;
      if (f === field) out.push(parse(body.subarray(i, i + n)));
      i += n;
    }
    else if (w === 5) { i += 4; }
    else if (w === 1) { i += 8; }
    else throw new Error('wire ' + w);
  }
  return out;
};

// int64 负数在 protobuf 里是 10 字节 varint，JS 位移到 64 位会溢出。
const readI64 = (b, i) => {
  let shift = 0n, val = 0n;
  for (;;) {
    const c = b[i++];
    val |= BigInt(c & 0x7f) << shift;
    if (!(c & 0x80)) break;
    shift += 7n;
  }
  if (val >= (1n << 63n)) val -= 1n << 64n;
  return [Number(val), i];
};

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

const frame = (cmdId, body) => {
  const b = Buffer.alloc(10 + body.length);
  b.writeUInt16BE(29099, 0); b.writeInt32BE(10 + body.length, 2); b.writeInt32BE(cmdId, 6);
  Buffer.from(body).copy(b, 10);
  return b;
};

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
      // ErrorResp_1 的帧号是 1，真正被拒的消息号在 field 1
      const key = cmd === 1 ? head[1] : cmd;
      const slot = this.#q.get(key);
      if (slot) { this.#q.delete(key); slot({ cmd, head, body: b.subarray(10) }); }
    };
    return this;
  }
  call(cmdId, body = Buffer.alloc(0), timeoutMs = 8000) {
    return new Promise((resolve, reject) => {
      this.#q.set(cmdId, resolve);
      this.#ws.send(frame(cmdId, body));
      setTimeout(() => { if (this.#q.delete(cmdId)) reject(new Error('超时 cmd=' + cmdId)); }, timeoutMs);
    });
  }
  close() { this.#ws.close(); }
}

const loginBody = () => Buffer.from([
  ...str(1, OPEN_ID), ...i32(2, SERVER_ID), ...str(3, 'tok'),
  ...i32(11, 1), ...i32(14, 1), ...str(42, 'zh_CN'), ...i32(31, 0),
]);
const createBody = () => Buffer.from([
  ...str(1, OPEN_ID), ...i32(2, SERVER_ID), ...str(3, NAME), ...str(4, '1'),
  ...i32(5, 1), ...str(6, String(Math.floor(Date.now() / 1000))),
  ...str(7, 'tok'), ...str(8, 'x'), ...i32(10, 1), ...i32(11, 1), ...str(32, 'zh_CN'),
]);

let fails = 0;
const ok = (m) => console.log('  ✅ ' + m);
const bad = (m) => { fails++; console.log('  ❌ ' + m); };

/** 断言「帧号 1 的 ErrorResp，被拒消息号 = want_cmd，错误码 = want_code」。 */
const expectErr = (g, wantCmd, wantCode, label) => {
  if (g.cmd !== 1) return bad(`${label} 帧号=${g.cmd}（错误必须是 ErrorResp_1）`);
  if (g.head[1] !== wantCmd) return bad(`${label} msgId=${g.head[1]}（应为 ${wantCmd}）`);
  if (g.head[2] !== wantCode) return bad(`${label} errorCode=${g.head[2]}（应为 ${wantCode}）`);
  ok(`${label} → ${wantCode}`);
};

const main = async () => {
  const c = await new Client().connect();
  let r = await c.call(4, xxteaEncrypt(Buffer.from(loginBody().toString('base64'), 'binary'), SIGN_KEY));
  if (!r.head[2]) {
    const cr = await c.call(6, createBody());
    console.log('创角 ret=%d', cr.head[1] ?? -1);
    r = await c.call(4, xxteaEncrypt(Buffer.from(loginBody().toString('base64'), 'binary'), SIGN_KEY));
  }
  console.log('登录 ok=%d\n', r.head[2] ?? 0);

  // 1) 19004 商人列表：新号空列表
  try {
    const g = await c.call(19004, Buffer.alloc(0));
    if (g.cmd !== 19004) bad(`19004 帧号=${g.cmd}`);
    else if (g.head[1] !== 0) bad(`19004 ret=${g.head[1]}`);
    else {
      const list = messages(g.body, 2);
      if (list.length !== 0) bad(`19004 返回 ${list.length} 个商人（新号应为 0）`);
      else ok('19004 → ret=0，msgTrader 空（新号没有商人，与原版一致）');
    }
  } catch (e) { bad('19004 ' + e.message); }

  // 2) 19002 用葡萄美酒通商：商人不存在 → 2201
  try {
    const g = await c.call(19002, Buffer.from([...i32(1, 1201), ...i32(2, 1)]));
    expectErr(g, 19002, 2201, '19002 id=1201 num=1');
  } catch (e) { bad('19002 ' + e.message); }

  // 3) 19003 单商人通商：商人不存在 → 2201
  try {
    const g = await c.call(19003, Buffer.from([...i32(1, 1201), ...i32(2, 0)]));
    expectErr(g, 19003, 2201, '19003 id=1201 all=0');
  } catch (e) { bad('19003 ' + e.message); }

  // 4) 19003 all=1：106 未开 且 商人数 0 < 3 → 2202（判在取商人之前）
  try {
    const g = await c.call(19003, Buffer.from([...i32(1, 1201), ...i32(2, 1)]));
    expectErr(g, 19003, 2202, '19003 id=1201 all=1');
  } catch (e) { bad('19003 all=1 ' + e.message); }

  // 5) 19005 一键通商：一个商人都没有 → 2210
  try {
    const g = await c.call(19005, Buffer.alloc(0));
    expectErr(g, 19005, 2210, '19005');
  } catch (e) { bad('19005 ' + e.message); }

  c.close();
  console.log('\n%s 通商冒烟', fails === 0 ? '✅' : '❌');
  if (fails) process.exit(1);
};

main().catch((e) => { console.error('❌', e.message); process.exit(1); });
