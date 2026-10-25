//! 无头客户端 CLI：直接跑真实服务端。
//!
//! ```bash
//! export DDJ_CLIENT_ASSETS=/path/to/apk/assets/assets
//! node run.js login  --open-id open-001
//! node run.js create --open-id open-001 --name 测试角色
//! node run.js item-use --open-id open-001 --template 400001 --num 3
//! node run.js flow   --open-id open-001 --name 测试角色
//! ```
//!
//! 帧格式用客户端原类（`lets.ByteBuffer`），消息体用客户端 proto 文本，
//! 登录包（cmd 4）走 XXTEA + Base64（与客户端/服务端同一把静态密钥）。

'use strict';
const { boot } = require('./boot.js');
const { Client } = require('./client.js');
const pb = require('./proto.js');
const xxtea = require('./xxtea.js');

function parseArgs(argv) {
  const [cmd, ...rest] = argv;
  const opts = {};
  for (let i = 0; i < rest.length; i += 2) {
    const k = rest[i];
    if (!k || !k.startsWith('--')) continue;
    opts[k.slice(2).replace(/-([a-z])/g, (_, c) => c.toUpperCase())] = rest[i + 1];
  }
  return { cmd, opts };
}

/** 登录：cmd 4，body 加密 */
async function login(c, opts) {
  const req = {
    openId: opts.openId || 'open-001',
    serverId: Number(opts.serverId || 7010544),
    platform: 1,
    channelId: opts.channelId || '0',
    version: Number(opts.version || 0),
    lan: opts.lan || "zh_CN",
  };
  const body = xxtea.wrapLogin(pb.encode('LoginReq_4', req));
  const f = await c.call(4, body);
  // 实测：服务端**只解密请求**，回包是明文 protobuf（`08 00 10 00` = ret 0 / ok false）。
  // 仍保留 XXTEA 兜底，以防某些分支真的加密。
  let resp;
  try {
    resp = pb.decode('LoginResp_4', f.body);
  } catch (e) {
    const plain = xxtea.unwrapLogin(f.body);
    if (!plain) throw new Error(`cmd 4 回包无法解析（${f.body.length} 字节: ${f.body.toString('hex')}）`);
    resp = pb.decode('LoginResp_4', plain);
  }
  console.log('LoginResp_4 =', JSON.stringify(resp));
  return resp;
}


/** 客户端握手密码：RandomSeed(loginvar).random(1,10000) + 9527 → md5 → 小写 */
function clientPassword(seed) {
  let state = seed;
  const step = () => {
    const n = 17 * state + 139;
    const i = Math.floor(n / 256);
    state = n - 256 * i;
    return state / 256;
  };
  const r = step();
  const v = 1 + Math.floor(r * (10000 - 1 + 1));
  return require('crypto').createHash('md5').update(String(v + 9527)).digest('hex').toLowerCase();
}

/** 握手：cmd 16 / 17 / 18，itemId 位置回传密码 */
async function handshake(c, loginvar, opts) {
  const pwd = opts.password || clientPassword(loginvar);
  // 原版 cmd 16/17/18 的 handler 返回 void、**不回包**；只有被守卫拒绝才会收到 ErrorResp_1。
  // 所以"等一个短超时"就是正确语义。
  const f = await c.call(16, pb.encode('PlayerGetItemReq_16', { itemId: pwd }), 1200).catch(() => null);
  if (!f) {
    console.log('握手通过（无回包 = 原版语义）pwd =', pwd);
    return true;
  }
  if (f.cmdId === 1) {
    const err = pb.decode('ErrorResp_1', f.body);
    console.log('握手失败 ErrorResp_1 =', JSON.stringify(err), 'pwd =', pwd);
    return false;
  }
  console.log('握手通过（cmd', f.cmdId, '）pwd =', pwd);
  return true;
}

/** 创角：cmd 6 */
async function create(c, opts) {
  const req = {
    openId: opts.openId || 'open-001',
    serverId: Number(opts.serverId || 7010544),
    name: opts.name || '测试角色',
    ico: '1',
    sex: '1',
    channelId: opts.channelId || '0',
    platform: 1,
    lan: opts.lan || 'zh_CN',
  };
  const f = await c.call(6, pb.encode('CreatePlayerReq_6', req));
  let resp;
  try {
    resp = pb.decode('CreatePlayerResp_6', f.body);
  } catch (e) {
    // 分支 1/5/6 原版回的是 LoginResp_4 双信封
    const plain = xxtea.unwrapLogin(f.body);
    resp = plain ? pb.decode('LoginResp_4', plain) : { raw: f.body.toString('hex') };
    console.log('（回包是 LoginResp_4 信封）');
  }
  console.log('CreatePlayer =', JSON.stringify(resp));
  return resp;
}

/** 使用道具：cmd 11002 */
async function itemUse(c, opts) {
  const req = {
    templateId: Number(opts.template || 400001),
    num: Number(opts.num || 1),
  };
  const f = await c.call(11002, pb.encode('ItemUseReq_11002', req));
  const resp = pb.decode('ItemUseResp_11002', f.body);
  console.log('ItemUseResp_11002 =', JSON.stringify(resp));
  return resp;
}

async function main() {
  const { cmd, opts } = parseArgs(process.argv.slice(2));
  if (!cmd) {
    console.error('用法: node run.js <login|create|item-use|flow> [--open-id ...]');
    process.exit(2);
  }
  const env = boot();
  console.log(`客户端 bundle 已加载：lets 导出 ${Object.keys(env.lets).length} 个`);
  const c = new Client(env);
  await c.connect(opts.host || '127.0.0.1', Number(opts.port || 10544));
  console.log(`已连接 ${opts.host || '127.0.0.1'}:${opts.port || 10544}`);
  try {
    if (cmd === 'login') await login(c, opts);
    else if (cmd === 'create') await create(c, opts);
    else if (cmd === 'handshake') {
      const l = await login(c, opts);
      await handshake(c, l.loginvar, opts);
    } else if (cmd === 'item-use') await itemUse(c, opts);
    else if (cmd === 'flow') {
      const l1 = await login(c, opts);
      if (!l1.ok) await create(c, opts);
      const l2 = await login(c, opts);
      if (l2.ok) await handshake(c, l2.loginvar, opts);
    } else {
      console.error(`未知子命令 ${cmd}`);
      process.exit(2);
    }
  } finally {
    c.close();
  }
}

main().catch((e) => {
  console.error('失败:', e.message);
  process.exit(1);
});
