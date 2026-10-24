//! 登录握手：`loginvar` 挑战-响应。
//!
//! ## 原来是什么
//! `WorkTaskPlayerSignIn.run()` 里：
//! ```text
//! v27 = UtilsTools.random(1, 10000)
//! builder.setLoginvar(v27)                                    // 下发给客户端
//! v29 = MD5Helper.md5Charge(String.valueOf(
//!           new RandomSeed(v27).random(1, 10000) + 9527)
//!       ).toLowerCase()
//! channelPlayer.setLoginvar(v29)                              // 服务端留底
//! log.debug("服务端计算出来的密码：" + v29)
//! ```
//! 客户端用同一个 `RandomSeed` 算出同样的串，再通过
//! `PlayerGetItemReq_16.getItemId()`（`getItemId` 字段被复用成密码位）
//! 回传，服务端存进 `ChannelPlayer.setClientLoginvar`。
//!
//! ## 为什么要按位复刻
//! 这个数是后续活动反作弊（`AbsPurposePlayPlayer.getCheatReturnData` /
//! `addIntegral`）判定的依据。算法错一位，客户端算出来的串就和服务端留底不同，
//! 表现出来是「功能莫名失效」，很难查。
//!
//! ## 算法（Java 字节码 + 客户端 JS 双向确认）
//! `RandomSeed.seed()`：
//! ```text
//! n = 17.0 * state + 139.0
//! i = (int) (n / 256.0)
//! state = n - 256.0 * i        // 即 state = (17 * state + 139) mod 256
//! return state / 256.0
//! ```
//! 客户端 `nat_module.js` 里的等价实现：
//! `n = 17 * this.randomSeed[0] + 139; i = Math.floor(n / 256); this.randomSeed[0] = n - 256 * i; return this.randomSeed[0] / 256;`

use md5::{Digest, Md5};

/// 客户端与服务端共用的偏移量。
pub const LOGINVAR_OFFSET: i32 = 9527;
/// `loginvar` 随机区间（闭区间）。
pub const LOGINVAR_RANGE: (i32, i32) = (1, 10_000);
/// 线性同余的模。
const MODULUS: f64 = 256.0;
/// 线性同余的乘数。
const MULTIPLIER: f64 = 17.0;
/// 线性同余的增量。
const INCREMENT: f64 = 139.0;

/// `bf.game.server.utils.RandomSeed` 的逐位复刻。
///
/// 只实现登录用得到的部分（`new` + `random`）。
#[derive(Debug, Clone)]
pub struct RandomSeed {
    state: f64,
}

impl RandomSeed {
    /// `new RandomSeed(seed)`。
    #[must_use]
    pub fn new(seed: i32) -> Self {
        // 原版 `randomSeed[0] = (double) seedI`
        Self {
            state: f64::from(seed),
        }
    }

    /// `seed()`：推进一次并返回 `[0, 1)`。
    fn next_double(&mut self) -> f64 {
        let n = MULTIPLIER * self.state + INCREMENT;
        // 原版 `(int) (n / 256.0)` 是**截断**，负数向零取整；Rust 的 `as i32` 同语义。
        #[allow(clippy::cast_possible_truncation)]
        let i = (n / MODULUS) as i32;
        self.state = n - MODULUS * f64::from(i);
        self.state / MODULUS
    }

    /// `random(min, max)`：闭区间，`min > max` 时自动交换。
    pub fn random(&mut self, min: i32, max: i32) -> i32 {
        let (a, b) = if min > max { (max, min) } else { (min, max) };
        let r = self.next_double();
        // 原版：`min + (int) (seed() * (max - min + 1))`
        #[allow(clippy::cast_possible_truncation)]
        let span = f64::from(b - a + 1);
        a + (r * span) as i32
    }
}

/// 由 `loginvar` 推出服务端留底的密码。
///
/// 对应 `MD5Helper.md5Charge(String.valueOf(new RandomSeed(v).random(1, 10000) + 9527)).toLowerCase()`。
#[must_use]
pub fn derive_password(loginvar: i32) -> String {
    let mut r = RandomSeed::new(loginvar);
    let v = r.random(LOGINVAR_RANGE.0, LOGINVAR_RANGE.1);
    let s = (v + LOGINVAR_OFFSET).to_string();
    let digest = Md5::digest(s.as_bytes());
    hex_lower(&digest)
}

fn hex_lower(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(out, "{b:02x}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 向量由**独立的 Python 实现**算出（不是从 Rust 反推），
    /// 用来证明 LCG 的每一步都没有走偏。
    #[test]
    fn lcg_state_matches_independent_vectors() {
        // (seed, 第一次推进后的 state, 浮点值, random(1,10000))
        let cases = [
            (1_i32, 156, 0.609_375_f64, 6094_i32),
            (42, 85, 0.332_031_25, 3321),
            (1234, 125, 0.488_281_25, 4883),
            (9527, 50, 0.195_312_5, 1954),
            (9999, 138, 0.539_062_5, 5391),
        ];
        for (seed, want_state, want_r, want_v) in cases {
            let mut r = RandomSeed::new(seed);
            let got_r = r.next_double();
            assert!((got_r - want_r).abs() < 1e-12, "seed={seed} r={got_r}");
            #[allow(clippy::cast_possible_truncation)]
            let st = (r.state) as i32;
            assert_eq!(st, want_state, "seed={seed} state");

            let mut r2 = RandomSeed::new(seed);
            assert_eq!(r2.random(1, 10_000), want_v, "seed={seed} random");
        }
    }

    #[test]
    fn password_matches_independent_vectors() {
        let cases = [
            (1, "2a232fce8107a69a090fba557c8292bc"),
            (42, "b8c4464e7ee7dc326d7807a8549e449f"),
            (1234, "a1d41c14c1d0aa9b9cc1e228d962fe42"),
            (9527, "86654aa91382ffa290d510f1d2dd3353"),
            (9999, "f5dd07d944fdcaba5af8b6828ed55f69"),
        ];
        for (seed, want) in cases {
            assert_eq!(derive_password(seed), want, "seed={seed}");
        }
    }

    #[test]
    fn password_is_lowercase_hex32() {
        let p = derive_password(7);
        assert_eq!(p.len(), 32);
        assert!(p
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }

    #[test]
    fn random_swaps_bounds() {
        let a = RandomSeed::new(100).random(10_000, 1);
        let b = RandomSeed::new(100).random(1, 10_000);
        assert_eq!(a, b);
    }

    #[test]
    fn random_stays_in_range() {
        for seed in 1..200 {
            let mut r = RandomSeed::new(seed);
            for _ in 0..50 {
                let v = r.random(1, 10_000);
                assert!((1..=10_000).contains(&v), "seed={seed} v={v}");
            }
        }
    }
}
