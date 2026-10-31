//! 认证协议客户端（design D4/D12a）。
//!
//! 登录步骤与 Web 完全一致：
//! `prelogin → 校验 KDF 参数 → Argon2id 派生 auth_key/pwd_wrap → 登录 →
//! 用 pwd_wrap 解包 wrapped_mk → /auth/me`。
//!
//! 约束：
//! - `auth_key` 只在本模块与请求体构造期间存在，不进入日志；
//! - 服务端下发的 KDF 参数必须先经 crypto-core 校验（算法 + 上下限）；
//! - 公开 API 只返回 [`crate::Account`]（展示数据），不返回密钥。

use crypto_core::{
    CIPHERTEXT_FORMAT_VERSION, FRAME_OVERHEAD, KDF_SALT_LEN, KEY_LEN, KdfParams, MasterKey,
    PasswordKeys,
};
use serde::Deserialize;
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::client::{Account, AccountStatus};
use crate::error::ClientError;
use crate::hex;

/// `POST /api/v1/auth/prelogin` 响应。
#[derive(Debug, Deserialize)]
pub(crate) struct PreloginResponse {
    pub kdf_params: serde_json::Value,
    pub kdf_salt: String,
}

/// 登录与 `/auth/me` 共用的用户资料响应。
#[derive(Debug, Deserialize)]
pub(crate) struct ProfileResponse {
    pub user_id: Uuid,
    pub email: String,
    #[serde(default)]
    pub email_verified: bool,
    pub kdf_params: serde_json::Value,
    pub kdf_salt: String,
    pub wrapped_mk: String,
    pub root_node_id: Uuid,
    /// 当前服务端总会返回该字段；缺失时按未知状态（不可写）处理，界面不提供写入口。
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub status_reason: Option<String>,
    #[serde(default)]
    pub is_admin: bool,
}

/// 登录响应（结构同 `/auth/me`）。
pub(crate) type LoginResponse = ProfileResponse;

/// 仅用于登录的请求体；`auth_key` 以十六进制传输。
#[derive(serde::Serialize)]
pub(crate) struct LoginRequest<'a> {
    pub email: &'a str,
    pub auth_key: &'a str,
}

/// 校验并解析服务端下发的 KDF 参数。
///
/// - 未知/缺失算法 → [`ClientError::UnsupportedKdfAlgorithm`]（提示升级客户端）；
/// - 越界参数 → [`ClientError::InvalidKdfParams`]（安全错误，不派生、不发请求）。
pub(crate) fn parse_kdf_params(value: &serde_json::Value) -> Result<KdfParams, ClientError> {
    let json = serde_json::to_string(value)
        .map_err(|_| ClientError::InvalidResponse("KDF 参数无法序列化"))?;
    crypto_core::parse_kdf_params(&json).map_err(|error| match error {
        crypto_core::CryptoError::UnsupportedKdfAlgorithm => ClientError::UnsupportedKdfAlgorithm,
        crypto_core::CryptoError::InvalidKdfParams => ClientError::InvalidKdfParams,
        other => ClientError::Crypto(other),
    })
}

/// 解析 16 字节 KDF 盐。
pub(crate) fn decode_kdf_salt(value: &str) -> Result<[u8; KDF_SALT_LEN], ClientError> {
    hex::decode_hex_array::<KDF_SALT_LEN>(value)
        .ok_or(ClientError::InvalidResponse("KDF 盐不是 16 字节十六进制"))
}

/// 解析包裹后的主密钥密文帧。
pub(crate) fn decode_wrapped_mk(value: &str) -> Result<Vec<u8>, ClientError> {
    let bytes =
        hex::decode_hex(value).ok_or(ClientError::InvalidResponse("wrapped_mk 不是十六进制"))?;
    if bytes.len() != FRAME_OVERHEAD + KEY_LEN {
        return Err(ClientError::InvalidResponse("wrapped_mk 长度无效"));
    }
    if bytes.first() != Some(&CIPHERTEXT_FORMAT_VERSION) {
        return Err(ClientError::InvalidResponse("wrapped_mk 格式版本未知"));
    }
    Ok(bytes)
}

/// 在阻塞线程中执行 Argon2id，避免拖住异步运行时。
pub(crate) async fn derive_password_keys(
    password: &str,
    salt: [u8; KDF_SALT_LEN],
    params: KdfParams,
) -> Result<PasswordKeys, ClientError> {
    // 密码副本在闭包结束时清零；Argon2 内部还会做 NFKC 规范化。
    let password = Zeroizing::new(password.to_owned());
    tokio::task::spawn_blocking(move || {
        crypto_core::derive_keys_from_password(&password, &salt, &params)
    })
    .await
    .map_err(|_| ClientError::Internal("KDF 任务失败"))?
    .map_err(|error| match error {
        crypto_core::CryptoError::InvalidKdfParams => ClientError::InvalidKdfParams,
        other => ClientError::Crypto(other),
    })
}

/// 用密码包裹密钥解包主密钥；AEAD 失败即“密码错误”。
pub(crate) fn unwrap_master_key(
    keys: &PasswordKeys,
    wrapped_mk: &[u8],
) -> Result<MasterKey, ClientError> {
    crypto_core::unwrap_mk_with_password(&keys.pwd_wrap, wrapped_mk).map_err(|error| match error {
        crypto_core::CryptoError::AuthenticationFailed => ClientError::UnlockFailed,
        other => ClientError::Crypto(other),
    })
}

/// 把服务端资料转换为只含展示数据的 [`Account`]。
pub(crate) fn account_from_profile(profile: &ProfileResponse) -> Account {
    Account {
        user_id: profile.user_id,
        email: profile.email.clone(),
        email_verified: profile.email_verified,
        root_node_id: profile.root_node_id,
        status: AccountStatus::from_wire(&profile.status),
        status_reason: profile.status_reason.clone(),
        is_admin: profile.is_admin,
    }
}

/// 计算认证密钥的十六进制表示；返回的字符串在离开作用域时清零。
pub(crate) fn auth_key_hex(keys: &PasswordKeys) -> Zeroizing<String> {
    Zeroizing::new(hex::encode_hex(keys.auth_key.expose_secret()))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;

    fn params_json(m: u32, t: u32, p: u32) -> serde_json::Value {
        serde_json::json!({
            "algorithm": "argon2id",
            "m_cost_kib": m,
            "t_cost": t,
            "p_cost": p,
        })
    }

    #[test]
    fn rejects_downgraded_params_before_derivation() {
        // client-encryption spec「被降级的参数」：8 MiB / 1 次迭代必须拒绝。
        let error = parse_kdf_params(&params_json(8 * 1024, 1, 1)).unwrap_err();
        assert!(matches!(error, ClientError::InvalidKdfParams));
    }

    #[test]
    fn rejects_oversized_params_before_allocation() {
        // client-encryption spec「过大的参数」：1 GiB 必须拒绝。
        let error = parse_kdf_params(&params_json(1024 * 1024, 3, 1)).unwrap_err();
        assert!(matches!(error, ClientError::InvalidKdfParams));
    }

    #[test]
    fn rejects_unknown_or_missing_algorithm() {
        let unknown = serde_json::json!({
            "algorithm": "scrypt",
            "m_cost_kib": 65536,
            "t_cost": 3,
            "p_cost": 1,
        });
        assert!(matches!(
            parse_kdf_params(&unknown),
            Err(ClientError::UnsupportedKdfAlgorithm)
        ));

        let missing = serde_json::json!({
            "m_cost_kib": 65536,
            "t_cost": 3,
            "p_cost": 1,
        });
        assert!(matches!(
            parse_kdf_params(&missing),
            Err(ClientError::UnsupportedKdfAlgorithm)
        ));
    }

    #[test]
    fn accepts_recommended_params() {
        let params = parse_kdf_params(&params_json(65536, 3, 1)).unwrap();
        assert_eq!(params, KdfParams::RECOMMENDED);
    }

    #[test]
    fn salt_must_be_sixteen_bytes() {
        assert!(decode_kdf_salt(&"00".repeat(16)).is_ok());
        assert!(matches!(
            decode_kdf_salt(&"00".repeat(15)),
            Err(ClientError::InvalidResponse(_))
        ));
        assert!(matches!(
            decode_kdf_salt("zz"),
            Err(ClientError::InvalidResponse(_))
        ));
    }

    #[test]
    fn wrapped_mk_requires_exact_frame_length_and_version() {
        let mut frame = vec![CIPHERTEXT_FORMAT_VERSION];
        frame.extend(std::iter::repeat_n(0x11_u8, FRAME_OVERHEAD + KEY_LEN - 1));
        assert_eq!(decode_wrapped_mk(&hex::encode_hex(&frame)).unwrap(), frame);

        assert!(matches!(
            decode_wrapped_mk(&hex::encode_hex(&frame[..10])),
            Err(ClientError::InvalidResponse(_))
        ));

        let mut wrong_version = frame;
        wrong_version[0] = 0xff;
        assert!(matches!(
            decode_wrapped_mk(&hex::encode_hex(&wrong_version)),
            Err(ClientError::InvalidResponse(_))
        ));
    }

    #[test]
    fn account_status_maps_wire_values() {
        assert_eq!(AccountStatus::from_wire("active"), AccountStatus::Active);
        assert_eq!(AccountStatus::from_wire("pending"), AccountStatus::Pending);
        assert_eq!(
            AccountStatus::from_wire("suspended"),
            AccountStatus::Suspended
        );
        assert_eq!(
            AccountStatus::from_wire("rejected"),
            AccountStatus::Rejected
        );
        assert_eq!(AccountStatus::from_wire(""), AccountStatus::Unknown);
        assert_eq!(
            AccountStatus::from_wire("future-state"),
            AccountStatus::Unknown
        );
    }
}
