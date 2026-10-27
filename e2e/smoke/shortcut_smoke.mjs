#!/usr/bin/env node
// 保存桌面 17301/17302/17303 + 客户端特约 1109 冒烟。
//
// 依赖的真实数据：
//   - `bf_config.configId=59`（保存桌面渠道白名单）实测是**空串**
//     （`data/sql/platform.sql:562`）→ `isEligibleShortcut` 恒 false
//   - `ConfigDBCache.SaveIconReward` = `11=1;215=3;216=3;30=1;234=1`
//     （`B-变量表-Config.csv:509`，id=801）
//
// 断言（`ShortcutModel.java:70-131` / `ClientSendSpecialMsgHandler.java:15-24`）：
//   1. 17301 → `ret=0`，`shortcutData{state=0, eligible=false}`
//   2. 17302 → **帧号 1**，`ErrorResp_1{msgId=17302, code=4731}`（渠道不符优先）
//   3. 17303 → **帧号 1**，`ErrorResp_1{msgId=17303, code=4731}`
//   4. 1109  → `ret=0`，`msgId` 回显，**`data` 不设置**
//
// 用法：node e2e/smoke/shortcut_smoke.mjs [ws://127.0.0.1:10544/]

const URL = process.argv[2] ?? 'ws://127.0.0.1:10544/';
const OPEN_ID = 'sc-' + Date.now();
const NAME = 'SC' + (Date.now() % 1000000);
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
    if (w === 0) { const [v, j2] = rdVarint(b, i); i = j2; out[f] = v; }
    else if (w === 2) { const [n, j2] = rdVarint(b, i); i = j2; out[f] = b.subarray(i, i + n); i += n; }
    else if (w === 5) { out[f] = b.readUInt32LE(i); i += 4; }
    else if (w === 1) { i += 8; }
    else throw new Error('wire ' + w);
  }
  return out;
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
      // 框架错误出口是 ErrorResp_1（帧号 1），body.msgId 才是请求号。
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
// channelId(11) / cid(12) 带上来，创角时会落库
const createBody = () => Buffer.from([
  ...str(1, OPEN_ID), ...i32(2, SERVER_ID), ...str(3, NAME), ...str(4, '1'),
  ...i32(5, 1), ...str(6, String(Math.floor(Date.now() / 1000))),
  ...str(7, 'tok'), ...str(8, 'x'), ...i32(10, 1), ...i32(11, 2), ...str(12, 'testcid'),
  ...str(32, 'zh_CN'),
]);

let fails = 0;
const ok = (m) => console.log('  ✅ ' + m);
const bad = (m) => { fails++; console.log('  ❌ ' + m); };

const main = async () => {
  const c = await new Client().connect();
  let r = await c.call(4, xxteaEncrypt(Buffer.from(loginBody().toString('base64'), 'binary'), SIGN_KEY));
  if (!r.head[2]) {
    const cr = await c.call(6, createBody());
    console.log('创角 ret=%d', cr.head[1] ?? -1);
    r = await c.call(4, xxteaEncrypt(Buffer.from(loginBody().toString('base64'), 'binary'), SIGN_KEY));
  }
  console.log('登录 ok=%d\n', r.head[2] ?? 0);

  // 1) 17301 → ret=0，state=0，eligible=false
  try {
    const g = await c.call(17301, Buffer.alloc(0));
    if (g.cmd !== 17301) bad(`17301 帧号=${g.cmd}（应为 17301）`);
    else if (g.head[1] !== 0) bad(`17301 ret=${g.head[1]}`);
    else {
      const d = g.head[2] ? parse(g.head[2]) : {};
      const state = d[3] ?? -1;
      const eligible = d[4] ?? -1;
      if (state !== 0) bad(`17301 state=${state}（应为 0）`);
      else if (eligible !== 0) bad(`17301 eligible=${eligible}（空配置应为 false）`);
      else ok(`17301 → state=0, eligible=false（channelId=${d[1] ?? -1}, cid="${d[2] ? d[2].toString() : ''}"）`);
    }
  } catch (e) { bad('17301 ' + e.message); }

  // 2) 17302 → 帧号 1，ErrorResp_1{msgId=17302, code=4731}
  try {
    const g = await c.call(17302, Buffer.alloc(0));
    if (g.cmd !== 1) bad(`17302 帧号=${g.cmd}（应为 1，走 ErrorResp_1）`);
    else if (g.head[1] !== 17302) bad(`17302 msgId=${g.head[1]}`);
    else if (g.head[2] !== 4731) bad(`17302 code=${g.head[2]}（渠道不符应为 4731）`);
    else ok('17302 → ErrorResp_1{msgId=17302, code=4731}');
  } catch (e) { bad('17302 ' + e.message); }

  // 3) 17303 → 帧号 1，code=4731（渠道判定优先于 state==0 的 4732）
  try {
    const g = await c.call(17303, Buffer.alloc(0));
    if (g.cmd !== 1) bad(`17303 帧号=${g.cmd}（应为 1）`);
    else if (g.head[1] !== 17303) bad(`17303 msgId=${g.head[1]}`);
    else if (g.head[2] !== 4731) bad(`17303 code=${g.head[2]}（渠道优先应为 4731，不是 4732）`);
    else ok('17303 → ErrorResp_1{code=4731}（渠道判定优先于 state）');
  } catch (e) { bad('17303 ' + e.message); }

  // 4) 1109 → ret=0，msgId 回显，data 不设置
  try {
    const g = await c.call(1109, Buffer.from([...str(1, '1'), ...str(2, '{"a":1}')]));
    if (g.cmd !== 1109) bad(`1109 帧号=${g.cmd}`);
    else if (g.head[1] !== 0) bad(`1109 ret=${g.head[1]}`);
    else if (!g.head[2] || g.head[2].toString() !== '1') bad(`1109 msgId 未回显`);
    else if (g.head[3] !== undefined) bad(`1109 设置了 data（原版恒为 null）`);
    else ok('1109 → {ret=0, msgId="1"}，data 未设置（原版空壳）');
  } catch (e) { bad('1109 ' + e.message); }

  c.close();
  console.log('\n%s 保存桌面 + 特约消息冒烟', fails === 0 ? '✅' : '❌');
  if (fails) process.exit(1);
};

main().catch((e) => { console.error('❌', e.message); process.exit(1); });
