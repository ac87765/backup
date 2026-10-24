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
//! - 参数随账号存储，服务端在 `POST /auth/prelogin` 原样返回；客户端必须先做算法与范围校验，
//!   避免被篡改的响应把客户端降级到廉价参数或拖入 OOM/长时间计算。
//!
//! KDF 参数的持久化 JSON 契约（design D4）：
//!
//! ```json
//! {"algorithm":"argon2id","m_cost_kib":65536,"t_cost":3,"p_cost":1}
//! ```
//!
//! 弱参数只允许通过 `#[cfg(test)]` 的入口使用；生产 API 没有任何绕过校验的路径。

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

/// 允许的最小内存代价：64 MiB（KiB 单位）。
pub const MIN_M_COST_KIB: u32 = 64 * 1024;

/// 允许的最大内存代价：256 MiB（KiB 单位）。
pub const MAX_M_COST_KIB: u32 = 256 * 1024;

/// 允许的最小迭代次数。
pub const MIN_T_COST: u32 = 3;

/// 允许的最大迭代次数。
pub const MAX_T_COST: u32 = 8;

/// 唯一允许的并行度（Argon2id 单 lane）。
pub const REQUIRED_P_COST: u32 = 1;

/// KDF 算法标识。
///
/// 当前只有 Argon2id（隐含 Argon2 版本 `0x13`）。新增算法只能追加枚举值，
/// 未知值或缺失字段会在反序列化阶段被拒绝。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KdfAlgorithm {
    /// Argon2id v1.3。
    Argon2id,
}

/// KDF 参数。
///
/// 通过 JSON 与账号一起存储（`users.kdf_params`），字段名是持久化契约。
/// 客户端在使用前 MUST 调用 [`KdfParams::validate`]（或直接用
/// [`parse_kdf_params`]），派生函数内部也会再次校验。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KdfParams {
    /// 算法标识，当前只能是 `argon2id`。
    pub algorithm: KdfAlgorithm,
    /// 内存代价，单位 KiB。
    pub m_cost_kib: u32,
    /// 迭代次数。
    pub t_cost: u32,
    /// 并行度，必须为 1。
    pub p_cost: u32,
}

impl KdfParams {
    /// 默认参数：Argon2id、m=64 MiB、t=3、p=1（design D4）。
    pub const RECOMMENDED: Self = Self {
        algorithm: KdfAlgorithm::Argon2id,
        m_cost_kib: MIN_M_COST_KIB,
        t_cost: MIN_T_COST,
        p_cost: REQUIRED_P_COST,
    };

    /// 构造并校验参数（算法固定为 Argon2id）。
    pub fn new(m_cost_kib: u32, t_cost: u32, p_cost: u32) -> Result<Self, CryptoError> {
        let params = Self {
            algorithm: KdfAlgorithm::Argon2id,
            m_cost_kib,
            t_cost,
            p_cost,
        };
        params.validate()?;
        Ok(params)
    }

    /// 校验参数是否在允许范围内。
    ///
    /// - 下界防止被篡改的 prelogin 响应把客户端降级到廉价参数
    ///   （否则截获的 `auth_key` 可被低成本离线爆破）；
    /// - 上界防止客户端被拖入 OOM 或长时间计算。
    pub fn validate(&self) -> Result<(), CryptoError> {
        let memory_ok = (MIN_M_COST_KIB..=MAX_M_COST_KIB).contains(&self.m_cost_kib);
        let iterations_ok = (MIN_T_COST..=MAX_T_COST).contains(&self.t_cost);
        let parallelism_ok = self.p_cost == REQUIRED_P_COST;

        if memory_ok && iterations_ok && parallelism_ok {
            Ok(())
        } else {
            Err(CryptoError::InvalidKdfParams)
        }
    }

    /// 生成任意参数，**仅测试可见**，用于以弱参数加速单元测试。
    ///
    /// 生产代码无法调用它；生产派生入口 [`derive_root_key`] 会先调用 `validate()`。
    #[cfg(test)]
    pub(crate) fn for_tests(m_cost_kib: u32, t_cost: u32, p_cost: u32) -> Self {
        Self {
            algorithm: KdfAlgorithm::Argon2id,
            m_cost_kib,
            t_cost,
            p_cost,
        }
    }
}

impl Default for KdfParams {
    fn default() -> Self {
        Self::RECOMMENDED
    }
}

/// 解析并校验服务端返回的 KDF 参数 JSON。
///
/// - 算法缺失或不是 `argon2id` → [`CryptoError::UnsupportedKdfAlgorithm`]
///   （客户端应提示升级）；
/// - JSON 结构错误或参数越界 → [`CryptoError::InvalidKdfParams`]。
pub fn parse_kdf_params(json: &str) -> Result<KdfParams, CryptoError> {
    // 先单独探测算法字段，以便区分“未知算法”和“参数非法”。
    #[derive(Deserialize)]
    struct AlgorithmProbe {
        algorithm: Option<String>,
    }

    let probe: AlgorithmProbe =
        serde_json::from_str(json).map_err(|_| CryptoError::InvalidKdfParams)?;
    if probe.algorithm.as_deref() != Some("argon2id") {
        return Err(CryptoError::UnsupportedKdfAlgorithm);
    }

    let params: KdfParams =
        serde_json::from_str(json).map_err(|_| CryptoError::InvalidKdfParams)?;
    params.validate()?;
    Ok(params)
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

/// 用 Argon2id 从密码派生根密钥（生产入口，强制校验参数）。
///
/// 根密钥不直接用于加密：它只作为 HKDF 的输入，派生 `pwd_wrap` 与 `auth_key`。
pub fn derive_root_key(
    password: &str,
    salt: &[u8; KDF_SALT_LEN],
    params: &KdfParams,
) -> Result<RootKey, CryptoError> {
    params.validate()?;
    derive_root_key_inner(password, salt, params)
}

/// 不校验参数的实际派生逻辑。只有生产入口与 `#[cfg(test)]` 入口能调用。
fn derive_root_key_inner(
    password: &str,
    salt: &[u8; KDF_SALT_LEN],
    params: &KdfParams,
) -> Result<RootKey, CryptoError> {
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

/// 跳过参数校验的派生入口，**仅测试可见**（弱参数加速单元测试）。
#[cfg(test)]
pub(crate) fn derive_root_key_for_tests(
    password: &str,
    salt: &[u8; KDF_SALT_LEN],
    params: &KdfParams,
) -> Result<RootKey, CryptoError> {
    derive_root_key_inner(password, salt, params)
}

/// 跳过参数校验的“密码 → 两个密钥”，**仅测试可见**。
#[cfg(test)]
pub(crate) fn derive_keys_from_password_for_tests(
    password: &str,
    salt: &[u8; KDF_SALT_LEN],
    params: &KdfParams,
) -> Result<PasswordKeys, CryptoError> {
    let root = derive_root_key_inner(password, salt, params)?;
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
        KdfParams::for_tests(8 * 1024, 1, 1)
    }

    #[test]
    fn params_validate_bounds() {
        assert!(KdfParams::new(65536, 3, 1).is_ok());
        assert!(KdfParams::new(262144, 8, 1).is_ok());

        // 下界：低于 64 MiB 或 t < 3 一律拒绝。
        assert_eq!(
            KdfParams::new(65535, 3, 1),
            Err(CryptoError::InvalidKdfParams)
        );
        assert_eq!(
            KdfParams::new(65536, 2, 1),
            Err(CryptoError::InvalidKdfParams)
        );
        // 上界：超过 256 MiB 或 t > 8 一律拒绝。
        assert_eq!(
            KdfParams::new(262145, 3, 1),
            Err(CryptoError::InvalidKdfParams)
        );
        assert_eq!(
            KdfParams::new(65536, 9, 1),
            Err(CryptoError::InvalidKdfParams)
        );
        // 并行度必须为 1。
        assert_eq!(
            KdfParams::new(65536, 3, 0),
            Err(CryptoError::InvalidKdfParams)
        );
        assert_eq!(
            KdfParams::new(65536, 3, 2),
            Err(CryptoError::InvalidKdfParams)
        );
        assert_eq!(KdfParams::default(), KdfParams::RECOMMENDED);
    }

    #[test]
    fn params_json_roundtrip_is_stable() {
        let json = serde_json::to_string(&KdfParams::RECOMMENDED).unwrap();
        assert_eq!(
            json,
            r#"{"algorithm":"argon2id","m_cost_kib":65536,"t_cost":3,"p_cost":1}"#
        );
        let parsed: KdfParams = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, KdfParams::RECOMMENDED);
    }

    #[test]
    fn rejects_downgraded_params() {
        // client-encryption spec「被降级的参数」：8 MiB / 1 次迭代必须拒绝。
        let json = r#"{"algorithm":"argon2id","m_cost_kib":8192,"t_cost":1,"p_cost":1}"#;
        assert_eq!(parse_kdf_params(json), Err(CryptoError::InvalidKdfParams));

        // 派生入口本身也必须拒绝，且不应开始计算。
        let downgraded = KdfParams::for_tests(8 * 1024, 1, 1);
        assert_eq!(
            derive_root_key("password", &[0u8; KDF_SALT_LEN], &downgraded),
            Err(CryptoError::InvalidKdfParams)
        );
        assert_eq!(
            derive_keys_from_password("password", &[0u8; KDF_SALT_LEN], &downgraded).unwrap_err(),
            CryptoError::InvalidKdfParams
        );
    }

    #[test]
    fn rejects_oversized_params() {
        // client-encryption spec「过大的参数」：1 GiB 必须拒绝，且不分配该内存。
        let json = r#"{"algorithm":"argon2id","m_cost_kib":1048576,"t_cost":3,"p_cost":1}"#;
        assert_eq!(parse_kdf_params(json), Err(CryptoError::InvalidKdfParams));

        let oversized = KdfParams::for_tests(1024 * 1024, 3, 1);
        assert_eq!(
            derive_root_key("password", &[0u8; KDF_SALT_LEN], &oversized),
            Err(CryptoError::InvalidKdfParams)
        );
    }

    #[test]
    fn rejects_unknown_or_missing_algorithm() {
        // client-encryption spec「未知算法」：非 argon2id 或缺失一律拒绝。
        let unknown = r#"{"algorithm":"scrypt","m_cost_kib":65536,"t_cost":3,"p_cost":1}"#;
        assert_eq!(
            parse_kdf_params(unknown),
            Err(CryptoError::UnsupportedKdfAlgorithm)
        );

        let missing = r#"{"m_cost_kib":65536,"t_cost":3,"p_cost":1}"#;
        assert_eq!(
            parse_kdf_params(missing),
            Err(CryptoError::UnsupportedKdfAlgorithm)
        );

        // 直接反序列化（服务端返回体 → KdfParams）同样必须失败。
        assert!(serde_json::from_str::<KdfParams>(unknown).is_err());
        assert!(serde_json::from_str::<KdfParams>(missing).is_err());

        // 大小写不匹配同样视为未知算法（JSON 契约要求精确小写）。
        let wrong_case = r#"{"algorithm":"Argon2id","m_cost_kib":65536,"t_cost":3,"p_cost":1}"#;
        assert_eq!(
            parse_kdf_params(wrong_case),
            Err(CryptoError::UnsupportedKdfAlgorithm)
        );
    }

    #[test]
    fn parse_kdf_params_accepts_valid_json() {
        let params = parse_kdf_params(
            r#"{"algorithm":"argon2id","m_cost_kib":131072,"t_cost":4,"p_cost":1}"#,
        )
        .unwrap();
        assert_eq!(params.m_cost_kib, 131072);
        assert_eq!(params.t_cost, 4);
    }

    #[test]
    fn test_only_entry_accepts_weak_params() {
        // 生产入口拒绝弱参数，而仅测试入口可以用于加速测试。
        let weak = KdfParams::for_tests(8 * 1024, 1, 1);
        assert!(derive_root_key("password", &[0u8; KDF_SALT_LEN], &weak).is_err());
        assert!(derive_root_key_for_tests("password", &[0u8; KDF_SALT_LEN], &weak).is_ok());
    }

    #[test]
    fn derived_keys_are_independent() {
        let salt = [0x42u8; KDF_SALT_LEN];
        let keys = derive_keys_from_password_for_tests(
            "correct horse battery staple",
            &salt,
            &fast_params(),
        )
        .unwrap();

        assert_ne!(keys.pwd_wrap.as_bytes(), keys.auth_key.as_bytes());
        assert_ne!(keys.pwd_wrap.as_bytes(), &[0u8; KEY_LEN]);
    }

    #[test]
    fn derivation_is_deterministic() {
        let salt = [0x42u8; KDF_SALT_LEN];
        let first = derive_keys_from_password_for_tests(
            "correct horse battery staple",
            &salt,
            &fast_params(),
        )
        .unwrap();
        let second = derive_keys_from_password_for_tests(
            "correct horse battery staple",
            &salt,
            &fast_params(),
        )
        .unwrap();

        assert_eq!(first.pwd_wrap.as_bytes(), second.pwd_wrap.as_bytes());
        assert_eq!(first.auth_key.as_bytes(), second.auth_key.as_bytes());
    }

    #[test]
    fn password_is_nfkc_normalized() {
        let salt = [0x7fu8; KDF_SALT_LEN];
        // "é" 的合成形式与分解形式必须派生同一把密钥。
        let composed =
            derive_keys_from_password_for_tests("caf\u{e9}", &salt, &fast_params()).unwrap();
        let decomposed =
            derive_keys_from_password_for_tests("cafe\u{301}", &salt, &fast_params()).unwrap();

        assert_eq!(composed.pwd_wrap.as_bytes(), decomposed.pwd_wrap.as_bytes());
        assert_eq!(composed.auth_key.as_bytes(), decomposed.auth_key.as_bytes());
    }

    #[test]
    fn different_salt_or_password_changes_keys() {
        let params = fast_params();
        let base =
            derive_keys_from_password_for_tests("password-one", &[1u8; KDF_SALT_LEN], &params)
                .unwrap();
        let other_salt =
            derive_keys_from_password_for_tests("password-one", &[2u8; KDF_SALT_LEN], &params)
                .unwrap();
        let other_password =
            derive_keys_from_password_for_tests("password-two", &[1u8; KDF_SALT_LEN], &params)
                .unwrap();

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
}
