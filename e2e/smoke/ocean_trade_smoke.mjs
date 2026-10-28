#!/usr/bin/env node
// 远洋贸易 41001 / 41002 / 41003 冒烟。
//
// 数据（`template/csv/Y-远洋贸易-OceanTrade.csv` 实测）：400 关，id 1..400 连续。
// `B-变量表-Config.csv`：`OceanTradeSeasonTime = 7`、`CheckpointCostInterval[0] = 0.2`、
// `OceanTradeRankListDisplayQuantity = 500`、`OceanTradeRankListActualNumber = 2000`。
//
// 断言（`OceanTradeMessageHandler.java`）：
//   1. 41001 → **成功**（帧号 41001），`checkpointId=1`、`seasonId=1`、`npcHp=12250000000`
//      即第 1 关 `cost`（`hpCost` 初值 "0"）
//   2. 41002 type=0 → 成功，`index=-1`（没打过就不在榜）、空列表
//   3. 41002 type=1 → 成功（原版对 type 不判合法性）
//   4. 41003 seasonId=999 → 帧号 1，code=3004（赛季 id 不符）
//   5. 41003 seasonId=1 type=0 → 成功；身上 20000 银两 << 第 1 关 cost
//      → `preMoney < needMoney` 走 `calNewCost`，返回 `damageHp` + `costMoney`
//   6. 41003 seasonId=1 type=1 num=99999 → 帧号 1，code=3003（一键未解锁 `isOpen(112)`）
//
// 用法：node e2e/smoke/ocean_trade_smoke.mjs [ws://127.0.0.1:10544/]

const URL = process.argv[2] ?? 'ws://127.0.0.1:10544/';
const OPEN_ID = 'ot-' + Date.now();
const NAME = 'OT' + (Date.now() % 1000000);
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

// OceanTradeEnterReq_41001 {} —— 空体
const req41001 = () => Buffer.alloc(0);
// OceanTradeGetRankingReq_41002 { type }
const req41002 = (type) => Buffer.from([...i32(1, type)]);
// OceanTradeNegotiationReq_41003 { type, num, seasonId }
const req41003 = (type, num, seasonId) =>
  Buffer.from([...i32(1, type), ...i32(2, num), ...i32(3, seasonId)]);

const s = (b, f) => (b[f] === undefined ? undefined : Buffer.from(b[f]).toString('utf8'));

const main = async () => {
  const c = await new Client().connect();
  let r = await c.call(4, xxteaEncrypt(Buffer.from(loginBody().toString('base64'), 'binary'), SIGN_KEY));
  if (!r.head[2]) {
    const cr = await c.call(6, createBody());
    console.log('创角 ret=%d', cr.head[1] ?? -1);
    r = await c.call(4, xxteaEncrypt(Buffer.from(loginBody().toString('base64'), 'binary'), SIGN_KEY));
  }
  console.log('登录 ok=%d\n', r.head[2] ?? 0);

  // ── 1. 41001 ──
  const e = await c.call(41001, req41001());
  const ed = parse(e.body);
  const data = ed[2] ? parse(ed[2]) : {};
  if (e.cmd !== 41001) bad(`41001 帧号=${e.cmd}（应为 41001）`);
  else if (ed[1] !== 0) bad(`41001 ret=${ed[1]}（应为 0）`);
  else if (data[1] !== 1) bad(`41001 checkpointId=${data[1]}（应为 1）`);
  else if (data[2] !== 1) bad(`41001 seasonId=${data[2]}（应为 1）`);
  else if (s(data, 5) !== '12250000000') bad(`41001 npcHp=${s(data, 5)}（应为第 1 关 cost 12250000000）`);
  else ok(`41001 checkpointId=1 seasonId=1 npcHp=${s(data, 5)}`);

  // ── 2. 41002 type=0 ──
  const r2 = await c.call(41002, req41002(0));
  const r2d = parse(r2.body);
  const rd = r2d[2] ? parse(r2d[2]) : {};
  if (r2.cmd !== 41002 || r2d[1] !== 0) bad(`41002 ret=${r2d[1]} 帧号=${r2.cmd}`);
  else if (rd[4] !== -1) bad(`41002 index=${rd[4]}（应为 -1：没打过就不在榜）`);
  else ok(`41002 type=0 ret=0 index=-1 progress=${rd[3]}`);

  // ── 3. 41002 type=1 —— 原版对 type 不判合法性 ──
  const r3 = await c.call(41002, req41002(1));
  const r3d = parse(r3.body);
  if (r3.cmd !== 41002 || r3d[1] !== 0) bad(`41002 type=1 ret=${r3d[1]}`);
  else ok('41002 type=1 ret=0（原版不校验 type）');

  // ── 4. 41003 seasonId 不符 → 3004 ──
  const n4 = await c.call(41003, req41003(0, 1, 999));
  if (n4.cmd !== 1) bad(`41003 seasonId=999 帧号=${n4.cmd}（应为 1）`);
  else if (n4.head[2] !== 3004) bad(`41003 seasonId=999 code=${n4.head[2]}（应为 3004）`);
  else ok('41003 seasonId=999 → 3004');

  // ── 5. 41003 正常单次 ──
  const n5 = await c.call(41003, req41003(0, 1, 1));
  const n5d = parse(n5.body);
  if (n5.cmd !== 41003) bad(`41003 帧号=${n5.cmd}（应为 41003） ${n5.cmd === 1 ? 'code=' + n5.head[2] : ''}`);
  else if (n5d[1] !== 0) bad(`41003 ret=${n5d[1]}`);
  else if (n5d[7] !== 0) bad(`41003 winNum=${n5d[7]}（20000 银两打不动第 1 关，应为 0）`);
  // `calNewCost`：rate = max(min 0.2, npcPower/totalMoney) = 0.2（totalMoney=0 走兜底）
  //   subtractCost = 20000 / 0.2 = 100000（DOWN）
  //   costMoney    = 100000 * 0.2 = 20000，不小于 preMoney → 不兜底
  //   damageHp     = subtractCost = 100000
  else if (s(n5d, 8) !== '100000') bad(`41003 damageHp=${s(n5d, 8)}（应为 100000，见 AUDIT A24）`);
  else if (s(n5d, 3) !== '20000') bad(`41003 costMoney=${s(n5d, 3)}（20000 全押）`);
  else ok(`41003 单次 lose winNum=0 pre=${s(n5d, 2)} cost=${s(n5d, 3)} damage=${s(n5d, 8)}`);
  console.log('    DEBUG 41003 resp:', JSON.stringify(Object.fromEntries(
    Object.entries(n5d).map(([k, v]) => [k, Buffer.isBuffer(v) ? v.toString('utf8') : v]))));

  // ── 6. 41003 一键未解锁 → 3003 ──
  const n6 = await c.call(41003, req41003(1, 99999, 1));
  if (n6.cmd !== 1) bad(`41003 type=1 帧号=${n6.cmd}（应为 1）`);
  else if (n6.head[2] !== 3003) bad(`41003 type=1 code=${n6.head[2]}（应为 3003 一键未解锁）`);
  else ok('41003 type=1 num=99999 → 3003');

  c.close();
  console.log('\n%s 远洋贸易冒烟', fails === 0 ? '✅' : '❌');
  if (fails) process.exit(1);
};

main().catch((e) => { console.error('❌', e.message); process.exit(1); });
