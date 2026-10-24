//! 密钥类型。
//!
//! 所有密钥都是 32 字节的私有 newtype：
//! - 只实现 `Clone`/`Zeroize`/`ZeroizeOnDrop`，**不实现** `Debug` 的原始值输出、
//!   `Display`、`Serialize`，避免被意外写进日志或序列化进请求体；
//! - `Debug` 一律输出 `<redacted>`；
//! - 内存中的字节只在 crate 内部通过 `as_bytes()` 读取。

use std::fmt;

use subtle::ConstantTimeEq;
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::error::CryptoError;

/// 所有对称密钥的字节长度。
pub const KEY_LEN: usize = 32;

macro_rules! define_key {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Zeroize, ZeroizeOnDrop)]
        pub struct $name([u8; KEY_LEN]);

        impl $name {
            /// 密钥字节长度。
            pub const LEN: usize = KEY_LEN;

            /// 从固定长度字节构造。
            ///
            /// 调用方必须保证字节来自可信来源（解包结果、KDF 输出或 CSPRNG）。
            #[must_use]
            pub fn from_bytes(bytes: [u8; KEY_LEN]) -> Self {
                Self(bytes)
            }

            pub(crate) fn as_bytes(&self) -> &[u8; KEY_LEN] {
                &self.0
            }
        }

        impl From<[u8; KEY_LEN]> for $name {
            fn from(bytes: [u8; KEY_LEN]) -> Self {
                Self(bytes)
            }
        }

        impl TryFrom<&[u8]> for $name {
            type Error = CryptoError;

            fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
                let bytes: [u8; KEY_LEN] = value
                    .try_into()
                    .map_err(|_| CryptoError::InvalidKeyLength)?;
                Ok(Self(bytes))
            }
        }

        impl PartialEq for $name {
            /// 常量时间比较，避免通过比较耗时泄露密钥前缀。
            fn eq(&self, other: &Self) -> bool {
                bool::from(self.0.ct_eq(&other.0))
            }
        }

        impl Eq for $name {}

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(concat!(stringify!($name), "(<redacted>)"))
            }
        }
    };
}

macro_rules! impl_random_key {
    ($name:ident) => {
        impl $name {
            /// 从系统 CSPRNG 生成新密钥。
            pub fn generate() -> Result<Self, CryptoError> {
                let mut bytes = [0u8; KEY_LEN];
                getrandom::getrandom(&mut bytes).map_err(|_| CryptoError::Randomness)?;
                Ok(Self::from_bytes(bytes))
            }
        }
    };
}

define_key! {
    /// 主密钥（MK）：账号内所有节点密钥的包裹根，由密码包裹密钥与恢复密钥分别包裹一份。
    MasterKey
}
impl_random_key!(MasterKey);

define_key! {
    /// 恢复密钥（rk）：与密码相互独立的人类可抄写密钥，用于忘记密码时恢复 MK。
    RecoveryKey
}
impl_random_key!(RecoveryKey);

define_key! {
    /// 节点密钥：每个文件/文件夹独立一把，包裹后存服务端。
    NodeKey
}
impl_random_key!(NodeKey);

define_key! {
    /// Argon2id 的输出根密钥，仅作为 HKDF 的输入使用，不直接加密任何数据。
    RootKey
}

define_key! {
    /// 密码包裹密钥（pwd_wrap）：包裹 MK。
    PasswordWrapKey
}

define_key! {
    /// 认证密钥（auth_key）：只用于向服务端证明身份，服务端存 `HMAC(pepper, auth_key)`。
    AuthKey
}

impl AuthKey {
    /// 认证密钥字节。
    ///
    /// 认证密钥本来就要发送给服务端（服务端只保存其含 pepper 的 HMAC），
    /// 因此这是唯一允许离开 Worker 内存的密钥类型。调用方**不得**记录日志或持久化。
    pub fn expose_secret(&self) -> &[u8; KEY_LEN] {
        self.as_bytes()
    }
}

define_key! {
    /// 恢复包裹密钥（rk_wrap）：用恢复密钥派生，包裹 MK。
    RecoveryWrapKey
}

define_key! {
    /// 恢复认证值（rk_auth）：用恢复密钥派生，供服务端校验恢复请求。
    RecoveryAuthKey
}

impl RecoveryAuthKey {
    /// 恢复认证值字节，同样需要随恢复请求发送给服务端。
    pub fn expose_secret(&self) -> &[u8; KEY_LEN] {
        self.as_bytes()
    }
}

define_key! {
    /// 名称令牌密钥：由 MK 派生，用于计算 HMAC 名称令牌。
    NameKey
}

define_key! {
    /// 内容密钥：由节点密钥与 content_id 派生，用于加密该内容的全部块。
    ContentKey
}

#[cfg(test)]
mod tests {
    use zeroize::{Zeroize, ZeroizeOnDrop};

    use super::*;

    fn assert_zeroize_key<T: Zeroize + ZeroizeOnDrop + Clone + fmt::Debug>() {}

    #[test]
    fn every_key_type_is_zeroizing() {
        assert_zeroize_key::<MasterKey>();
        assert_zeroize_key::<RecoveryKey>();
        assert_zeroize_key::<NodeKey>();
        assert_zeroize_key::<RootKey>();
        assert_zeroize_key::<PasswordWrapKey>();
        assert_zeroize_key::<AuthKey>();
        assert_zeroize_key::<RecoveryWrapKey>();
        assert_zeroize_key::<RecoveryAuthKey>();
        assert_zeroize_key::<NameKey>();
        assert_zeroize_key::<ContentKey>();
    }

    #[test]
    fn debug_output_is_redacted() {
        let key = MasterKey::from_bytes([0xab; KEY_LEN]);
        let printed = format!("{key:?}");
        assert_eq!(printed, "MasterKey(<redacted>)");
        assert!(!printed.to_lowercase().contains("abab"));
    }

    #[test]
    fn wrong_length_is_rejected() {
        assert_eq!(
            MasterKey::try_from(&[0u8; 31][..]),
            Err(CryptoError::InvalidKeyLength)
        );
        assert!(MasterKey::try_from(&[0u8; 32][..]).is_ok());
    }

    #[test]
    fn generated_keys_differ() {
        let first = MasterKey::generate().unwrap();
        let second = MasterKey::generate().unwrap();
        assert_ne!(first.as_bytes(), second.as_bytes());
        assert_ne!(first.as_bytes(), &[0u8; KEY_LEN]);
    }

    #[test]
    fn zeroize_clears_bytes() {
        let mut key = NodeKey::from_bytes([0x5a; KEY_LEN]);
        key.zeroize();
        assert_eq!(key.as_bytes(), &[0u8; KEY_LEN]);
    }
}
