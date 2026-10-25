//! 在 Node 里加载**真实的客户端 bundle**，提供最小 shim。
//!
//! 目标：不重新实现协议，而是直接跑反编译出来的 `assets/main/index.js`，
//! 让帧格式、protobuf 编解码、XXTEA 全部由客户端自己的代码完成。
//!
//! 关键事实（实测）：
//! - bundle 是 Cocos Creator 2.4 的构建产物，模块系统是 `window.__require`，
//!   模块**懒执行**：不 require 就不跑，所以大部分 `cc.xxx` 依赖不会触发。
//! - 加载后 `globalThis.lets` 上有 110 个导出，其中网络层齐全：
//!   `WebPacket`、`ByteBuffer`、`WebSocketService`、`BaseSocket`、
//!   `encodeProto`/`decodeProto`、`ProtoBufExt`、`SOCKET_EVENT_CONST`。
//! - `cc` 用 Proxy 兜底成"什么都能点、点了还是自己"的对象即可完成加载。

'use strict';
const fs = require('fs');
const path = require('path');
const vm = require('vm');

/** 造一个万能桩：任何属性访问都返回自身，可调用可 new。 */
function makeStub() {
  const stub = new Proxy(function () {}, {
    get(_t, key) {
      if (key === Symbol.toPrimitive) return () => 0;
      if (key === 'toString' || key === 'valueOf') return () => '';
      if (key === Symbol.iterator) return undefined;
      if (key === 'length') return 0;
      return stub;
    },
    apply: () => stub,
    construct: () => stub,
  });
  return stub;
}

/**
 * 客户端默认从 `assets/` 找 bundle；可用环境变量覆盖：
 *   DDJ_CLIENT_ASSETS=/path/to/apk-extract/assets/assets
 */
function assetsDir() {
  const fromEnv = process.env.DDJ_CLIENT_ASSETS;
  if (fromEnv) return fromEnv;
  return path.resolve(__dirname, 'vendor/assets');
}

/**
 * 启动一个客户端运行环境。
 * @param {object} [opts]
 * @param {string[]} [opts.bundles] 要加载的 bundle 名，默认 `['main']`
 * @returns {{sandbox: object, lets: object, load: (name: string) => void}}
 */
function boot(opts = {}) {
  const dir = opts.assetsDir || assetsDir();
  const bundles = opts.bundles || ['main'];

  const noop = () => 0;
  const ccStub = makeStub();
  ccStub.sys = { isNative: false, platform: 'node', localStorage: {} };

  const sandbox = {
    console,
    setTimeout,
    setInterval,
    clearTimeout,
    clearInterval,
    setImmediate,
    Uint8Array,
    Int8Array,
    Uint16Array,
    Int16Array,
    Uint32Array,
    Int32Array,
    Float32Array,
    Float64Array,
    ArrayBuffer,
    DataView,
    TextEncoder,
    TextDecoder,
    Buffer,
    Math,
    Date,
    JSON,
    Object,
    Array,
    String,
    Number,
    Boolean,
    RegExp,
    Error,
    TypeError,
    Promise,
    Map,
    Set,
    WeakMap,
    WeakSet,
    Symbol,
    Reflect,
    Proxy,
    parseInt,
    parseFloat,
    isNaN,
    isFinite,
    encodeURIComponent,
    decodeURIComponent,
    cc: ccStub,
    document: { createElement: () => ccStub, addEventListener: noop },
    localStorage: { getItem: () => null, setItem: noop, removeItem: noop },
    navigator: { userAgent: 'ddj-jsclient' },
    performance: { now: () => Date.now() },
    location: { href: '' },
    // Node 22+ 自带 WebSocket；真实 socket 交给 WebSocketService 使用
    WebSocket: globalThis.WebSocket,
  };
  sandbox.window = sandbox;
  sandbox.globalThis = sandbox;
  sandbox.self = sandbox;

  const ctx = vm.createContext(sandbox);

  const load = (name) => {
    const file = path.join(dir, name, 'index.js');
    const code = fs.readFileSync(file, 'utf8');
    vm.runInContext(code, ctx, { filename: `${name}/index.js`, timeout: 120_000 });
  };

  for (const b of bundles) load(b);

  if (!sandbox.lets) {
    throw new Error(
      `bundle 加载后没有 lets 命名空间；请检查 DDJ_CLIENT_ASSETS=${dir} 是否为 APK 的 assets/assets 目录`,
    );
  }
  return { sandbox, lets: sandbox.lets, load, ctx };
}

module.exports = { boot, assetsDir, makeStub };
