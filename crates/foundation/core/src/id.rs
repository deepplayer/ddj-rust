//! 强类型 ID
//!
//! Java 侧全部是裸 `long` / `int`，`playerId` 和 `templateId` 混用，
//! 参数顺序写反编译器不报错。这里用 newtype 隔离。

use std::fmt;

macro_rules! define_id {
    ($(#[$meta:meta])* $name:ident, $inner:ty, $doc:literal) => {
        $(#[$meta])*
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name($inner);

        impl $name {
            #[doc = concat!("构造 ", $doc)]
            #[must_use]
            pub const fn new(v: $inner) -> Self { Self(v) }
            /// 取原始值。
            #[must_use]
            pub const fn get(self) -> $inner { self.0 }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, concat!(stringify!($name), "({})"), self.0)
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                self.0.serialize(s)
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                <$inner>::deserialize(d).map(Self)
            }
        }
    };
}

define_id!(/// 角色 ID（`player.player_id`）。
    PlayerId, i64, "角色 ID");
define_id!(/// 公会 ID。
    GuildId, i64, "公会 ID");
define_id!(/// 区服 ID（`platform` 里的区号）。
    ServerId, i32, "区服 ID");
define_id!(/// 配置表模板 ID（CSV 里的 `id` 列）。
    TemplateId, i32, "模板 ID");
