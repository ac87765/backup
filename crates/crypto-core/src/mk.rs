//! MK 的包裹与解包（design D5）。
//!
//! - 密码路径：`AEAD(pwd_wrap, MK, aad="bk/v1/mk")`
//! - 恢复路径：`AEAD(rk_wrap,  MK, aad="bk/v1/mk-recovery")`
//!
//! 两条路径的 AAD 不同，因此同一把 MK 的两份包裹密文不能互换。

use zeroize::Zeroizing;

use crate::{
    aad, aead, error::CryptoError, keys::MasterKey, keys::PasswordWrapKey, keys::RecoveryWrapKey,
};

/// 用密码包裹密钥包裹 MK。
pub fn wrap_mk_with_password(
    key: &PasswordWrapKey,
    master_key: &MasterKey,
) -> Result<Vec<u8>, CryptoError> {
    aead::seal_random(key.as_bytes(), aad::master_key(), master_key.as_bytes())
}

/// 用密码包裹密钥解包 MK。
pub fn unwrap_mk_with_password(
    key: &PasswordWrapKey,
    wrapped: &[u8],
) -> Result<MasterKey, CryptoError> {
    let plaintext = Zeroizing::new(aead::open(key.as_bytes(), aad::master_key(), wrapped)?);
    MasterKey::try_from(plaintext.as_slice())
}

/// 用恢复包裹密钥包裹 MK。
pub fn wrap_mk_with_recovery(
    key: &RecoveryWrapKey,
    master_key: &MasterKey,
) -> Result<Vec<u8>, CryptoError> {
    aead::seal_random(
        key.as_bytes(),
        aad::master_key_recovery(),
        master_key.as_bytes(),
    )
}

/// 用恢复包裹密钥解包 MK。
pub fn unwrap_mk_with_recovery(
    key: &RecoveryWrapKey,
    wrapped: &[u8],
) -> Result<MasterKey, CryptoError> {
    let plaintext = Zeroizing::new(aead::open(
        key.as_bytes(),
        aad::master_key_recovery(),
        wrapped,
    )?);
    MasterKey::try_from(plaintext.as_slice())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kdf::{KdfParams, derive_keys_from_password};

    fn password_keys() -> crate::kdf::PasswordKeys {
        derive_keys_from_password(
            "correct horse battery staple",
            &[0x11u8; 16],
            &KdfParams::new(8 * 1024, 1, 1).unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn password_wrap_roundtrip() {
        let keys = password_keys();
        let master_key = MasterKey::generate().unwrap();

        let wrapped = wrap_mk_with_password(&keys.pwd_wrap, &master_key).unwrap();
        let unwrapped = unwrap_mk_with_password(&keys.pwd_wrap, &wrapped).unwrap();

        assert_eq!(unwrapped.as_bytes(), master_key.as_bytes());
        assert_eq!(
            wrapped.len(),
            crate::aead::FRAME_OVERHEAD + crate::keys::KEY_LEN
        );
    }

    #[test]
    fn wrong_password_key_cannot_unwrap() {
        let first = password_keys();
        let second = derive_keys_from_password(
            "another password",
            &[0x22u8; 16],
            &KdfParams::new(8 * 1024, 1, 1).unwrap(),
        )
        .unwrap();
        let master_key = MasterKey::generate().unwrap();

        let wrapped = wrap_mk_with_password(&first.pwd_wrap, &master_key).unwrap();
        assert_eq!(
            unwrap_mk_with_password(&second.pwd_wrap, &wrapped),
            Err(CryptoError::AuthenticationFailed)
        );
    }

    #[test]
    fn recovery_wrap_roundtrip() {
        let recovery = crate::keys::RecoveryKey::generate().unwrap();
        let keys = crate::recovery::derive_recovery_keys(&recovery).unwrap();
        let master_key = MasterKey::generate().unwrap();

        let wrapped = wrap_mk_with_recovery(&keys.rk_wrap, &master_key).unwrap();
        let unwrapped = unwrap_mk_with_recovery(&keys.rk_wrap, &wrapped).unwrap();

        assert_eq!(unwrapped.as_bytes(), master_key.as_bytes());
    }

    #[test]
    fn password_ciphertext_is_not_valid_recovery_ciphertext() {
        let keys = password_keys();
        let recovery = crate::keys::RecoveryKey::generate().unwrap();
        let recovery_keys = crate::recovery::derive_recovery_keys(&recovery).unwrap();
        let master_key = MasterKey::generate().unwrap();

        let wrapped = wrap_mk_with_password(&keys.pwd_wrap, &master_key).unwrap();

        // AAD 不同：恢复路径不能解密码路径的密文。
        assert_eq!(
            unwrap_mk_with_recovery(&recovery_keys.rk_wrap, &wrapped),
            Err(CryptoError::AuthenticationFailed)
        );
    }

    #[test]
    fn tampered_mk_ciphertext_fails() {
        let keys = password_keys();
        let master_key = MasterKey::generate().unwrap();
        let mut wrapped = wrap_mk_with_password(&keys.pwd_wrap, &master_key).unwrap();

        let last = wrapped.len() - 1;
        wrapped[last] ^= 0x80;

        assert_eq!(
            unwrap_mk_with_password(&keys.pwd_wrap, &wrapped),
            Err(CryptoError::AuthenticationFailed)
        );
    }
}
