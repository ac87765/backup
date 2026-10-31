//! 小写十六进制编解码。
//!
//! 服务端 API 的密钥材料、盐与包裹密文都是十六进制字符串；这里只做格式转换，
//! 不解释内容。解码失败返回 `None`，由调用方映射为强类型错误。

/// 编码为小写十六进制。
#[must_use]
pub fn encode_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    output
}

/// 解码任意长度的十六进制字符串；奇数长度或非法字符返回 `None`。
#[must_use]
pub fn decode_hex(value: &str) -> Option<Vec<u8>> {
    let bytes = value.as_bytes();
    if !bytes.len().is_multiple_of(2) {
        return None;
    }
    let (pairs, remainder) = bytes.as_chunks::<2>();
    if !remainder.is_empty() {
        return None;
    }
    let mut output = Vec::with_capacity(pairs.len());
    for pair in pairs {
        output.push((hex_digit(pair[0])? << 4) | hex_digit(pair[1])?);
    }
    Some(output)
}

/// 解码固定长度的十六进制字符串。
#[must_use]
pub fn decode_hex_array<const N: usize>(value: &str) -> Option<[u8; N]> {
    decode_hex(value)?.try_into().ok()
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;

    #[test]
    fn roundtrip_and_case_insensitive_decode() {
        let bytes = [0x00, 0x0f, 0xa5, 0xff];
        assert_eq!(encode_hex(&bytes), "000fa5ff");
        assert_eq!(decode_hex("000fa5ff").unwrap(), bytes);
        assert_eq!(decode_hex("000FA5FF").unwrap(), bytes);
    }

    #[test]
    fn rejects_odd_length_and_invalid_characters() {
        assert!(decode_hex("0").is_none());
        assert!(decode_hex("zz").is_none());
        assert!(decode_hex("00 0").is_none());
        assert!(decode_hex_array::<3>("000fa5ff").is_none());
    }
}
