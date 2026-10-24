//! 内部十六进制编解码。
//!
//! 只用于名称令牌的文本表示与测试向量；不用于密钥传输协议。

/// 小写十六进制编码。
pub(crate) fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(HEX[usize::from(byte >> 4)] as char);
        out.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    out
}

/// 十六进制解码；输入必须是偶数长度且只含 `[0-9a-fA-F]`。
pub(crate) fn decode_hex(input: &str) -> Option<Vec<u8>> {
    if !input.len().is_multiple_of(2) {
        return None;
    }
    let mut out = Vec::with_capacity(input.len() / 2);
    let bytes = input.as_bytes();
    for pair in bytes.as_chunks::<2>().0 {
        let hi = hex_value(pair[0])?;
        let lo = hex_value(pair[1])?;
        out.push((hi << 4) | lo);
    }
    Some(out)
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{decode_hex, encode_hex};

    #[test]
    fn hex_roundtrip() {
        let bytes = [0x00, 0x0f, 0xa5, 0xff];
        let encoded = encode_hex(&bytes);
        assert_eq!(encoded, "000fa5ff");
        assert_eq!(decode_hex(&encoded).as_deref(), Some(&bytes[..]));
        assert_eq!(decode_hex("ABCD").as_deref(), Some(&[0xab, 0xcd][..]));
    }

    #[test]
    fn rejects_malformed_hex() {
        assert!(decode_hex("abc").is_none());
        assert!(decode_hex("zz").is_none());
        assert!(decode_hex("0x12").is_none());
    }
}
