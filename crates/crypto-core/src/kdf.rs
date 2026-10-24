//! 密码派生（design D4）。
//!
//! ```text
//! root     = Argon2id(NFKC(password), salt16, m=64 MiB, t=3, p=1) -> 32B
//! pwd_wrap = HKDF-SHA256(root, info="bk/v1/pwd-wrap")
//! auth_key = HKDF-SHA256(root, info="bk/v1/auth")
//! ```
//!
//! - 密码在 Argon2id 之前做 Unicode NFKC 规范化，保证不同输入法/平台输入同一密码得到同一结果。
//! - `pwd_wrap` 与 `auth_key` 使用不同 info 派生，互相独立（拿到 auth_key 推不出 pwd_wrap）。
//! - 参数随账号存储，服务端在 `POST /auth/prelogin` 返回；客户端必须先做范围校验，
//!   避免被恶意/异常的服务端参数拖入长时间计算。

use argon2::{Algorithm, Argon2, Params, Version};
use hkdf::Hkdf;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use unicode_normalization::UnicodeNormalization;
use zeroize::Zeroizing;

use crate::domain::{HKDF_INFO_AUTH, HKDF_INFO_PWD_WRAP};
use crate::error::CryptoError;
use crate::keys::{AuthKey, KEY_LEN, PasswordWrapKey, RootKey};

/// KDF 盐长度（16 字节，CSPRNG 生成）。
pub const KDF_SALT_LEN: usize = 16;

/// Argon2id 输出长度，也是所有对称密钥的长度。
pub const KDF_OUTPUT_LEN: usize = KEY_LEN;

/// Argon2id 参数。
///
/// 通过 JSON 与账号一起存储（`users.kdf_params`），因此字段名是持久化契约。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KdfParams {
    /// 内存代价，单位 KiB。
    pub m_cost_kib: u32,
    /// 迭代次数。
    pub t_cost: u32,
    /// 并行度。
    pub p_cost: u32,
}

impl KdfParams {
    /// 默认参数：m=64 MiB、t=3、p=1（design D4）。
    pub const RECOMMENDED: Self = Self {
        m_cost_kib: 64 * 1024,
        t_cost: 3,
        p_cost: 1,
    };

    /// 构造并校验参数。
    pub fn new(m_cost_kib: u32, t_cost: u32, p_cost: u32) -> Result<Self, CryptoError> {
        let params = Self {
            m_cost_kib,
            t_cost,
            p_cost,
        };
        params.validate()?;
        Ok(params)
    }

    /// 校验参数是否在允许范围内。
    ///
    /// 参数来自服务端，必须校验上下界：
    /// - 下界避免弱参数；
    /// - 上界避免恶意服务端让客户端分配数 GiB 内存/长时间计算（DoS）。
    pub fn validate(&self) -> Result<(), CryptoError> {
        let memory_ok = (8 * 1024..=1024 * 1024).contains(&self.m_cost_kib);
        let iterations_ok = (1..=16).contains(&self.t_cost);
        let parallelism_ok = (1..=8).contains(&self.p_cost);
        // Argon2 要求 m_cost >= 8 * p_cost（单位均为 KiB / lane）。
        let lanes_ok = self.m_cost_kib >= 8 * self.p_cost;

        if memory_ok && iterations_ok && parallelism_ok && lanes_ok {
            Ok(())
        } else {
            Err(CryptoError::InvalidKdfParams)
        }
    }
}

impl Default for KdfParams {
    fn default() -> Self {
        Self::RECOMMENDED
    }
}

/// 从 CSPRNG 生成新的 KDF 盐。
pub fn generate_salt() -> Result<[u8; KDF_SALT_LEN], CryptoError> {
    let mut salt = [0u8; KDF_SALT_LEN];
    getrandom::getrandom(&mut salt).map_err(|_| CryptoError::Randomness)?;
    Ok(salt)
}

/// 密码派生的两个密钥。二者互相独立，用途互不重叠。
#[derive(Debug)]
pub struct PasswordKeys {
    /// 包裹 MK 的密钥。
    pub pwd_wrap: PasswordWrapKey,
    /// 提交给服务端做认证的密钥（服务端只存其含 pepper 的 HMAC）。
    pub auth_key: AuthKey,
}

/// 用 Argon2id 从密码派生根密钥。
///
/// 根密钥不直接用于加密：它只作为 HKDF 的输入，派生 `pwd_wrap` 与 `auth_key`。
pub fn derive_root_key(
    password: &str,
    salt: &[u8; KDF_SALT_LEN],
    params: &KdfParams,
) -> Result<RootKey, CryptoError> {
    params.validate()?;

    // NFKC 规范化；Zeroizing 保证这份密码副本在离开作用域时被清零。
    let normalized: Zeroizing<String> = Zeroizing::new(password.nfkc().collect());

    let argon2_params = Params::new(
        params.m_cost_kib,
        params.t_cost,
        params.p_cost,
        Some(KDF_OUTPUT_LEN),
    )
    .map_err(|_| CryptoError::InvalidKdfParams)?;

    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, argon2_params);

    let mut output = [0u8; KDF_OUTPUT_LEN];
    argon2
        .hash_password_into(normalized.as_bytes(), salt, &mut output)
        .map_err(|_| CryptoError::KdfFailure)?;

    Ok(RootKey::from_bytes(output))
}

/// 从根密钥派生 `pwd_wrap` 与 `auth_key`。
pub fn derive_password_keys(root: &RootKey) -> Result<PasswordKeys, CryptoError> {
    Ok(PasswordKeys {
        pwd_wrap: PasswordWrapKey::from_bytes(hkdf_expand(
            root.as_bytes(),
            None,
            HKDF_INFO_PWD_WRAP,
        )?),
        auth_key: AuthKey::from_bytes(hkdf_expand(root.as_bytes(), None, HKDF_INFO_AUTH)?),
    })
}

/// 一步完成“密码 → 两个密钥”，根密钥在函数内部被清零丢弃。
pub fn derive_keys_from_password(
    password: &str,
    salt: &[u8; KDF_SALT_LEN],
    params: &KdfParams,
) -> Result<PasswordKeys, CryptoError> {
    let root = derive_root_key(password, salt, params)?;
    derive_password_keys(&root)
}

/// HKDF-SHA256 扩展为 32 字节。
///
/// `salt` 为 `None` 时使用 RFC 5869 的“全零盐”。
pub(crate) fn hkdf_expand(
    ikm: &[u8],
    salt: Option<&[u8]>,
    info: &[u8],
) -> Result<[u8; KEY_LEN], CryptoError> {
    let hkdf = Hkdf::<Sha256>::new(salt, ikm);
    let mut output = [0u8; KEY_LEN];
    hkdf.expand(info, &mut output)
        .map_err(|_| CryptoError::KdfFailure)?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试用弱参数：只验证逻辑正确性，不代表生产参数。
    fn fast_params() -> KdfParams {
        KdfParams::new(8 * 1024, 1, 1).unwrap()
    }

    #[test]
    fn params_validate_bounds() {
        assert!(KdfParams::new(8 * 1024, 1, 1).is_ok());
        assert_eq!(
            KdfParams::new(8 * 1024 - 1, 1, 1),
            Err(CryptoError::InvalidKdfParams)
        );
        assert_eq!(
            KdfParams::new(1024 * 1024 + 1, 1, 1),
            Err(CryptoError::InvalidKdfParams)
        );
        assert_eq!(
            KdfParams::new(8 * 1024, 0, 1),
            Err(CryptoError::InvalidKdfParams)
        );
        assert_eq!(
            KdfParams::new(8 * 1024, 17, 1),
            Err(CryptoError::InvalidKdfParams)
        );
        assert_eq!(
            KdfParams::new(8 * 1024, 1, 9),
            Err(CryptoError::InvalidKdfParams)
        );
        assert_eq!(KdfParams::new(8, 1, 2), Err(CryptoError::InvalidKdfParams));
        assert_eq!(KdfParams::default(), KdfParams::RECOMMENDED);
    }

    #[test]
    fn params_json_roundtrip_is_stable() {
        let json = serde_json::to_string(&KdfParams::RECOMMENDED).unwrap();
        assert_eq!(json, r#"{"m_cost_kib":65536,"t_cost":3,"p_cost":1}"#);
        let parsed: KdfParams = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, KdfParams::RECOMMENDED);
    }

    #[test]
    fn derived_keys_are_independent() {
        let salt = [0x42u8; KDF_SALT_LEN];
        let keys = derive_keys_from_password("correct horse battery staple", &salt, &fast_params())
            .unwrap();

        assert_ne!(keys.pwd_wrap.as_bytes(), keys.auth_key.as_bytes());
        assert_ne!(keys.pwd_wrap.as_bytes(), &[0u8; KEY_LEN]);
    }

    #[test]
    fn derivation_is_deterministic() {
        let salt = [0x42u8; KDF_SALT_LEN];
        let first =
            derive_keys_from_password("correct horse battery staple", &salt, &fast_params())
                .unwrap();
        let second =
            derive_keys_from_password("correct horse battery staple", &salt, &fast_params())
                .unwrap();

        assert_eq!(first.pwd_wrap.as_bytes(), second.pwd_wrap.as_bytes());
        assert_eq!(first.auth_key.as_bytes(), second.auth_key.as_bytes());
    }

    #[test]
    fn password_is_nfkc_normalized() {
        let salt = [0x7fu8; KDF_SALT_LEN];
        // "é" 的合成形式与分解形式必须派生同一把密钥。
        let composed = derive_keys_from_password("caf\u{e9}", &salt, &fast_params()).unwrap();
        let decomposed = derive_keys_from_password("cafe\u{301}", &salt, &fast_params()).unwrap();

        assert_eq!(composed.pwd_wrap.as_bytes(), decomposed.pwd_wrap.as_bytes());
        assert_eq!(composed.auth_key.as_bytes(), decomposed.auth_key.as_bytes());
    }

    #[test]
    fn different_salt_or_password_changes_keys() {
        let params = fast_params();
        let base =
            derive_keys_from_password("password-one", &[1u8; KDF_SALT_LEN], &params).unwrap();
        let other_salt =
            derive_keys_from_password("password-one", &[2u8; KDF_SALT_LEN], &params).unwrap();
        let other_password =
            derive_keys_from_password("password-two", &[1u8; KDF_SALT_LEN], &params).unwrap();

        assert_ne!(base.pwd_wrap.as_bytes(), other_salt.pwd_wrap.as_bytes());
        assert_ne!(base.pwd_wrap.as_bytes(), other_password.pwd_wrap.as_bytes());
    }

    #[test]
    fn generated_salt_is_random() {
        let first = generate_salt().unwrap();
        let second = generate_salt().unwrap();
        assert_ne!(first, second);
        assert_ne!(first, [0u8; KDF_SALT_LEN]);
    }

    #[test]
    fn rejects_invalid_params_before_computing() {
        let invalid = KdfParams {
            m_cost_kib: 4096,
            t_cost: 1,
            p_cost: 1,
        };
        assert_eq!(
            derive_root_key("password", &[0u8; KDF_SALT_LEN], &invalid),
            Err(CryptoError::InvalidKdfParams)
        );
    }
}
