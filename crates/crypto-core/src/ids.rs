//! UUID 标识类型。
//!
//! 节点 ID、内容 ID、版本 ID 全部由客户端生成（UUIDv4，design D5），
//! 同时充当创建类请求的幂等键。加密时它们会进入 AAD / HKDF salt，
//! 因此**字节表示必须是所有实现一致的规范形式**：
//!
//! > 小写、带连字符的 36 字节 ASCII（`Uuid::hyphenated()`），
//! > 例如 `550e8400-e29b-41d4-a716-446655440000`。
//!
//! 解析时接受 `uuid` crate 支持的各种文本形式（大写、无连字符、花括号、URN），
//! 但存放与参与密码学计算的一律是上面的规范形式。

use std::fmt;

use uuid::Uuid;

use crate::error::CryptoError;

macro_rules! define_id {
    ($(#[$meta:meta])* $name:ident, $kind:literal) => {
        $(#[$meta])*
        #[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(String);

        impl $name {
            /// 解析 UUID 文本并转换为规范小写形式。
            pub fn parse(input: &str) -> Result<Self, CryptoError> {
                let uuid = Uuid::parse_str(input.trim()).map_err(|_| CryptoError::InvalidId {
                    kind: $kind,
                })?;
                Ok(Self(uuid.hyphenated().to_string()))
            }

            /// 规范形式（36 字节 ASCII）。
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }

            /// 参与 AAD / HKDF salt 的字节。
            #[must_use]
            pub fn as_bytes(&self) -> &[u8] {
                self.0.as_bytes()
            }
        }

        impl TryFrom<&str> for $name {
            type Error = CryptoError;

            fn try_from(value: &str) -> Result<Self, Self::Error> {
                Self::parse(value)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}({})", stringify!($name), self.0)
            }
        }
    };
}

define_id! {
    /// 文件/文件夹节点 ID。
    NodeId, "node"
}

define_id! {
    /// 一次上传产生的内容 ID（文件块的逻辑分组）。
    ContentId, "content"
}

define_id! {
    /// 文件版本 ID。
    VersionId, "version"
}

/// 规范 ID 字符串的长度（36 字节）。
pub const ID_LEN: usize = 36;

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "550E8400-E29B-41D4-A716-446655440000";

    #[test]
    fn parses_and_canonicalizes() {
        let id = NodeId::parse(SAMPLE).unwrap();
        assert_eq!(id.as_str(), "550e8400-e29b-41d4-a716-446655440000");
        assert_eq!(id.as_bytes().len(), ID_LEN);
    }

    #[test]
    fn accepts_non_hyphenated_input() {
        let hyphenated = ContentId::parse(SAMPLE).unwrap();
        let plain = ContentId::parse("550e8400e29b41d4a716446655440000").unwrap();
        assert_eq!(hyphenated, plain);
    }

    #[test]
    fn trims_surrounding_whitespace() {
        let id = VersionId::parse(" 550e8400-e29b-41d4-a716-446655440000 \n").unwrap();
        assert_eq!(id.as_str(), "550e8400-e29b-41d4-a716-446655440000");
    }

    #[test]
    fn rejects_invalid_ids() {
        assert_eq!(
            NodeId::parse("not-a-uuid"),
            Err(CryptoError::InvalidId { kind: "node" })
        );
        assert_eq!(
            ContentId::parse(""),
            Err(CryptoError::InvalidId { kind: "content" })
        );
    }
}
