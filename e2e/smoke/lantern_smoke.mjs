#!/usr/bin/env node
// 猜灯谜 37603/37604/37605/37607 冒烟。
//
// 线上数据真实状态（`data/sql/platform.sql` 实测）：
//   `bf_activity` **没有 typeId = 50 的行**；`bf_operate_lantern_riddle` 与
//   `bf_operate_riddle_reward` 都是空表；`bf_config` 也没有
//   `LanternRiddleLimitNum` / `LanternRiddleLLayTime`。
//
// 所以原版 `getActivityDbTemplate(50, actId)` 拿到 null，
// `dbTemplate.isOpen()` **NPE**，被框架兜底成 `COMMON_SERVER_BUSY(2)`。
//
// 断言（`LanternRiddlesMessageHandler.java`）：
//   1. 37603 actId=1 → 帧号 1，code=2（模板 None，原版 NPE 兜底）
//   2. 37604 actId=1 id=1 lanternId=1 → 帧号 1，code=2（同上，先于 id 越界判定）
//   3. 37604 actId=1 id=9 → **仍是** 2，证明 isOpen 在 id 校验之前
//   4. 37605 actId=1 lanternId=1 → 帧号 1，code=2
//   5. 37607 actId=1 → 帧号 1，code=2
//   6. actId=0 → 帧号 1，code=2（空 actId 也一样）
//
// 用法：node e2e/smoke/lantern_smoke.mjs [ws://127.0.0.1:10544/]

const URL = process.argv[2] ?? 'ws://127.0.0.1:10544/';
const OPEN_ID = 'lr-' + Date.now();
const NAME = 'LR' + (Date.now() % 1000000);
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

// MsgLanternRiddleGetSubjectReq_37603 { actId=1 }
const req37603 = (actId) => Buffer.from([...i32(1, actId)]);
// MsgLanternRiddleChangeSubjectReq_37604 { actId=1, id=2, lanternId=3 }
const req37604 = (actId, id, lanternId) =>
  Buffer.from([...i32(1, actId), ...i32(2, id), ...i32(3, lanternId)]);
// MsgLanternRiddleExcludeOptionReq_37605 { actId=1, lanternId=2 }
const req37605 = (actId, lanternId) => Buffer.from([...i32(1, actId), ...i32(2, lanternId)]);
// MsgLanternRiddleGetRwdReq_37607 { actId=1 }
const req37607 = (actId) => Buffer.from([...i32(1, actId)]);

const expect = async (c, label, cmd, body, want) => {
  try {
    const g = await c.call(cmd, body);
    if (g.cmd !== 1) bad(`${label} 帧号=${g.cmd}（应为 1，走 ErrorResp_1）`);
    else if (g.head[2] !== want) bad(`${label} code=${g.head[2]}（应为 ${want}）`);
    else ok(`${label} → ${want}`);
  } catch (e) { bad(`${label} ${e.message}`); }
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

  // 四条消息的入口第一段完全一样：模板 None / 不在时间 → COMMON_SERVER_BUSY(2)。
  // 线上 bf_activity 没有 typeId=50 的行，所以等价类只有「恒 2」这一个。
  await expect(c, '37603 actId=1 取题', 37603, req37603(1), 2);
  await expect(c, '37604 actId=1 id=2 答题', 37604, req37604(1, 2, 3), 2);
  // ⚠ id=9 越界（合法值 1..4），但 isOpen 在 id 校验之前 → 仍回 2，不是 5401
  await expect(c, '37604 actId=1 id=9（越界但仍先判 isOpen）', 37604, req37604(1, 9, 3), 2);
  await expect(c, '37605 actId=1 排错', 37605, req37605(1, 2), 2);
  await expect(c, '37607 actId=1 领奖', 37607, req37607(1), 2);
  // actId=0 也走同一条 null 分支
  await expect(c, '37603 actId=0', 37603, req37603(0), 2);

  c.close();
  console.log('\n%s 猜灯谜冒烟', fails === 0 ? '✅' : '❌');
  if (fails) process.exit(1);
};

main().catch((e) => { console.error('❌', e.message); process.exit(1); });
