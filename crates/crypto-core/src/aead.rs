//! 密文帧与 AEAD 原语（design D5）。
//!
//! 帧布局（所有密文字节都以此格式落库/入对象存储）：
//!
//! ```text
//! version(1B, 固定 0x01) ‖ nonce(24B) ‖ ciphertext(可变) ‖ tag(16B)
//! ```
//!
//! - 算法：XChaCha20-Poly1305；nonce 由 CSPRNG 生成，绝不复用同一 (key, nonce)。
//! - AAD 由调用方按域分隔规则构造（见 [`crate::aad`]）。
//! - 帧开销固定为 [`FRAME_OVERHEAD`] = 41 字节（空明文也是 41 字节密文）。
//! - 解密失败时**不返回任何部分明文**（AEAD 一次性校验通过才输出）。

use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit, Payload},
};

use crate::error::CryptoError;

/// XChaCha20-Poly1305 的 nonce 长度。
pub const NONCE_LEN: usize = 24;

/// Poly1305 认证标签长度。
pub const TAG_LEN: usize = 16;

/// 帧的固定开销：版本(1) + nonce(24) + tag(16)。
pub const FRAME_OVERHEAD: usize = 1 + NONCE_LEN + TAG_LEN;

/// 用调用方提供的 nonce 加密，返回完整帧。
///
/// # 安全
///
/// **仅用于**生成/校验固定测试向量与跨实现兼容性测试。
/// 生产代码必须使用 [`seal_random`]，由 CSPRNG 生成 nonce；
/// 手工固定 nonce 会导致 nonce 复用，直接摧毁 XChaCha20-Poly1305 的安全性。
pub fn seal_with_nonce(
    key: &[u8; crate::keys::KEY_LEN],
    nonce: &[u8; NONCE_LEN],
    aad: &[u8],
    plaintext: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    let cipher =
        XChaCha20Poly1305::new_from_slice(key).map_err(|_| CryptoError::InvalidKeyLength)?;
    let ciphertext = cipher
        .encrypt(
            XNonce::from_slice(nonce),
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| CryptoError::AuthenticationFailed)?;

    if ciphertext.len() < TAG_LEN {
        return Err(CryptoError::AuthenticationFailed);
    }

    let mut frame = Vec::with_capacity(FRAME_OVERHEAD + plaintext.len());
    frame.push(crate::CIPHERTEXT_FORMAT_VERSION);
    frame.extend_from_slice(nonce);
    frame.extend_from_slice(&ciphertext);
    Ok(frame)
}

/// 用 CSPRNG 生成的 nonce 加密，返回完整帧。
pub(crate) fn seal_random(
    key: &[u8; crate::keys::KEY_LEN],
    aad: &[u8],
    plaintext: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    let mut nonce = [0u8; NONCE_LEN];
    getrandom::getrandom(&mut nonce).map_err(|_| CryptoError::Randomness)?;
    seal_with_nonce(key, &nonce, aad, plaintext)
}

/// 校验并解密一帧密文。
///
/// 任何一字节（版本、nonce、密文、tag）或 AAD 被改动都会返回错误，
/// 且不会输出部分明文。
pub fn open(
    key: &[u8; crate::keys::KEY_LEN],
    aad: &[u8],
    frame: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    let (version, rest) = frame.split_first().ok_or(CryptoError::TruncatedFrame {
        min: FRAME_OVERHEAD,
    })?;

    if *version != crate::CIPHERTEXT_FORMAT_VERSION {
        return Err(CryptoError::UnsupportedFormatVersion(*version));
    }

    if rest.len() < NONCE_LEN + TAG_LEN {
        return Err(CryptoError::TruncatedFrame {
            min: FRAME_OVERHEAD,
        });
    }

    let (nonce, ciphertext) = rest.split_at(NONCE_LEN);
    let cipher =
        XChaCha20Poly1305::new_from_slice(key).map_err(|_| CryptoError::InvalidKeyLength)?;

    cipher
        .decrypt(
            XNonce::from_slice(nonce),
            Payload {
                msg: ciphertext,
                aad,
            },
        )
        .map_err(|_| CryptoError::AuthenticationFailed)
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: [u8; 32] = [0x11; 32];
    const NONCE: [u8; NONCE_LEN] = [0x22; NONCE_LEN];

    #[test]
    fn roundtrip() {
        let plaintext = b"hello, e2ee backup";
        let frame = seal_with_nonce(&KEY, &NONCE, b"aad", plaintext).unwrap();
        assert_eq!(frame.len(), FRAME_OVERHEAD + plaintext.len());
        assert_eq!(frame[0], crate::CIPHERTEXT_FORMAT_VERSION);
        assert_eq!(open(&KEY, b"aad", &frame).unwrap(), plaintext);
    }

    #[test]
    fn empty_plaintext_roundtrip() {
        let frame = seal_with_nonce(&KEY, &NONCE, b"aad", b"").unwrap();
        assert_eq!(frame.len(), FRAME_OVERHEAD);
        assert_eq!(open(&KEY, b"aad", &frame).unwrap(), b"");
    }

    #[test]
    fn tampering_any_byte_fails() {
        let plaintext = b"tamper me";
        let frame = seal_with_nonce(&KEY, &NONCE, b"aad", plaintext).unwrap();

        for index in 0..frame.len() {
            let mut tampered = frame.clone();
            tampered[index] ^= 0x01;
            assert!(
                open(&KEY, b"aad", &tampered).is_err(),
                "第 {index} 字节被篡改后仍然解密成功"
            );
        }
    }

    #[test]
    fn wrong_key_fails() {
        let frame = seal_with_nonce(&KEY, &NONCE, b"aad", b"data").unwrap();
        assert_eq!(
            open(&[0x33; 32], b"aad", &frame),
            Err(CryptoError::AuthenticationFailed)
        );
    }

    #[test]
    fn wrong_aad_fails() {
        let frame = seal_with_nonce(&KEY, &NONCE, b"aad", b"data").unwrap();
        assert_eq!(
            open(&KEY, b"other-aad", &frame),
            Err(CryptoError::AuthenticationFailed)
        );
    }

    #[test]
    fn unknown_version_is_reported() {
        let mut frame = seal_with_nonce(&KEY, &NONCE, b"aad", b"data").unwrap();
        frame[0] = 0x02;
        assert_eq!(
            open(&KEY, b"aad", &frame),
            Err(CryptoError::UnsupportedFormatVersion(0x02))
        );
    }

    #[test]
    fn truncated_frames_are_rejected() {
        assert_eq!(
            open(&KEY, b"aad", &[]),
            Err(CryptoError::TruncatedFrame {
                min: FRAME_OVERHEAD
            })
        );
        assert_eq!(
            open(&KEY, b"aad", &[crate::CIPHERTEXT_FORMAT_VERSION]),
            Err(CryptoError::TruncatedFrame {
                min: FRAME_OVERHEAD
            })
        );
        let mut frame = seal_with_nonce(&KEY, &NONCE, b"aad", b"data").unwrap();
        frame.truncate(FRAME_OVERHEAD - 1);
        assert!(open(&KEY, b"aad", &frame).is_err());
    }
}
