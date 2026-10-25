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
const frame = (cmdId, body) => {
  const b = Buffer.alloc(10 + body.length);
  b.writeUInt16BE(29099, 0); b.writeInt32BE(10 + body.length, 2); b.writeInt32BE(cmdId, 6);
  Buffer.from(body).copy(b, 10);
  return b;
};

const rpc = (cmdId, body) => new Promise((resolve, reject) => {
  const ws = new WebSocket(URL);
  ws.binaryType = 'arraybuffer';
  ws.onopen = () => ws.send(frame(cmdId, body));
  ws.onmessage = (e) => {
    const b = Buffer.from(e.data);
    ws.close();
    resolve({ cmd: b.readInt32BE(6), head: parse(b.subarray(10)) });
  };
  ws.onerror = () => reject(new Error('WebSocket 连接失败：' + URL));
  setTimeout(() => reject(new Error('超时 cmd=' + cmdId)), 8000);
});

const login = async () => {
  const plain = Buffer.from([
    ...str(1, OPEN_ID), ...i32(2, SERVER_ID), ...str(3, 'tok'),
    ...i32(11, 1), ...i32(14, 1), ...str(42, 'zh_CN'), ...i32(31, 0),
  ]);
  const enc = xxteaEncrypt(Buffer.from(plain.toString('base64'), 'binary'), SIGN_KEY);
  return rpc(4, enc);
};

const create = () => rpc(6, Buffer.from([
  ...str(1, OPEN_ID), ...i32(2, SERVER_ID), ...str(3, NAME), ...str(4, '1'),
  ...i32(5, 1), ...str(6, String(Math.floor(Date.now() / 1000))),
  ...str(7, 'tok'), ...str(8, 'x'), ...i32(10, 1), ...i32(11, 1), ...str(32, 'zh_CN'),
]));

const main = async () => {
  const first = await login();
  console.log('① 登录      cmd=%d ret=%d ok=%d', first.cmd, first.head[1], first.head[2] ?? 0);

  let created = { head: {} };
  if (!first.head[2]) {
    created = await create();
    console.log('② 创角      cmd=%d ret=%d', created.cmd, created.head[1]);
  } else {
    console.log('② 创角      跳过（角色已存在）');
  }

  const again = await login();
  const loginvar = again.head[5];
  console.log('③ 再登录    cmd=%d ret=%d ok=%d loginvar=%d', again.cmd, again.head[1], again.head[2] ?? 0, loginvar ?? 0);

  const ok = first.head[1] === 0 && again.head[1] === 0 && again.head[2] === 1;
  if (!ok) { console.error('\n❌ 冒烟失败'); process.exit(1); }
  console.log('\n✅ 冒烟通过：WS 传输 + XXTEA 登录 + 创角 + 角色绑定全部对齐');
};

main().catch((e) => { console.error('❌', e.message); process.exit(1); });
