#!/usr/bin/env node
// 焚香祈愿 17071/17072 冒烟。
//
// 真实配置（`F-焚香祈愿-Pray.csv`，实测 1 行 day=1）：
//   单抽消耗 263×1 / 十连 263×9 / 幸运值上限 150 / 每次 +1
//   掉落 151 / 核心奖励 262×1
//
// 断言（`PrayMessageHandler.java`）：
//   1. 17072 → 公告栏为空（还没人抽到核心）
//   2. 17071 单抽（新号无 263 道具）→ 帧号 **1**，`ErrorResp_1{3902}`
//      （第一次免费 —— 免费就不用扣费，所以应该成功）
//   3. 17071 十连 → 扣 263×9，新号没有 → `3902`
//   4. 17071 单抽成功后 → **两个包**：先 17074，再 17071
//
// 用法：node e2e/smoke/pray_smoke.mjs [ws://127.0.0.1:10544/]

const URL = process.argv[2] ?? 'ws://127.0.0.1:10544/';
const OPEN_ID = 'pr-' + Date.now();
const NAME = 'PR' + (Date.now() % 1000000);
const SERVER_ID = 7010544;

const varint = (n) => { const o = []; let v = n >>> 0; while (v > 0x7f) { o.push((v & 0x7f) | 0x80); v >>>= 7; } o.push(v); return o; };
const tag = (f, w) => varint((f << 3) | w);
const str = (f, s) => { const b = [...Buffer.from(s, 'utf8')]; return [...tag(f, 2), ...varint(b.length), ...b]; };
const i32 = (f, n) => [...tag(f, 0), ...varint(n)];
const rdVarint = (b, i) => { let v = 0, s = 0; for (;;) { const c = b[i++]; v |= (c & 0x7f) << s; if (!(c & 0x80)) return [v >>> 0, i]; s += 7; } };
const readI64 = (b, i) => {
  let shift = 0n, val = 0n;
  for (;;) { const c = b[i++]; val |= BigInt(c & 0x7f) << shift; if (!(c & 0x80)) break; shift += 7n; }
  if (val >= (1n << 63n)) val -= 1n << 64n;
  return [Number(val), i];
};
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
  #pushes = [];
  #ws;
  async connect() {
    this.#ws = new WebSocket(URL);
    this.#ws.binaryType = 'arraybuffer';
    await new Promise((res, rej) => { this.#ws.onopen = res; this.#ws.onerror = rej; });
    this.#ws.onmessage = (e) => {
      const b = Buffer.from(e.data);
      const cmd = b.readInt32BE(6);
      const head = parse(b.subarray(10));
      const key = cmd === 1 ? head[1] : cmd;
      const slot = this.#q.get(key);
      if (slot) { this.#q.delete(key); slot({ cmd, head, body: b.subarray(10) }); }
      else this.#pushes.push({ cmd, head, body: b.subarray(10) });
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
  pushes() { return this.#pushes; }
  clearPushes() { this.#pushes = []; }
  close() { this.#ws.close(); }
}

const loginBody = () => Buffer.from([
  ...str(1, OPEN_ID), ...i32(2, SERVER_ID), ...str(3, 'tok'),
  ...i32(11, 1), ...i32(14, 1), ...str(42, 'zh_CN'), ...i32(31, 0),
]);
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

  // 1) 17072 公告栏为空
  try {
    const g = await c.call(17072, Buffer.alloc(0));
    if (g.cmd !== 17072) bad(`17072 帧号=${g.cmd}`);
    else {
      // notices 是 repeated NoticeData（tag 1），空数组则 body 里没有 tag 1
      if (g.head[1] !== undefined) bad(`17072 公告栏非空（${g.head[1].length} 字节）`);
      else ok('17072 → 公告栏为空');
    }
  } catch (e) { bad('17072 ' + e.message); }

  // 2) 17071 单抽（第一次免费）→ 应成功，且**先推 17074**
  c.clearPushes();
  try {
    const g = await c.call(17071, Buffer.from([...i32(1, 1)]));
    if (g.cmd === 1) {
      // 走了 ErrorResp_1
      const code = g.head[2];
      if (code === 3901) ok('17071 单抽 → 3901 未开放（表未装载）');
      else bad(`17071 单抽 → ErrorResp_1 code=${code}`);
    } else if (g.cmd !== 17071) bad(`17071 帧号=${g.cmd}`);
    else {
      const ps = c.pushes().filter((p) => p.cmd === 17074);
      if (ps.length === 0) bad('17071 没有推 17074（原版先推后回）');
      else {
        const d = ps[0].head[1] ? parse(ps[0].head[1]) : {};
        ok(`17071 单抽 → ret=0，先推 17074{prayData{freeTime=${d[1]}, luckNum=${d[2]}, cycle=${d[3]}}}`);
        if (d[2] !== 1) bad(`17074 luckNum=${d[2]}（应为 1，每次 +1）`);
      }
    }
  } catch (e) { bad('17071 单抽 ' + e.message); }

  // 3) 17071 十连（同一天，无免费）→ 新号没有 263 → 3902
  try {
    const g = await c.call(17071, Buffer.from([...i32(1, 10)]));
    if (g.cmd !== 1) bad(`17071 十连帧号=${g.cmd}（应为 1，走 ErrorResp_1）`);
    else if (g.head[2] !== 3902) bad(`17071 十连 code=${g.head[2]}（应为 3902 资源不足）`);
    else ok('17071 十连 → 3902 资源不足（十连无免费）');
  } catch (e) { bad('17071 十连 ' + e.message); }

  // 4) 17071 单抽第二次（同一天）→ 已经不免费 → 3902
  try {
    const g = await c.call(17071, Buffer.from([...i32(1, 1)]));
    if (g.cmd !== 1) bad(`17071 二次单抽帧号=${g.cmd}（应为 1）`);
    else if (g.head[2] !== 3902) bad(`17071 二次单抽 code=${g.head[2]}（应为 3902，同一天不免费）`);
    else ok('17071 二次单抽 → 3902（同一天只有第一次免费）');
  } catch (e) { bad('17071 二次单抽 ' + e.message); }

  c.close();
  console.log('\n%s 焚香祈愿冒烟', fails === 0 ? '✅' : '❌');
  if (fails) process.exit(1);
};

main().catch((e) => { console.error('❌', e.message); process.exit(1); });
