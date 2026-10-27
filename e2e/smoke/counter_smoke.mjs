#!/usr/bin/env node
// 计数器恢复 8001/8002 冒烟。
//
// 真实配置（`J-计数器恢复-CounterRecovery.csv` 92 行 / `V-VIP表-Vip.csv` 18 级）：
//   - 2 号：宠物精力，type=1, cd=1800s, everyNum=1, isLimit=0（受限），VIP0 上限 3
//   - 4 号：商战挑战，type=2（跨天补满），cd 空 → cdTime=0
//   - 5 号：子嗣活力，dbInit 时**跳过**（由 add(templateId, param1) 单独创建）
//
// 断言（`CounterRecoveryMessageHandler:17-39` / `CounterRecoverySet:124-147`）：
//   1. 8001 → 92 条里除 5 号外的 91 条，全部 num=max、cdTime 按 id/type 分派
//   2. 8001 的 2 号：num=3（VIP0 上限）、cdTime=-1（满值 → getCdTime 返回 -1）
//   3. 8001 **不含** 5 号
//   4. 8002 templateId=2 → 单条 ret=0
//   5. 8002 templateId=999999 → **无回包**（原版没有 else）
//
// 用法：node e2e/smoke/counter_smoke.mjs [ws://127.0.0.1:10544/]

const URL = process.argv[2] ?? 'ws://127.0.0.1:10544/';
const OPEN_ID = 'ct-' + Date.now();
const NAME = 'CT' + (Date.now() % 1000000);
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
    // 用 BigInt 处理：int64 的 -1 是 10 字节 varint，32 位位移会溢出
    if (w === 0) { const [v, j2] = readI64(b, i); i = j2; out[f] = v; }
    else if (w === 2) { const [n, j2] = rdVarint(b, i); i = j2; out[f] = b.subarray(i, i + n); i += n; }
    else if (w === 5) { out[f] = b.readUInt32LE(i); i += 4; }
    else throw new Error('wire ' + w);
  }
  return out;
};
// 逐段提取某个 repeated 嵌套消息字段（parse() 对重复 tag 会互相覆盖，不能复用）。
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

// int64 负数在 protobuf 里是 10 字节 varint（two's complement），
// JS 位移到 64 位会溢出，这里用 BigInt 读后再转回 Number。
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

const main = async () => {
  const c = await new Client().connect();
  let r = await c.call(4, xxteaEncrypt(Buffer.from(loginBody().toString('base64'), 'binary'), SIGN_KEY));
  if (!r.head[2]) {
    const cr = await c.call(6, createBody());
    console.log('创角 ret=%d', cr.head[1] ?? -1);
    r = await c.call(4, xxteaEncrypt(Buffer.from(loginBody().toString('base64'), 'binary'), SIGN_KEY));
  }
  console.log('登录 ok=%d\n', r.head[2] ?? 0);

  // 1) 8001 全量
  let rows = [];
  try {
    const g = await c.call(8001, Buffer.alloc(0));
    if (g.cmd !== 8001) bad(`8001 帧号=${g.cmd}`);
    else if (g.head[1] !== 0) bad(`8001 ret=${g.head[1]}`);
    else {
      rows = messages(g.body, 2);
      if (rows.length === 91) ok(`8001 → 91 条（92 模板 - 跳过 5 号）`);
      else bad(`8001 返回 ${rows.length} 条（应为 91）`);
    }
  } catch (e) { bad('8001 ' + e.message); }

  const byId = new Map(rows.map((x) => [x[1], x]));

  // 2) 2 号：VIP0 上限 3，满值 → cdTime=-1
  const c2 = byId.get(2);
  if (!c2) bad('8001 缺 2 号');
  else if (c2[2] !== 3) bad(`2 号 num=${c2[2]}（应为 3）`);
  else if (c2[3] !== -1) bad(`2 号 cdTime=${c2[3]}（满值应为 -1）`);
  else ok('2 号 → num=3（VIP0 上限），cdTime=-1（满值停表）');

  // 3) 不含 5 号
  if (byId.has(5)) bad('8001 含 5 号（dbInit 应跳过子嗣）');
  else ok('8001 不含 5 号（dbInit 跳过子嗣）');

  // 4) 4 号：type=2 → cdTime = time（满值停表 → -1）
  const c4 = byId.get(4);
  if (!c4) bad('8001 缺 4 号');
  else if (c4[2] !== 5) bad(`4 号 num=${c4[2]}（VIP0 上限应为 5）`);
  else ok('4 号 → num=5（VIP0 上限）');

  // 5) 8002 命中
  try {
    const g = await c.call(8002, Buffer.from([...i32(1, 2)]));
    if (g.cmd !== 8002) bad(`8002 帧号=${g.cmd}`);
    else if (g.head[1] !== 0) bad(`8002 ret=${g.head[1]}`);
    else {
      const d = parse(g.head[2]);
      if (d[1] !== 2) bad(`8002 templateId=${d[1]}`);
      else ok(`8002 → 单条 templateId=2 num=${d[2]} cdTime=${d[3]}`);
    }
  } catch (e) { bad('8002 ' + e.message); }

  // 6) 8002 未命中 → 无回包
  try {
    const g = await c.call(8002, Buffer.from([...i32(1, 999999)]), 1500);
    bad(`8002 未知模板意外收到回包 cmd=${g.cmd}`);
  } catch { ok('8002 未知模板 → 无回包（原版没有 else）'); }

  c.close();
  console.log('\n%s 计数器恢复冒烟', fails === 0 ? '✅' : '❌');
  if (fails) process.exit(1);
};

main().catch((e) => { console.error('❌', e.message); process.exit(1); });
