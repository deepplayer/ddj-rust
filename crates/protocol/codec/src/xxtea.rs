//! XXTEA 加解密
//!
//! 对应 Java：`bf.game.server.utils.XXeta`（已用 `tools/jclass2.py` 反汇编核对）。
//!
//! ## 服务端在什么时候用它
//! 只有一条消息：**`msgId == 4`（登录 `LoginReq_4`）**。
//! 处理链：`XXeta.decrypt(body, SIGN_KEY)` → 得到 base64 文本 → `Base64.getDecoder().decode` → protobuf。
//!
//! ## 密钥是厂家写死的 16 字节
//! `{48,53,58,38,40,41,94,38,42,97,98,100,100,122,77,81}`
//! = ASCII `05:&()^&*abddzMQ`，与客户端 `lets` 里的字符串常量完全一致
//! （`main.js` 字符串表第 1308 项）。这是厂家留的「测试登录」后门，不是配置项。
//!
//! ## 算法来源（双源互证）
//! 服务端：`XXeta.decrypt([I[I)[I` 字节码 —— 轮数 `6 + 52/n`、delta `2654435769`。
//! 客户端：`nat_module.js:10580` 起那份内联的开源 XXTEA（`u`=加密 / `d`=解密 / `c`=mx）。
//! 两份完全一致，就是标准 XXTEA，只是**字节序是小端**（`r()`: `n[a>>2] |= c << ((3&a)<<3)`）。
//!
//! ## 长度字的坑
//! Java 侧 `encrypt(byte[],byte[])` 调 `toIntArray(data, **true**)`，
//! 会把原始字节长度作为一个额外的 `u32` 追加到数组末尾；
//! `decrypt(byte[],byte[])` 末尾用 `toByteArray(v, **true**)` 把它读回来并截断。
//! 所以字节级解密必须还原这个长度字，否则输出尾部会多 0~3 个填充字节。

/// 厂家写死的登录密钥（与客户端一致）。
pub const SIGN_KEY: [u8; 16] = [
    48, 53, 58, 38, 40, 41, 94, 38, 42, 97, 98, 100, 100, 122, 77, 81,
];

/// delta，即 ⌊(√5−1)/2 · 2³²⌋。客户端里写作 `2654435769`。
const DELTA: u32 = 0x9E37_79B9;

/// `mx` 函数，对应客户端 `c(e, t, n, i, s, a)`。
///
/// 展开后：
/// `((z >>> 5 ^ y << 2) + (y >>> 3 ^ z << 4)) ^ ((sum ^ y) + (k[(3 & p) ^ e] ^ z))`
fn mx(sum: u32, y: u32, z: u32, p: usize, e: u32, k: &[u32; 4]) -> u32 {
    (((z >> 5) ^ (y << 2)).wrapping_add((y >> 3) ^ (z << 4)))
        ^ ((sum ^ y).wrapping_add(k[(3 & p) ^ e as usize] ^ z))
}

/// 轮数：`⌊6 + 52 / n⌋`（n 为 `u32` 个数）。
const fn rounds(n: usize) -> u32 {
    6 + 52 / n as u32
}

/// 字节 → `u32` 数组（**小端**，对应客户端 `r(e, t)`）。
fn to_words(data: &[u8], include_len: bool) -> Vec<u32> {
    let mut n = data.len().div_ceil(4);
    if include_len {
        n += 1;
    }
    let mut v = vec![0u32; n];
    for (i, &b) in data.iter().enumerate() {
        v[i >> 2] |= u32::from(b) << ((3 & i) << 3);
    }
    if include_len {
        v[n - 1] = data.len() as u32;
    }
    v
}

/// `u32` 数组 → 字节（**小端**，对应客户端 `a(e, t)`）。
///
/// `include_len` 为真时把最后一个 `u32` 当作原始长度并校验范围；
/// 校验失败返回 `None`（等价于 Java 返回 `null`）。
fn from_words(v: &[u32], include_len: bool) -> Option<Vec<u8>> {
    let mut n = v.len() << 2;
    if include_len {
        let m = v[v.len() - 1] as usize;
        n -= 4;
        // 客户端的判据：`s < (i -= 4) - 3 || s > i`
        if m < n.saturating_sub(3) || m > n {
            return None;
        }
        n = m;
    }
    let mut out = Vec::with_capacity(v.len() * 4);
    for w in v {
        out.extend_from_slice(&w.to_le_bytes());
    }
    out.truncate(n);
    Some(out)
}

fn key_words(key: &[u8; 16]) -> [u32; 4] {
    let mut k = [0u32; 4];
    for (i, slot) in k.iter_mut().enumerate() {
        let mut b = [0u8; 4];
        b.copy_from_slice(&key[i * 4..i * 4 + 4]);
        *slot = u32::from_le_bytes(b);
    }
    k
}

/// 就地加密 `u32` 数组（对应 `XXeta.encrypt([I[I)[I` / 客户端 `u()`）。
pub fn encrypt_int(v: &mut [u32], k: &[u32; 4]) {
    let n = v.len();
    if n < 2 {
        return;
    }
    let p = n - 1;
    let q = rounds(n);
    let mut sum: u32 = 0;
    let mut z = v[p];
    for _ in 0..q {
        sum = sum.wrapping_add(DELTA);
        let e = (sum >> 2) & 3;
        for i in 0..p {
            let y = v[i + 1];
            z = v[i].wrapping_add(mx(sum, y, z, i, e, k));
            v[i] = z;
        }
        let y = v[0];
        z = v[p].wrapping_add(mx(sum, y, z, p, e, k));
        v[p] = z;
    }
}

/// 就地解密 `u32` 数组（对应 `XXeta.decrypt([I[I)[I` / 客户端 `d()`）。
pub fn decrypt_int(v: &mut [u32], k: &[u32; 4]) {
    let n = v.len();
    if n < 2 {
        return;
    }
    let p = n - 1;
    let q = rounds(n);
    let mut sum = q.wrapping_mul(DELTA);
    let mut y = v[0];
    while sum != 0 {
        let e = (sum >> 2) & 3;
        for i in (1..=p).rev() {
            let z = v[i - 1];
            y = v[i].wrapping_sub(mx(sum, y, z, i, e, k));
            v[i] = y;
        }
        let z = v[p];
        y = v[0].wrapping_sub(mx(sum, y, z, 0, e, k));
        v[0] = y;
        sum = sum.wrapping_sub(DELTA);
    }
}

/// 字节级加密（追加长度字，与 Java `encrypt(byte[],byte[])` 等价）。
#[must_use]
pub fn encrypt(data: &[u8], key: &[u8; 16]) -> Vec<u8> {
    if data.is_empty() {
        return Vec::new();
    }
    let k = key_words(key);
    let mut v = to_words(data, true);
    encrypt_int(&mut v, &k);
    let mut out = Vec::with_capacity(v.len() * 4);
    for w in &v {
        out.extend_from_slice(&w.to_le_bytes());
    }
    out
}

/// 字节级解密（读取长度字并截断，与 Java `decrypt(byte[],byte[])` 等价）。
///
/// 返回 `None` 表示长度字越界 —— 客户端 `a(e, true)` 在这种情况下返回 `null`，
/// 服务端会把它当解密失败处理。
#[must_use]
pub fn decrypt(data: &[u8], key: &[u8; 16]) -> Option<Vec<u8>> {
    if data.is_empty() {
        return Some(Vec::new());
    }
    let k = key_words(key);
    let mut v = to_words(data, false);
    if v.len() < 2 {
        return Some(data.to_vec());
    }
    decrypt_int(&mut v, &k);
    from_words(&v, true)
}

/// 登录包的完整解包：`密文 → base64 文本 → protobuf 字节`。
///
/// # Errors
/// XXTEA 长度字越界，或内容不是合法 base64。
pub fn unpack_login_body(body: &[u8], key: &[u8; 16]) -> Result<Vec<u8>, LoginBodyError> {
    use base64::Engine as _;
    let text = decrypt(body, key).ok_or(LoginBodyError::LengthWord)?;
    // 客户端写出的尾部可能有 `\0` 填充，base64 解码器不接受，先剔掉。
    let trimmed = text
        .iter()
        .copied()
        .take_while(|&b| b != 0)
        .collect::<Vec<u8>>();
    let text = String::from_utf8(trimmed).map_err(|_| LoginBodyError::NotUtf8)?;
    base64::engine::general_purpose::STANDARD
        .decode(text.trim())
        .map_err(|_| LoginBodyError::NotBase64)
}

/// 登录包解包的失败原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginBodyError {
    /// XXTEA 末尾长度字越界（多半是拿错密钥或包被截断）。
    LengthWord,
    /// 解密出来的不是 UTF-8 文本。
    NotUtf8,
    /// 文本不是合法 base64。
    NotBase64,
}

impl std::fmt::Display for LoginBodyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::LengthWord => "XXTEA 长度字越界",
            Self::NotUtf8 => "解密结果不是 UTF-8",
            Self::NotBase64 => "解密结果不是合法 base64",
        };
        f.write_str(s)
    }
}

impl std::error::Error for LoginBodyError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_is_the_vendor_string() {
        assert_eq!(
            std::str::from_utf8(&SIGN_KEY).expect("ascii"),
            "05:&()^&*abddzMQ"
        );
    }

    #[test]
    fn bytes_roundtrip_keeps_length() {
        // 长度不是 4 的倍数：长度字必须把填充去掉
        for len in [1usize, 3, 5, 8, 17, 40, 100] {
            let plain: Vec<u8> = (0..len)
                .map(|i| (i as u8).wrapping_mul(37).wrapping_add(1))
                .collect();
            let enc = encrypt(&plain, &SIGN_KEY);
            let dec = decrypt(&enc, &SIGN_KEY).expect("解密");
            assert_eq!(dec, plain, "len={len}");
        }
    }

    #[test]
    fn empty_input_is_identity() {
        assert!(encrypt(&[], &SIGN_KEY).is_empty());
        assert_eq!(decrypt(&[], &SIGN_KEY), Some(Vec::new()));
    }

    #[test]
    fn short_input_passes_through() {
        // 少于 2 个 u32：客户端 `r()` 只产 1 个词，解密不处理
        assert_eq!(decrypt(&[1, 2, 3], &SIGN_KEY), Some(vec![1, 2, 3]));
    }

    /// 由 `work/test/xxtea_vectors.json` 生成的向量，与客户端 JS 实现逐字节比对。
    #[test]
    fn matches_client_js_vectors() {
        // 向量随仓库走，不依赖 work/ 目录是否同步到远端。
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/xxtea_vectors.json");
        let text = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("读不到 {path}: {e}（向量必须随仓库提交）"));
        let v: serde_json::Value = serde_json::from_str(&text).expect("json");
        for case in v["cases"].as_array().expect("cases") {
            let plain = hex_decode(case["plain"].as_str().expect("plain"));
            let cipher = hex_decode(case["cipher"].as_str().expect("cipher"));
            assert_eq!(encrypt(&plain, &SIGN_KEY), cipher, "加密向量不符");
            assert_eq!(decrypt(&cipher, &SIGN_KEY), Some(plain), "解密向量不符");
        }
    }

    fn hex_decode(s: &str) -> Vec<u8> {
        (0..s.len() / 2)
            .map(|i| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).expect("hex"))
            .collect()
    }
}
