//! 恢复密钥：生成、Crockford Base32 展示编码与校验位（design D5）。
//!
//! 展示格式：
//!
//! ```text
//! payload = rk(32B) ‖ SHA-256(rk)[0..2](2B) = 34B
//! display = Crockford-Base32(payload) 按每 5 个字符分组，用 '-' 连接
//! ```
//!
//! 34 字节 = 272 位，Base32 采用高位在前的位流编码，最后一组补 3 个 0 位，
//! 共 55 个字符（11 组）。解码时要求填充位为 0，保证表示唯一。
//!
//! 人类输入容错（Crockford 标准）：忽略 '-' 与空白；大小写不敏感；
//! `I`/`L` 读作 `1`，`O` 读作 `0`；`U` 不是合法字符（Crockford 字母表不含 U）。

use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

use crate::domain::{HKDF_INFO_RK_AUTH, HKDF_INFO_RK_WRAP};
use crate::error::CryptoError;
use crate::kdf::hkdf_expand;
use crate::keys::{KEY_LEN, RecoveryAuthKey, RecoveryKey, RecoveryWrapKey};

/// 恢复密钥本身的字节长度。
pub const RECOVERY_KEY_LEN: usize = KEY_LEN;

/// 校验位（SHA-256 前 2 字节）长度。
pub const RECOVERY_CHECKSUM_LEN: usize = 2;

/// 展示时每组的字符数。
pub const RECOVERY_DISPLAY_GROUP_SIZE: usize = 5;

/// 待编码负载长度：恢复密钥 + 校验位。
const RECOVERY_PAYLOAD_LEN: usize = RECOVERY_KEY_LEN + RECOVERY_CHECKSUM_LEN;

/// 编码后的字符总数（不含分组连字符）：向上取整(34 * 8 / 5) = 55。
pub const RECOVERY_SYMBOL_COUNT: usize = {
    let bits = RECOVERY_PAYLOAD_LEN * 8;
    if bits.is_multiple_of(5) {
        bits / 5
    } else {
        bits / 5 + 1
    }
};

/// 恢复密钥派生出的两把密钥。
#[derive(Debug)]
pub struct RecoveryKeys {
    /// 包裹 MK 的密钥。
    pub rk_wrap: RecoveryWrapKey,
    /// 提交给服务端校验的恢复认证值。
    pub rk_auth: RecoveryAuthKey,
}

/// 从恢复密钥派生 `rk_wrap` 与 `rk_auth`。
pub fn derive_recovery_keys(recovery_key: &RecoveryKey) -> Result<RecoveryKeys, CryptoError> {
    Ok(RecoveryKeys {
        rk_wrap: RecoveryWrapKey::from_bytes(hkdf_expand(
            recovery_key.as_bytes(),
            None,
            HKDF_INFO_RK_WRAP,
        )?),
        rk_auth: RecoveryAuthKey::from_bytes(hkdf_expand(
            recovery_key.as_bytes(),
            None,
            HKDF_INFO_RK_AUTH,
        )?),
    })
}

/// Crockford Base32 字母表（去掉 I、L、O、U）。
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// 把恢复密钥编码成可抄写、带校验位的字符串（含 `-` 分组）。
#[must_use]
pub fn encode_recovery_key(recovery_key: &RecoveryKey) -> String {
    let mut payload = Zeroizing::new([0u8; RECOVERY_KEY_LEN + RECOVERY_CHECKSUM_LEN]);
    payload[..RECOVERY_KEY_LEN].copy_from_slice(recovery_key.as_bytes());
    let checksum = Sha256::digest(recovery_key.as_bytes());
    payload[RECOVERY_KEY_LEN..].copy_from_slice(&checksum[..RECOVERY_CHECKSUM_LEN]);

    let encoded = base32_encode(&payload[..]);

    let mut display =
        String::with_capacity(encoded.len() + encoded.len() / RECOVERY_DISPLAY_GROUP_SIZE);
    for (index, symbol) in encoded.chars().enumerate() {
        if index > 0 && index % RECOVERY_DISPLAY_GROUP_SIZE == 0 {
            display.push('-');
        }
        display.push(symbol);
    }
    display
}

/// 解析用户输入的恢复密钥。
///
/// 长度、字符集或填充位不合法返回 [`CryptoError::InvalidRecoveryEncoding`]；
/// 校验位不匹配返回 [`CryptoError::RecoveryChecksumMismatch`]（提示用户核对抄写）。
pub fn parse_recovery_key(display: &str) -> Result<RecoveryKey, CryptoError> {
    let mut symbols = Vec::with_capacity(RECOVERY_SYMBOL_COUNT);

    for character in display.chars() {
        if character == '-' || character.is_whitespace() {
            continue;
        }
        let upper = character.to_ascii_uppercase();
        if upper == 'U' {
            return Err(CryptoError::InvalidRecoveryEncoding);
        }
        let mapped = match upper {
            'I' | 'L' => '1',
            'O' => '0',
            other => other,
        };
        if !ALPHABET.contains(&(mapped as u8)) {
            return Err(CryptoError::InvalidRecoveryEncoding);
        }
        symbols.push(mapped);
    }

    if symbols.len() != RECOVERY_SYMBOL_COUNT {
        return Err(CryptoError::InvalidRecoveryEncoding);
    }

    let mut payload: Vec<u8> = Vec::with_capacity(RECOVERY_PAYLOAD_LEN);
    let mut buffer: u32 = 0;
    let mut bits: u32 = 0;

    for symbol in symbols {
        let value = alphabet_index(symbol).ok_or(CryptoError::InvalidRecoveryEncoding)?;
        buffer = (buffer << 5) | u32::from(value);
        bits += 5;
        while bits >= 8 {
            bits -= 8;
            payload.push(((buffer >> bits) & 0xff) as u8);
        }
    }

    if payload.len() != RECOVERY_PAYLOAD_LEN {
        return Err(CryptoError::InvalidRecoveryEncoding);
    }

    let payload = Zeroizing::new(payload);

    // 最后一组补的 3 个 0 位必须确实是 0，否则不是本编码器的规范输出。
    if bits > 0 && (buffer & ((1 << bits) - 1)) != 0 {
        return Err(CryptoError::InvalidRecoveryEncoding);
    }

    let mut key_bytes = [0u8; RECOVERY_KEY_LEN];
    key_bytes.copy_from_slice(&payload[..RECOVERY_KEY_LEN]);
    let recovery_key = RecoveryKey::from_bytes(key_bytes);

    let expected = Sha256::digest(recovery_key.as_bytes());
    let checksum_matches: bool = expected[..RECOVERY_CHECKSUM_LEN]
        .ct_eq(&payload[RECOVERY_KEY_LEN..])
        .into();

    if checksum_matches {
        Ok(recovery_key)
    } else {
        Err(CryptoError::RecoveryChecksumMismatch)
    }
}

fn base32_encode(data: &[u8]) -> String {
    let mut encoded = String::with_capacity(RECOVERY_SYMBOL_COUNT);
    let mut buffer: u32 = 0;
    let mut bits: u32 = 0;

    for &byte in data {
        buffer = (buffer << 8) | u32::from(byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            let index = ((buffer >> bits) & 0x1f) as usize;
            encoded.push(char::from(ALPHABET[index]));
        }
    }

    if bits > 0 {
        let index = ((buffer << (5 - bits)) & 0x1f) as usize;
        encoded.push(char::from(ALPHABET[index]));
    }

    encoded
}

fn alphabet_index(symbol: char) -> Option<u8> {
    let byte = symbol as u8;
    ALPHABET
        .iter()
        .position(|candidate| *candidate == byte)
        .map(|index| index as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_key() -> RecoveryKey {
        let mut bytes = [0u8; RECOVERY_KEY_LEN];
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = index as u8;
        }
        RecoveryKey::from_bytes(bytes)
    }

    #[test]
    fn display_format_matches_spec() {
        let display = encode_recovery_key(&sample_key());
        let groups: Vec<&str> = display.split('-').collect();

        assert_eq!(RECOVERY_SYMBOL_COUNT, 55);
        assert_eq!(groups.len(), 11);
        assert!(groups.iter().all(|group| group.len() == 5));
        assert_eq!(display.chars().filter(|c| *c != '-').count(), 55);
        assert!(
            display
                .chars()
                .all(|c| c == '-' || ALPHABET.contains(&(c as u8)))
        );
    }

    #[test]
    fn roundtrip() {
        let key = sample_key();
        let display = encode_recovery_key(&key);
        let parsed = parse_recovery_key(&display).unwrap();
        assert_eq!(parsed.as_bytes(), key.as_bytes());
    }

    #[test]
    fn one_wrong_character_is_detected() {
        let display = encode_recovery_key(&sample_key());
        let mut characters: Vec<char> = display.chars().collect();

        // 把第一个非分隔字符换成字母表里的另一个字符（保持长度与填充位不变）。
        let position = characters.iter().position(|c| *c != '-').unwrap();
        characters[position] = if characters[position] == '0' {
            '1'
        } else {
            '0'
        };
        let corrupted: String = characters.into_iter().collect();

        assert_eq!(
            parse_recovery_key(&corrupted),
            Err(CryptoError::RecoveryChecksumMismatch)
        );
    }

    #[test]
    fn lowercase_and_ambiguous_characters_are_accepted() {
        let key = sample_key();
        let display = encode_recovery_key(&key);

        let lowercased = display.to_lowercase();
        assert_eq!(
            parse_recovery_key(&lowercased).unwrap().as_bytes(),
            key.as_bytes()
        );

        let without_hyphens = display.replace('-', " ");
        assert_eq!(
            parse_recovery_key(&without_hyphens).unwrap().as_bytes(),
            key.as_bytes()
        );

        // 0 -> O、1 -> I/L 是 Crockford 的等价输入。
        let with_letters = display.replace('0', "O").replace('1', "l");
        assert_eq!(
            parse_recovery_key(&with_letters).unwrap().as_bytes(),
            key.as_bytes()
        );
    }

    #[test]
    fn non_zero_padding_is_rejected() {
        let display = encode_recovery_key(&sample_key());
        let mut characters: Vec<char> = display.chars().collect();
        let last = characters.len() - 1;

        // 最后一个符号的低 3 位是填充位；构造一个高 2 位不变、低 3 位非零的符号。
        let index = alphabet_index(characters[last]).unwrap();
        let corrupted_index = (index & 0b1_1000) | 0b001;
        characters[last] = char::from(ALPHABET[usize::from(corrupted_index)]);
        let corrupted: String = characters.into_iter().collect();

        assert_eq!(
            parse_recovery_key(&corrupted),
            Err(CryptoError::InvalidRecoveryEncoding)
        );
    }

    #[test]
    fn wrong_length_is_rejected() {
        let display = encode_recovery_key(&sample_key());
        let short = &display[..display.len() - 1];
        assert_eq!(
            parse_recovery_key(short),
            Err(CryptoError::InvalidRecoveryEncoding)
        );
        assert_eq!(
            parse_recovery_key(""),
            Err(CryptoError::InvalidRecoveryEncoding)
        );
    }

    #[test]
    fn forbidden_character_is_rejected() {
        let display = encode_recovery_key(&sample_key());
        let corrupted = display.replacen(&display.chars().next().unwrap().to_string(), "U", 1);
        assert_eq!(
            parse_recovery_key(&corrupted),
            Err(CryptoError::InvalidRecoveryEncoding)
        );
    }

    #[test]
    fn recovery_keys_are_independent_and_deterministic() {
        let key = sample_key();
        let first = derive_recovery_keys(&key).unwrap();
        let second = derive_recovery_keys(&key).unwrap();

        assert_eq!(first.rk_wrap.as_bytes(), second.rk_wrap.as_bytes());
        assert_eq!(first.rk_auth.as_bytes(), second.rk_auth.as_bytes());
        assert_ne!(first.rk_wrap.as_bytes(), first.rk_auth.as_bytes());
    }

    #[test]
    fn generated_recovery_key_roundtrips() {
        let key = RecoveryKey::generate().unwrap();
        let display = encode_recovery_key(&key);
        assert_eq!(
            parse_recovery_key(&display).unwrap().as_bytes(),
            key.as_bytes()
        );
    }
}
