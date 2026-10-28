#!/usr/bin/env node
// 系统开放 + 活动模板 + 聚宝盆 全链路冒烟（2026-10-28 实测数据）。
//
// ## 真实数据
// - `G-功能开启表-SystemOpen.csv`：100 行
// - PG `bf_server.open_time` = 1733356835000（2024-12-05 03:00:35 +08:00）
// - PG `bf_activity`：11 行，时间全是 2023-07-20 09:00:00 ~ 2026-07-26 23:59:59
// - `bf_config.configId` 23/24 是**空串** → `cidTimeMap`/`cidChargeMap` 空
//
// ## 断言
//   1. 1015 已知 systemId（如 120）  → 帧号 1015，ret=0
//      （`name = change_word_607`，`getDescribeName` 取末段 `607` 能 parseInt）
//   2. 1015 未知 systemId（如 99999）→ 帧号 1015，**ret=2**
//      （原版 `getSystemOpenDB(id)` 为 null → NPE → 外层 catch → COMMON_SERVER_BUSY）
//   3. 17071 开服第 N 天：120 = `13=2` → 开服 2 天后才开放。
//      本服 open_time 是 2024-12-05，现在（2026-10）已 690+ 天 → **应已开放**，
//      于是往下走到「资源不足」→ 帧号 1，code=3902
//   4. 38303 聚宝盆记录：`bf_activity` 里**没有 typeId=60 的行**
//      → 帧号 1，code=2751（ACTIVITY_NO_EXITS）
//   5. 38304 同上 → 2751
//   6. 38303 带一个不存在的 activityId 也应是 2751
//
// 用法：node e2e/smoke/system_open_smoke.mjs [ws://127.0.0.1:10544/]

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

  // ---- 1015 系统开放：真实模板 ----
  // 实测 CSV 有 120 行，name = `change_word_607`。
  // `LogicUtils.getDescribeName` 按 `_` 切分取末段 → `607` → parseInt 成功 → ret=0。
  try {
    const g = await c.call(1015, Buffer.from([...i32(1, 120)]));
    if (g.cmd !== 1015) bad(`1015 已知 id 帧号=${g.cmd}（应为 1015）`);
    else if (g.head[1] !== 0) bad(`1015 已知 id ret=${g.head[1]}（应为 0）`);
    else ok('1015 已知 id=120 → ret=0（真实模板 + getDescribeName 取末段 607）');
  } catch (e) { bad('1015 已知 id ' + e.message); }

  // 未知 id → 原版 `getSystemOpenDB(99999)` 为 null → NPE → catch → ret=2
  try {
    const g = await c.call(1015, Buffer.from([...i32(1, 99999)]));
    if (g.cmd !== 1015) bad(`1015 未知 id 帧号=${g.cmd}（应为 1015）`);
    else if (g.head[1] !== 2) bad(`1015 未知 id ret=${g.head[1]}（应为 2 = COMMON_SERVER_BUSY）`);
    else ok('1015 未知 id=99999 → ret=2（原版 NPE 被 catch）');
  } catch (e) { bad('1015 未知 id ' + e.message); }

  // ---- 17071 焚香祈愿：isOpen(120) 门禁 ----
  // 120 = `13 = 2`（开服 2 天）。本服 open_time=2024-12-05，现已 690+ 天 → 已开放。
  // 新号没有 263 道具 → 走到「资源不足」→ ErrorResp_1{code=3902}
  try {
    const g = await c.call(17071, Buffer.from([...i32(1, 10)]));
    if (g.cmd !== 1) bad(`17071 十连 帧号=${g.cmd}（应为 1）`);
    else if (g.head[2] !== 3902) bad(`17071 十连 code=${g.head[2]}（应为 3902 = PRAY_ERROR_COST）`);
    else ok('17071 十连 → ErrorResp_1{3902}（isOpen(120) 已过，卡在资源不足）');
  } catch (e) { bad('17071 十连 ' + e.message); }

  // ---- 38303 / 38304 聚宝盆：活动模板 ----
  // `bf_activity` 11 行里**没有 typeId=60**（Cornucopia），所以模板查不到 → 2751。
  const expectActivityNoExits = async (cmd, actId, label) => {
    try {
      const g = await c.call(cmd, Buffer.from([...i32(1, actId)]));
      if (g.cmd !== 1) bad(`${label} 帧号=${g.cmd}（应为 1 = ErrorResp_1）`);
      else if (g.head[2] !== 2751) bad(`${label} code=${g.head[2]}（应为 2751 = ACTIVITY_NO_EXITS）`);
      else ok(`${label} → ErrorResp_1{2751}（bf_activity 无 typeId=60）`);
    } catch (e) { bad(`${label} ${e.message}`); }
  };
  await expectActivityNoExits(38303, 100043242, '38303 自己的记录');
  await expectActivityNoExits(38304, 100043242, '38304 全服记录');
  await expectActivityNoExits(38303, 1, '38303 不存在的 activityId');

  c.close();
  console.log('\n%s 系统开放 + 活动模板全链路', fails === 0 ? '✅' : '❌');
  if (fails) process.exit(1);
};

main().catch((e) => { console.error(e); process.exit(1); });
