//! 性质测试（proptest）：用随机数据验证往返与完整性约束。
//!
//! 这里刻意不覆盖 Argon2id 派生（每次派生 64 MiB 内存代价太高不适合随机化），
//! KDF 由固定向量与单元测试覆盖。

use proptest::prelude::*;

use crate::{
    ContentId, MasterKey, NodeId, NodeKey, RecoveryKey, aead, chunk, compute_name_token,
    decrypt_chunk, derive_content_key, derive_name_key, encode_recovery_key, encrypt_chunk,
    parse_recovery_key,
};

fn key32() -> impl Strategy<Value = [u8; 32]> {
    proptest::collection::vec(any::<u8>(), 32).prop_map(|bytes| {
        let mut key = [0u8; 32];
        key.copy_from_slice(&bytes);
        key
    })
}

fn nonce24() -> impl Strategy<Value = [u8; 24]> {
    proptest::collection::vec(any::<u8>(), 24).prop_map(|bytes| {
        let mut nonce = [0u8; 24];
        nonce.copy_from_slice(&bytes);
        nonce
    })
}

proptest! {
    #[test]
    fn aead_roundtrip(
        key in key32(),
        nonce in nonce24(),
        aad in proptest::collection::vec(any::<u8>(), 0..64),
        plaintext in proptest::collection::vec(any::<u8>(), 0..512),
    ) {
        let frame = aead::seal_with_nonce(&key, &nonce, &aad, &plaintext).unwrap();
        prop_assert_eq!(frame.len(), plaintext.len() + aead::FRAME_OVERHEAD);
        prop_assert_eq!(aead::open(&key, &aad, &frame).unwrap(), plaintext);
    }

    #[test]
    fn aead_detects_tampering(
        key in key32(),
        nonce in nonce24(),
        plaintext in proptest::collection::vec(any::<u8>(), 1..256),
        position in 0usize..64,
        mask in any::<u8>(),
    ) {
        let frame = aead::seal_with_nonce(&key, &nonce, b"aad", &plaintext).unwrap();
        let index = position % frame.len();
        let mut tampered = frame.clone();
        tampered[index] ^= if mask == 0 { 1 } else { mask };

        prop_assert!(aead::open(&key, b"aad", &tampered).is_err());
    }

    #[test]
    fn name_token_ignores_ascii_case(
        name in "[A-Za-z0-9 ._-]{0,24}",
    ) {
        let name_key = derive_name_key(&MasterKey::from_bytes([0x01; 32])).unwrap();
        let parent = NodeId::parse("550e8400-e29b-41d4-a716-446655440000").unwrap();

        let upper = compute_name_token(&name_key, &parent, &name).unwrap();
        let lower = compute_name_token(&name_key, &parent, &name.to_lowercase()).unwrap();
        prop_assert_eq!(upper, lower);
    }

    #[test]
    fn recovery_display_roundtrip(key in key32()) {
        let recovery_key = RecoveryKey::from_bytes(key);
        let display = encode_recovery_key(&recovery_key);
        let parsed = parse_recovery_key(&display).unwrap();
        prop_assert_eq!(parsed, recovery_key);
    }

    #[test]
    fn chunk_roundtrip(
        node_key in key32(),
        plaintext in proptest::collection::vec(any::<u8>(), 0..1024),
    ) {
        let content_id = ContentId::parse("6ba7b811-9dad-41d1-80b4-00c04fd430c8").unwrap();
        let node_key = NodeKey::from_bytes(node_key);
        let content_key = derive_content_key(&node_key, &content_id).unwrap();

        let ciphertext = encrypt_chunk(&content_key, &content_id, 0, 1, true, &plaintext).unwrap();
        let decrypted = decrypt_chunk(&content_key, &content_id, 0, 1, true, &ciphertext).unwrap();
        prop_assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn chunk_detects_any_flipped_byte(
        node_key in key32(),
        plaintext in proptest::collection::vec(any::<u8>(), 1..256),
        position in 0usize..300,
        mask in any::<u8>(),
    ) {
        let content_id = ContentId::parse("6ba7b811-9dad-41d1-80b4-00c04fd430c8").unwrap();
        let node_key = NodeKey::from_bytes(node_key);
        let content_key = derive_content_key(&node_key, &content_id).unwrap();

        let ciphertext = encrypt_chunk(&content_key, &content_id, 0, 1, true, &plaintext).unwrap();
        let mut tampered = ciphertext.clone();
        let index = position % tampered.len();
        tampered[index] ^= if mask == 0 { 1 } else { mask };

        prop_assert!(decrypt_chunk(&content_key, &content_id, 0, 1, true, &tampered).is_err());
    }

    #[test]
    fn chunk_count_covers_size(size in 0u64..(8 * 1024 * 1024)) {
        let count = chunk::chunk_count_for_size(size).unwrap();
        prop_assert!(count >= 1);
        let minimum = (size.max(1)).div_ceil(chunk::CHUNK_PLAINTEXT_SIZE as u64);
        prop_assert_eq!(u64::from(count), minimum);
    }
}
