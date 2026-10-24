//! 文件内容的分块加密（design D5）。
//!
//! ```text
//! content_key = HKDF(node_key, salt=content_id, info="bk/v1/content")
//! chunk_i     = AEAD(content_key, data_i,
//!                    aad="bk/v1/chunk|" ‖ content_id ‖ u32be(i) ‖ u32be(n) ‖ u8(is_last))
//! ```
//!
//! 约束：
//! - 明文块大小固定 4 MiB，只有最后一块可以更小；空文件是“恰好一块空块”。
//! - AAD 绑定 content_id / 序号 / 总块数 / 是否最后一块，因此重排、截断、
//!   跨内容拼接都会解密失败。
//! - 每次上传产生一个新 content（ID 由客户端生成）；恢复旧版本时新版本直接引用原
//!   content，不重新加密也不重复计配额。

use zeroize::Zeroizing;

use crate::domain::HKDF_INFO_CONTENT;
use crate::error::CryptoError;
use crate::ids::ContentId;
use crate::kdf::hkdf_expand;
use crate::keys::{ContentKey, NodeKey};
use crate::{aad, aead};

/// 明文块大小：4 MiB。
pub const CHUNK_PLAINTEXT_SIZE: usize = 4 * 1024 * 1024;

/// 单块密文的额外开销（帧头 + tag）。
pub const CHUNK_CIPHERTEXT_OVERHEAD: usize = aead::FRAME_OVERHEAD;

/// 单块密文的最大长度（4 MiB 明文 + 41 字节）。
pub const MAX_CHUNK_CIPHERTEXT_SIZE: usize = CHUNK_PLAINTEXT_SIZE + CHUNK_CIPHERTEXT_OVERHEAD;

/// 计算给定明文长度需要的块数。
///
/// 0 字节文件也需要恰好一块（空块），见 client-encryption spec 的“空文件”场景。
pub fn chunk_count_for_size(plaintext_size: u64) -> Result<u32, CryptoError> {
    let effective = plaintext_size.max(1);
    let chunk_size = CHUNK_PLAINTEXT_SIZE as u64;
    let count = effective.div_ceil(chunk_size);
    u32::try_from(count).map_err(|_| CryptoError::InvalidChunkLayout)
}

/// 由节点密钥与 content_id 派生内容密钥。
pub fn derive_content_key(
    node_key: &NodeKey,
    content_id: &ContentId,
) -> Result<ContentKey, CryptoError> {
    Ok(ContentKey::from_bytes(hkdf_expand(
        node_key.as_bytes(),
        Some(content_id.as_bytes()),
        HKDF_INFO_CONTENT,
    )?))
}

/// 加密一块明文，返回可直传对象存储的密文帧。
pub fn encrypt_chunk(
    content_key: &ContentKey,
    content_id: &ContentId,
    index: u32,
    count: u32,
    is_last: bool,
    plaintext: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    validate_layout(index, count, is_last)?;
    validate_plaintext_size(index, count, plaintext.len())?;

    aead::seal_random(
        content_key.as_bytes(),
        &aad::chunk(content_id, index, count, is_last),
        plaintext,
    )
}

/// 解密一块密文。
///
/// 调用方必须传入该块应有的 `index`/`count`/`is_last`（来自版本块清单），
/// 而不是从密文中读取——否则服务端可以篡改块顺序而不被发现。
///
/// 返回值用 [`Zeroizing`] 包裹：明文在离开作用域时会被清零，且不再额外复制。
pub fn decrypt_chunk(
    content_key: &ContentKey,
    content_id: &ContentId,
    index: u32,
    count: u32,
    is_last: bool,
    ciphertext: &[u8],
) -> Result<Zeroizing<Vec<u8>>, CryptoError> {
    validate_layout(index, count, is_last)?;

    // 先限制长度，避免超大“块”造成内存放大。
    if ciphertext.len() > MAX_CHUNK_CIPHERTEXT_SIZE {
        return Err(CryptoError::InvalidChunkLayout);
    }

    let plaintext = Zeroizing::new(aead::open(
        content_key.as_bytes(),
        &aad::chunk(content_id, index, count, is_last),
        ciphertext,
    )?);

    // 非最后一块必须是满块，防止服务端返回一个被“缩短”的中间块。
    validate_plaintext_size(index, count, plaintext.len())?;

    Ok(plaintext)
}

fn validate_layout(index: u32, count: u32, is_last: bool) -> Result<(), CryptoError> {
    if count == 0 || index >= count {
        return Err(CryptoError::InvalidChunkLayout);
    }
    if is_last != (index + 1 == count) {
        return Err(CryptoError::InvalidChunkLayout);
    }
    Ok(())
}

fn validate_plaintext_size(index: u32, count: u32, len: usize) -> Result<(), CryptoError> {
    if len > CHUNK_PLAINTEXT_SIZE {
        return Err(CryptoError::InvalidChunkLayout);
    }

    let expected_last = index + 1 == count;
    if expected_last {
        // 空块只允许出现在“只有一个块”的空文件里。
        if len == 0 && count != 1 {
            return Err(CryptoError::InvalidChunkLayout);
        }
    } else if len != CHUNK_PLAINTEXT_SIZE {
        return Err(CryptoError::InvalidChunkLayout);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn content_id() -> ContentId {
        ContentId::parse("550e8400-e29b-41d4-a716-446655440000").unwrap()
    }

    fn other_content_id() -> ContentId {
        ContentId::parse("550e8400-e29b-41d4-a716-446655440002").unwrap()
    }

    fn key_for(content_id: &ContentId) -> ContentKey {
        let node_key = NodeKey::generate().unwrap();
        derive_content_key(&node_key, content_id).unwrap()
    }

    #[test]
    fn single_chunk_roundtrip() {
        let content = content_id();
        let key = key_for(&content);
        let plaintext = b"hello chunk".to_vec();

        let ciphertext = encrypt_chunk(&key, &content, 0, 1, true, &plaintext).unwrap();
        assert_eq!(
            ciphertext.len(),
            CHUNK_CIPHERTEXT_OVERHEAD + plaintext.len()
        );
        let decrypted = decrypt_chunk(&key, &content, 0, 1, true, &ciphertext).unwrap();
        assert_eq!(decrypted.as_slice(), plaintext.as_slice());
    }

    #[test]
    fn empty_file_has_exactly_one_empty_chunk() {
        assert_eq!(chunk_count_for_size(0).unwrap(), 1);
        assert_eq!(chunk_count_for_size(1).unwrap(), 1);
        assert_eq!(
            chunk_count_for_size(CHUNK_PLAINTEXT_SIZE as u64).unwrap(),
            1
        );
        assert_eq!(
            chunk_count_for_size(CHUNK_PLAINTEXT_SIZE as u64 + 1).unwrap(),
            2
        );

        let content = content_id();
        let key = key_for(&content);
        let ciphertext = encrypt_chunk(&key, &content, 0, 1, true, &[]).unwrap();
        assert_eq!(ciphertext.len(), CHUNK_CIPHERTEXT_OVERHEAD);
        assert!(
            decrypt_chunk(&key, &content, 0, 1, true, &ciphertext)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn multi_chunk_roundtrip() {
        let content = content_id();
        let key = key_for(&content);

        let first = vec![0x41u8; CHUNK_PLAINTEXT_SIZE];
        let second = vec![0x42u8; 1024];
        let count = 2;

        let first_ciphertext = encrypt_chunk(&key, &content, 0, count, false, &first).unwrap();
        let second_ciphertext = encrypt_chunk(&key, &content, 1, count, true, &second).unwrap();

        assert_eq!(
            decrypt_chunk(&key, &content, 0, count, false, &first_ciphertext)
                .unwrap()
                .as_slice(),
            first.as_slice()
        );
        assert_eq!(
            decrypt_chunk(&key, &content, 1, count, true, &second_ciphertext)
                .unwrap()
                .as_slice(),
            second.as_slice()
        );
    }

    #[test]
    fn reordered_chunks_are_detected() {
        let content = content_id();
        let key = key_for(&content);
        let count = 2;

        let first = vec![0x41u8; CHUNK_PLAINTEXT_SIZE];
        let second = vec![0x42u8; 16];

        let first_ciphertext = encrypt_chunk(&key, &content, 0, count, false, &first).unwrap();
        let second_ciphertext = encrypt_chunk(&key, &content, 1, count, true, &second).unwrap();

        assert_eq!(
            decrypt_chunk(&key, &content, 1, count, true, &first_ciphertext),
            Err(CryptoError::AuthenticationFailed)
        );
        assert_eq!(
            decrypt_chunk(&key, &content, 0, count, false, &second_ciphertext),
            Err(CryptoError::AuthenticationFailed)
        );
    }

    #[test]
    fn truncated_content_is_detected() {
        let content = content_id();
        let key = key_for(&content);
        let count = 3;

        let last = vec![0x43u8; 8];
        let last_ciphertext = encrypt_chunk(&key, &content, 2, count, true, &last).unwrap();

        // 服务端声称只有 2 块：count 变化 → AAD 不匹配。
        assert_eq!(
            decrypt_chunk(&key, &content, 1, 2, true, &last_ciphertext),
            Err(CryptoError::AuthenticationFailed)
        );
    }

    #[test]
    fn chunk_cannot_be_moved_to_another_content() {
        let content = content_id();
        let other = other_content_id();
        let key = key_for(&content);
        let other_key = key_for(&other);

        let plaintext = vec![0x44u8; 64];
        let ciphertext = encrypt_chunk(&key, &content, 0, 1, true, &plaintext).unwrap();

        // 内容 ID 与 content_key 都不同，两种组合都必须失败。
        assert_eq!(
            decrypt_chunk(&key, &other, 0, 1, true, &ciphertext),
            Err(CryptoError::AuthenticationFailed)
        );
        assert_eq!(
            decrypt_chunk(&other_key, &content, 0, 1, true, &ciphertext),
            Err(CryptoError::AuthenticationFailed)
        );
    }

    #[test]
    fn content_key_depends_on_content_id() {
        let content = content_id();
        let other = other_content_id();
        let node_key = NodeKey::generate().unwrap();

        let first = derive_content_key(&node_key, &content).unwrap();
        let second = derive_content_key(&node_key, &other).unwrap();
        assert_ne!(first.as_bytes(), second.as_bytes());

        let third = derive_content_key(&node_key, &content).unwrap();
        assert_eq!(first.as_bytes(), third.as_bytes());
    }

    #[test]
    fn invalid_layout_is_rejected() {
        let content = content_id();
        let key = key_for(&content);

        assert_eq!(
            encrypt_chunk(&key, &content, 0, 0, false, &[]),
            Err(CryptoError::InvalidChunkLayout)
        );
        assert_eq!(
            encrypt_chunk(&key, &content, 1, 1, true, &[]),
            Err(CryptoError::InvalidChunkLayout)
        );
        // index=0 不是最后一块却标记 is_last。
        assert_eq!(
            encrypt_chunk(&key, &content, 0, 2, true, &vec![0u8; CHUNK_PLAINTEXT_SIZE]),
            Err(CryptoError::InvalidChunkLayout)
        );
        // 非最后一块必须是满块。
        assert_eq!(
            encrypt_chunk(&key, &content, 0, 2, false, &[0u8; 10]),
            Err(CryptoError::InvalidChunkLayout)
        );
        // 超过 4 MiB。
        assert_eq!(
            encrypt_chunk(
                &key,
                &content,
                0,
                1,
                true,
                &vec![0u8; CHUNK_PLAINTEXT_SIZE + 1]
            ),
            Err(CryptoError::InvalidChunkLayout)
        );
        // 多块内容里出现空块。
        assert_eq!(
            encrypt_chunk(&key, &content, 1, 2, true, &[]),
            Err(CryptoError::InvalidChunkLayout)
        );
    }

    #[test]
    fn oversized_ciphertext_is_rejected_before_decryption() {
        let content = content_id();
        let key = key_for(&content);
        let oversized = vec![0u8; MAX_CHUNK_CIPHERTEXT_SIZE + 1];

        assert_eq!(
            decrypt_chunk(&key, &content, 0, 1, true, &oversized),
            Err(CryptoError::InvalidChunkLayout)
        );
    }

    #[test]
    fn tampered_chunk_is_rejected() {
        let content = content_id();
        let key = key_for(&content);
        let mut ciphertext = encrypt_chunk(&key, &content, 0, 1, true, b"data").unwrap();
        let last = ciphertext.len() - 1;
        ciphertext[last] ^= 0x01;

        assert!(decrypt_chunk(&key, &content, 0, 1, true, &ciphertext).is_err());
    }
}
