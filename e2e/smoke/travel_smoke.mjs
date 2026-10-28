#!/usr/bin/env node
// 游历 15002/15004/15009 冒烟。
//
// 真实配置（`B-变量表-Config.csv` 实测）：
//   PhysicalStrength=1 / FixedPointTravel=24=1 / PartnerInviteTime=50
//   PartnerChangeCost=1=500 / TravelFirstEventOffspring=4
//   TravelFirstEventGirl = 1=9101;2=12401;3=7301;4=12401;5=2101
// 真实事件表 `Y-游历事件-TravelEvents.csv` = 84 行，**没有 place=0**。
//
// 断言（`TravelMessageHandler.java`）：
//   1. 15002 place=99 → 帧号 1，code=1006（TRAVEL_ERROR_PLACE_LIMIT，最先判）
//   2. 15002 place=0（体力 0）→ 帧号 1，code=1001（TRAVEL_ERROR_PHYSICAL_LIMIT）
//   3. 15002 place=12 → 帧号 1，code=2356（NPC_NO_OPEN；26 号开放未满足）
//   4. 15004 → 帧号 1，code=1003 或 2（133 未开 → 1003；开了但无体力无道具 → 2）
//   5. 15009 sex=9 → 帧号 1，code=2（COMMON_SERVER_BUSY，sex 越界先判）
//   6. 15009 sex=0 → 要么 1003（161 未开）要么 2（扣不起 1×500）
//
// 用法：node e2e/smoke/travel_smoke.mjs [ws://127.0.0.1:10544/]

const URL = process.argv[2] ?? 'ws://127.0.0.1:10544/';
const OPEN_ID = 'tv-' + Date.now();
const NAME = 'TV' + (Date.now() % 1000000);
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

  // MsgTravelReq_15002 { place=1  allTimes=2  travelCnt=3 }
  const req15002 = (place, allTimes = 0, travelCnt = 0) =>
    Buffer.from([...i32(1, place), ...i32(2, allTimes), ...i32(3, travelCnt)]);

  // 1) place=99 → 1006（三个前置里最先判 place）
  try {
    const g = await c.call(15002, req15002(99));
    if (g.cmd !== 1) bad(`15002 place=99 帧号=${g.cmd}（应为 1，走 ErrorResp_1）`);
    else if (g.head[2] !== 1006) bad(`15002 place=99 code=${g.head[2]}（应为 1006）`);
    else ok('15002 place=99 → 1006 TRAVEL_ERROR_PLACE_LIMIT');
  } catch (e) { bad('15002 place=99 ' + e.message); }

  // 2) place=0，新号体力为 0 → 1001
  try {
    const g = await c.call(15002, req15002(0));
    if (g.cmd !== 1) bad(`15002 place=0 帧号=${g.cmd}（应为 1）`);
    else if (g.head[2] !== 1001) bad(`15002 place=0 code=${g.head[2]}（应为 1001，体力不足）`);
    else ok('15002 place=0 → 1001 TRAVEL_ERROR_PHYSICAL_LIMIT');
  } catch (e) { bad('15002 place=0 ' + e.message); }

  // 3) place=12 → 26 号开放未满足 → 2356
  try {
    const g = await c.call(15002, req15002(12));
    if (g.cmd !== 1) bad(`15002 place=12 帧号=${g.cmd}（应为 1）`);
    else if (g.head[2] !== 2356) bad(`15002 place=12 code=${g.head[2]}（应为 2356 NPC_NO_OPEN）`);
    else ok('15002 place=12 → 2356 NPC_NO_OPEN');
  } catch (e) { bad('15002 place=12 ' + e.message); }

  // 4) 15004 十连。MsgTravelTenReq_15004 是空消息。
  try {
    const g = await c.call(15004, Buffer.alloc(0));
    if (g.cmd !== 1) bad(`15004 帧号=${g.cmd}（应为 1）`);
    else if (![1003, 2].includes(g.head[2]))
      bad(`15004 code=${g.head[2]}（应为 1003 未开 或 2 资源不足）`);
    else ok(`15004 → ${g.head[2]} ${g.head[2] === 1003 ? '未开放' : '资源不足'}`);
  } catch (e) { bad('15004 ' + e.message); }

  // 5) 15009 sex=9 越界 → 2（sex 检查在 isOpen 之前）
  try {
    const g = await c.call(15009, Buffer.from([...i32(1, 9)]));
    if (g.cmd !== 1) bad(`15009 sex=9 帧号=${g.cmd}（应为 1）`);
    else if (g.head[2] !== 2) bad(`15009 sex=9 code=${g.head[2]}（应为 2，sex 越界先判）`);
    else ok('15009 sex=9 → 2 COMMON_SERVER_BUSY（sex 越界先于 isOpen）');
  } catch (e) { bad('15009 sex=9 ' + e.message); }

  // 6) 15009 sex=0 → 1003 或 2
  try {
    const g = await c.call(15009, Buffer.from([...i32(1, 0)]));
    if (g.cmd !== 1) bad(`15009 sex=0 帧号=${g.cmd}（应为 1）`);
    else if (![1003, 2].includes(g.head[2]))
      bad(`15009 sex=0 code=${g.head[2]}（应为 1003 未开 或 2 扣不起）`);
    else ok(`15009 sex=0 → ${g.head[2]} ${g.head[2] === 1003 ? '未开放' : '资源不足'}`);
  } catch (e) { bad('15009 sex=0 ' + e.message); }

  c.close();
  console.log('\n%s 游历冒烟', fails === 0 ? '✅' : '❌');
  if (fails) process.exit(1);
};

main().catch((e) => { console.error('❌', e.message); process.exit(1); });
