#!/usr/bin/env node
// 防骗手册 17151/17152/17153 全链路冒烟（真实配置 B-变量表-Config.csv:568）。
//
// 真实配置（`AntiCheatReward`，实测 4 组）：
//   0: 1=25;231=1;211=1;201=1
//   1: 1=25;231=1;211=2;201=2
//   2: 1=25;231=1;45=1;202=1
//   3: 1=25;231=1;46=1;202=1
// 所以 index 0..3 在范围内，index 4 越界。
//
// 断言（全部来自 `SysActivityMessageHandler:119-162`）：
//   1. 17151 越界      → 帧号 **17151**，ret=29（PARAM_ERROR）
//   2. 17151 未阅读    → 帧号 17151，ret=6451（ANTICHEAT_READ）
//   3. 17152 越界      → **完全无回包**（原版 `return`）
//   4. 17152 范围内    → 帧号 17152，ret=0
//   5. 17151 已阅读    → 帧号 17151，ret=0，奖励入包
//   6. 17151 重复领取  → 帧号 17151，ret=1501（REPEATED）
//   7. 17153           → info{reward=[0], read=[0]}
//
// 用法：node e2e/smoke/anticheat_smoke.mjs [ws://127.0.0.1:10544/]

const URL = process.argv[2] ?? 'ws://127.0.0.1:10544/';
const OPEN_ID = 'ac-' + Date.now();
const NAME = 'AC' + (Date.now() % 1000000);
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
    else throw new Error('wire ' + w);
  }
  return out;
};
// ⚠ proto2 的 repeated int32 默认 **不打包**（`packed = "false"`），
// 每个元素都是独立的 `tag(field,0) + varint`。prost 生成的代码也是这样。
const repeatedInts = (body, field) => {
  if (!body) return [];
  const out = []; let i = 0;
  while (i < body.length) {
    const [k, j] = rdVarint(body, i); i = j;
    const f = k >>> 3, w = k & 7;
    if (w === 0) { const [v, j2] = rdVarint(body, i); i = j2; if (f === field) out.push(v); }
    else if (w === 2) { const [n, j2] = rdVarint(body, i); i = j2; i += n; }
    else if (w === 5) { i += 4; }
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
const createBody = () => Buffer.from([
  ...str(1, OPEN_ID), ...i32(2, SERVER_ID), ...str(3, NAME), ...str(4, '1'),
  ...i32(5, 1), ...str(6, String(Math.floor(Date.now() / 1000))),
  ...str(7, 'tok'), ...str(8, 'x'), ...i32(10, 1), ...i32(11, 1), ...str(32, 'zh_CN'),
]);

let fails = 0;
const ok = (m) => console.log('  ✅ ' + m);
const bad = (m) => { fails++; console.log('  ❌ ' + m); };

// 断言「帧号 + ret」。
const expectRet = async (c, cmd, index, wantCmd, wantRet, label) => {
  try {
    const g = await c.call(cmd, Buffer.from([...i32(1, index)]));
    const ret = g.head[1] ?? -1;
    if (g.cmd !== wantCmd) bad(`${label} 帧号=${g.cmd}（应为 ${wantCmd}）`);
    else if (ret !== wantRet) bad(`${label} ret=${ret}（应为 ${wantRet}）`);
    else ok(`${label} → 帧号 ${wantCmd}，ret=${wantRet}`);
    return g;
  } catch (e) { bad(`${label} ${e.message}`); return null; }
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

  // 1) 17151 越界（index=4，配置只有 0..3）→ 帧号 17151，ret=29
  await expectRet(c, 17151, 4, 17151, 29, '17151 越界');

  // 2) 17151 未阅读（index=0）→ 6451
  await expectRet(c, 17151, 0, 17151, 6451, '17151 未阅读');

  // 3) 17152 越界 → 必须**没有**回包
  try {
    const g = await c.call(17152, Buffer.from([...i32(1, 4)]), 1500);
    bad(`17152 越界意外收到回包 cmd=${g.cmd} f1=${g.head[1]}`);
  } catch { ok('17152 越界 → 无回包（照抄原版 return）'); }

  // 4) 17152 范围内 → ret=0
  await expectRet(c, 17152, 0, 17152, 0, '17152 标记已读');

  // 5) 17151 已阅读 → ret=0（发奖）
  await expectRet(c, 17151, 0, 17151, 0, '17151 领奖');

  // 6) 重复领取 → 1501
  await expectRet(c, 17151, 0, 17151, 1501, '17151 重复领取');

  // 7) 17153 → info{reward=[0], read=[0]}
  try {
    const g = await c.call(17153, Buffer.alloc(0));
    if (g.cmd !== 17153) bad(`17153 帧号=${g.cmd}`);
    else if (!g.head[1]) bad('17153 无 info');
    else {
      const info = g.head[1];
      const rw = repeatedInts(info, 1);
      const rd = repeatedInts(info, 2);
      if (rw.length !== 1 || rw[0] !== 0) bad(`17153 reward=${JSON.stringify(rw)}（应为 [0]）`);
      else if (rd.length !== 1 || rd[0] !== 0) bad(`17153 read=${JSON.stringify(rd)}（应为 [0]）`);
      else ok('17153 → info{reward=[0], read=[0]}');
    }
  } catch (e) { bad('17153 ' + e.message); }

  c.close();
  console.log('\n%s 防骗手册全链路', fails === 0 ? '✅' : '❌');
  if (fails) process.exit(1);
};

main().catch((e) => { console.error('❌', e.message); process.exit(1); });
