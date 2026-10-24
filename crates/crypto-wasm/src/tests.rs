//! 在 `wasm-pack test --headless --chrome` 下运行的测试。
//!
//! 覆盖两类内容：
//! 1. `testdata/crypto-vectors/` 的**全部**跨实现测试向量（浏览器 WASM 实现必须与
//!    Rust 原生实现逐字节一致）；
//! 2. `KeyStore` 的对外契约：默认 KDF 参数可在浏览器跑通、密钥只留在 WASM 内、
//!    `lock()` 之后需要密钥的调用一律返回 `locked`。

use crypto_core::{
    ContentId, NodeId, NodeKey, VersionId, aad, aead, derive_content_key, derive_name_key,
    derive_password_keys, derive_recovery_keys, derive_root_key, encode_recovery_key,
    parse_recovery_key,
};
use serde::Deserialize;
use wasm_bindgen_test::*;

use crate::KeyStore;

wasm_bindgen_test_configure!(run_in_browser);

// ---------------------------------------------------------------------------
// 向量文件（编译期嵌入，浏览器测试无法读取本地文件系统）
// ---------------------------------------------------------------------------

const AEAD_VECTORS: &str = include_str!("../../../testdata/crypto-vectors/aead.json");
const KDF_VECTORS: &str = include_str!("../../../testdata/crypto-vectors/kdf.json");
const NODE_VECTORS: &str = include_str!("../../../testdata/crypto-vectors/node.json");
const CHUNK_VECTORS: &str = include_str!("../../../testdata/crypto-vectors/chunk.json");
const RECOVERY_VECTORS: &str = include_str!("../../../testdata/crypto-vectors/recovery.json");

#[derive(Deserialize)]
struct AeadVectors {
    cases: Vec<AeadCase>,
}

#[derive(Deserialize)]
struct AeadCase {
    name: String,
    key_hex: String,
    nonce_hex: String,
    aad_hex: String,
    plaintext_hex: String,
    frame_hex: String,
}

#[derive(Deserialize)]
struct KdfVectors {
    cases: Vec<KdfCase>,
}

#[derive(Deserialize)]
struct KdfCase {
    name: String,
    password: String,
    salt_hex: String,
    params: crypto_core::KdfParams,
    root_hex: String,
    pwd_wrap_hex: String,
    auth_key_hex: String,
}

#[derive(Deserialize)]
struct NodeVectors {
    cases: Vec<NodeCase>,
}

#[derive(Deserialize)]
struct NodeCase {
    name: String,
    node_id: String,
    parent_id: String,
    version_id: String,
    content_id: String,
    master_key_hex: String,
    node_key_hex: String,
    wrapped_node_key_hex: String,
    node_meta_plaintext: String,
    node_meta_frame_hex: String,
    version_meta_frame_hex: String,
    file_name: String,
    name_token_hex: String,
}

#[derive(Deserialize)]
struct ChunkVectors {
    cases: Vec<ChunkCase>,
}

#[derive(Deserialize)]
struct ChunkCase {
    name: String,
    content_id: String,
    node_key_hex: String,
    content_key_hex: String,
    index: u32,
    count: u32,
    is_last: bool,
    aad_hex: String,
    nonce_hex: String,
    plaintext_hex: String,
    frame_hex: String,
}

#[derive(Deserialize)]
struct RecoveryVectors {
    cases: Vec<RecoveryCase>,
}

#[derive(Deserialize)]
struct RecoveryCase {
    name: String,
    recovery_key_hex: String,
    display: String,
    rk_wrap_hex: String,
    rk_auth_hex: String,
}

// ---------------------------------------------------------------------------
// 工具
// ---------------------------------------------------------------------------

fn hex_decode(input: &str) -> Vec<u8> {
    assert!(input.len().is_multiple_of(2), "非法十六进制长度");
    input
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| (hex_value(pair[0]) << 4) | hex_value(pair[1]))
        .collect()
}

fn hex_value(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        b'A'..=b'F' => byte - b'A' + 10,
        _ => panic!("非法十六进制字符"),
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(HEX[usize::from(byte >> 4)] as char);
        out.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    out
}

fn key32(hex: &str) -> [u8; 32] {
    hex_decode(hex).try_into().unwrap()
}

fn error_code(error: &crate::KeyStoreError) -> &'static str {
    error.code()
}

/// 与 crypto-core 的默认参数一致（design D4）。
const DEFAULT_KDF_PARAMS: &str =
    r#"{"algorithm":"argon2id","m_cost_kib":65536,"t_cost":3,"p_cost":1}"#;

// ---------------------------------------------------------------------------
// 测试向量
// ---------------------------------------------------------------------------

#[wasm_bindgen_test]
fn aead_vectors_match_in_wasm() {
    let vectors: AeadVectors = serde_json::from_str(AEAD_VECTORS).unwrap();
    assert!(!vectors.cases.is_empty());

    for case in &vectors.cases {
        let key = key32(&case.key_hex);
        let nonce: [u8; 24] = hex_decode(&case.nonce_hex).try_into().unwrap();
        let aad_bytes = hex_decode(&case.aad_hex);
        let plaintext = hex_decode(&case.plaintext_hex);
        let expected = hex_decode(&case.frame_hex);

        let frame = aead::seal_with_nonce(&key, &nonce, &aad_bytes, &plaintext).unwrap();
        assert_eq!(frame, expected, "case {}", case.name);
        assert_eq!(
            aead::open(&key, &aad_bytes, &frame).unwrap(),
            plaintext,
            "case {}",
            case.name
        );
    }
}

#[wasm_bindgen_test]
fn kdf_vectors_match_in_wasm() {
    let vectors: KdfVectors = serde_json::from_str(KDF_VECTORS).unwrap();
    assert!(!vectors.cases.is_empty());

    for case in &vectors.cases {
        let salt: [u8; 16] = hex_decode(&case.salt_hex).try_into().unwrap();
        let root = derive_root_key(&case.password, &salt, &case.params).unwrap();
        assert_eq!(
            hex_encode(root.expose_for_test_vectors()),
            case.root_hex,
            "case {}",
            case.name
        );

        let keys = derive_password_keys(&root).unwrap();
        assert_eq!(
            hex_encode(keys.pwd_wrap.expose_for_test_vectors()),
            case.pwd_wrap_hex
        );
        assert_eq!(
            hex_encode(keys.auth_key.expose_secret()),
            case.auth_key_hex,
            "case {}",
            case.name
        );
    }
}

#[wasm_bindgen_test]
fn node_vectors_match_in_wasm() {
    use crypto_core::{
        compute_name_token, decrypt_node_metadata, decrypt_version_metadata_for_content,
        unwrap_node_key,
    };

    let vectors: NodeVectors = serde_json::from_str(NODE_VECTORS).unwrap();

    for case in &vectors.cases {
        let node_id = NodeId::parse(&case.node_id).unwrap();
        let parent_id = NodeId::parse(&case.parent_id).unwrap();
        let version_id = VersionId::parse(&case.version_id).unwrap();
        let content_id = ContentId::parse(&case.content_id).unwrap();
        let master_key = crypto_core::MasterKey::from_bytes(key32(&case.master_key_hex));
        let node_key = NodeKey::from_bytes(key32(&case.node_key_hex));

        // 包裹的节点密钥可以解开，且 AAD 绑定 node_id。
        let wrapped = hex_decode(&case.wrapped_node_key_hex);
        assert_eq!(
            unwrap_node_key(&master_key, &node_id, &wrapped).unwrap(),
            node_key,
            "case {}",
            case.name
        );

        // 节点元数据。
        let node_meta =
            decrypt_node_metadata(&node_key, &node_id, &hex_decode(&case.node_meta_frame_hex))
                .unwrap();
        assert_eq!(node_meta.name, case.file_name);
        assert_eq!(
            String::from_utf8(
                aead::open(
                    node_key.expose_for_test_vectors(),
                    &aad::node_metadata(&node_id),
                    &hex_decode(&case.node_meta_frame_hex)
                )
                .unwrap()
            )
            .unwrap(),
            case.node_meta_plaintext
        );

        // 版本元数据必须绑定 content。
        let version_meta = decrypt_version_metadata_for_content(
            &node_key,
            &version_id,
            &content_id,
            &hex_decode(&case.version_meta_frame_hex),
        )
        .unwrap();
        assert_eq!(version_meta.content_id, content_id);

        // 名称令牌（NFC(lower(NFC(name)))）。
        let name_key = derive_name_key(&master_key).unwrap();
        let token = compute_name_token(&name_key, &parent_id, &case.file_name).unwrap();
        assert_eq!(token.to_hex(), case.name_token_hex, "case {}", case.name);
    }
}

#[wasm_bindgen_test]
fn chunk_vectors_match_in_wasm() {
    use crypto_core::{decrypt_chunk, encrypt_chunk};

    let vectors: ChunkVectors = serde_json::from_str(CHUNK_VECTORS).unwrap();

    for case in &vectors.cases {
        let content_id = ContentId::parse(&case.content_id).unwrap();
        let node_key = NodeKey::from_bytes(key32(&case.node_key_hex));
        let content_key = derive_content_key(&node_key, &content_id).unwrap();
        assert_eq!(
            hex_encode(content_key.expose_for_test_vectors()),
            case.content_key_hex,
            "case {}",
            case.name
        );

        assert_eq!(
            hex_encode(&aad::chunk(
                &content_id,
                case.index,
                case.count,
                case.is_last
            )),
            case.aad_hex
        );

        let frame = hex_decode(&case.frame_hex);
        let plaintext = decrypt_chunk(
            &content_key,
            &content_id,
            case.index,
            case.count,
            case.is_last,
            &frame,
        )
        .unwrap();
        assert_eq!(
            plaintext.as_slice(),
            hex_decode(&case.plaintext_hex),
            "case {}",
            case.name
        );

        // 确定性封帧结果必须与向量一致。
        let nonce: [u8; 24] = hex_decode(&case.nonce_hex).try_into().unwrap();
        assert_eq!(
            aead::seal_with_nonce(
                content_key.expose_for_test_vectors(),
                &nonce,
                &aad::chunk(&content_id, case.index, case.count, case.is_last),
                &plaintext
            )
            .unwrap(),
            frame,
            "case {}",
            case.name
        );

        // 高层 API（随机 nonce）也要能往返。
        let encrypted = encrypt_chunk(
            &content_key,
            &content_id,
            case.index,
            case.count,
            case.is_last,
            &plaintext,
        )
        .unwrap();
        assert_eq!(
            decrypt_chunk(
                &content_key,
                &content_id,
                case.index,
                case.count,
                case.is_last,
                &encrypted
            )
            .unwrap()
            .as_slice(),
            plaintext.as_slice()
        );
    }
}

#[wasm_bindgen_test]
fn recovery_vectors_match_in_wasm() {
    let vectors: RecoveryVectors = serde_json::from_str(RECOVERY_VECTORS).unwrap();

    for case in &vectors.cases {
        let recovery_key = parse_recovery_key(&case.display).unwrap();
        assert_eq!(
            hex_encode(recovery_key.expose_for_test_vectors()),
            case.recovery_key_hex
        );
        assert_eq!(encode_recovery_key(&recovery_key), case.display);

        let keys = derive_recovery_keys(&recovery_key).unwrap();
        assert_eq!(
            hex_encode(keys.rk_wrap.expose_for_test_vectors()),
            case.rk_wrap_hex
        );
        assert_eq!(
            hex_encode(keys.rk_auth.expose_secret()),
            case.rk_auth_hex,
            "case {}",
            case.name
        );
    }
}

// ---------------------------------------------------------------------------
// KeyStore 契约
// ---------------------------------------------------------------------------

#[wasm_bindgen_test]
fn default_kdf_params_work_in_browser() {
    // 每组都显式使用 64 MiB / t=3：低端设备也应能跑通，只是需要几秒。
    let mut store = KeyStore::new();
    let salt = [0x42u8; 16];

    let auth_key = store
        .derive_password_keys("correct horse battery staple", DEFAULT_KDF_PARAMS, &salt)
        .unwrap();
    assert_eq!(auth_key.len(), 32);

    let wrapped_mk = store.create_master_key().unwrap();
    assert_eq!(wrapped_mk.first(), Some(&0x01));
    assert!(!store.is_locked());

    store.lock();
    assert!(store.is_locked());

    // 重新派生后解锁（模拟刷新页面后的本地解锁）。
    let auth_key_again = store
        .derive_password_keys("correct horse battery staple", DEFAULT_KDF_PARAMS, &salt)
        .unwrap();
    assert_eq!(auth_key, auth_key_again);
    store.unlock_with_password(&wrapped_mk).unwrap();
    assert!(!store.is_locked());
}

#[wasm_bindgen_test]
fn key_store_roundtrip_through_high_level_api() {
    let salt = [0x42u8; 16];
    let mut store = KeyStore::new();
    store
        .derive_password_keys("correct horse battery staple", DEFAULT_KDF_PARAMS, &salt)
        .unwrap();
    let wrapped_mk = store.create_master_key().unwrap();
    let node_id = "550e8400-e29b-41d4-a716-446655440000";
    let parent_id = "550e8400-e29b-41d4-a716-44665544000a";
    let version_id = "6ba7b810-9dad-41d1-80b4-00c04fd430c8";
    let content_id = "6ba7b811-9dad-41d1-80b4-00c04fd430c8";

    // 节点密钥：返回的是包裹密文，长度 32 + 41。
    let wrapped = store.generate_node_key(node_id).unwrap();
    assert_eq!(wrapped.len(), 32 + 41);

    // 节点元数据往返。
    let meta = store.encrypt_node_metadata(node_id, "报告.pdf").unwrap();
    assert_eq!(
        store.decrypt_node_metadata(node_id, &meta).unwrap(),
        "报告.pdf"
    );

    // 名称令牌：64 位十六进制、大小写不敏感。
    let token = store.name_token(parent_id, "Report.PDF").unwrap();
    assert_eq!(token.len(), 64);
    assert_eq!(store.name_token(parent_id, "report.pdf").unwrap(), token);

    // 版本元数据：加密后用带 content 校验的入口解密。
    let version_meta = store
        .encrypt_version_metadata(
            node_id,
            version_id,
            content_id,
            10_485_760.0,
            1_700_000_000.0,
            "application/pdf",
        )
        .unwrap();
    let view = store
        .decrypt_version_metadata_for_content(node_id, version_id, content_id, &version_meta)
        .unwrap()
        .to_rust()
        .unwrap();
    assert_eq!(view.content_id, content_id);
    assert_eq!(view.size, 10_485_760.0);
    assert_eq!(view.mtime, 1_700_000_000.0);

    // 指向别的 content 必须失败（回滚防护）。
    let error = store
        .decrypt_version_metadata_for_content(
            node_id,
            version_id,
            "6ba7b811-9dad-41d1-80b4-00c04fd430c9",
            &version_meta,
        )
        .err()
        .unwrap();
    assert_eq!(error_code(&error), "content_binding_mismatch");

    // 分块往返（空块与普通块）。
    let count = 1;
    let ciphertext = store
        .encrypt_chunk(node_id, content_id, 0, count, true, b"hello chunk")
        .unwrap();
    assert_eq!(
        store
            .decrypt_chunk(node_id, content_id, 0, count, true, &ciphertext)
            .unwrap(),
        b"hello chunk"
    );
    let empty = store
        .encrypt_chunk(node_id, content_id, 0, count, true, b"")
        .unwrap();
    assert_eq!(empty.len(), 41);

    // 大小校验。
    assert!(store.verify_plaintext_size(10.0, 10.0).is_ok());
    let error = store.verify_plaintext_size(10.0, 9.0).unwrap_err();
    assert_eq!(error_code(&error), "plaintext_size_mismatch");

    // 刷新后从服务端拿到包裹密钥，用同一把 MK 重新加载节点密钥。
    store.lock();
    let mut reloaded = KeyStore::new();
    reloaded
        .derive_password_keys("correct horse battery staple", DEFAULT_KDF_PARAMS, &salt)
        .unwrap();
    reloaded.unlock_with_password(&wrapped_mk).unwrap();
    reloaded.load_node_key(node_id, &wrapped).unwrap();
    assert_eq!(
        reloaded.decrypt_node_metadata(node_id, &meta).unwrap(),
        "报告.pdf"
    );

    // 恢复密钥：展示串可解析，且不返回恢复密钥本身。
    let recovery = reloaded.create_recovery_key().unwrap().to_rust().unwrap();
    assert!(recovery.display.contains('-'));
    assert_eq!(recovery.rk_auth.len(), 32);
    parse_recovery_key(&recovery.display).unwrap();
}

#[wasm_bindgen_test]
fn lock_releases_keys_and_rejects_operations() {
    let mut store = unlocked_store();
    let node_id = "550e8400-e29b-41d4-a716-446655440000";
    store.generate_node_key(node_id).unwrap();

    store.lock();
    assert!(store.is_locked());

    // 锁定后所有需要密钥的调用都返回 locked。
    let error = store.generate_node_key(node_id).unwrap_err();
    assert_eq!(error_code(&error), "locked");

    let error = store
        .name_token("550e8400-e29b-41d4-a716-44665544000a", "a.txt")
        .unwrap_err();
    assert_eq!(error_code(&error), "locked");

    let error = store
        .encrypt_chunk(
            node_id,
            "6ba7b811-9dad-41d1-80b4-00c04fd430c8",
            0,
            1,
            true,
            b"x",
        )
        .unwrap_err();
    assert_eq!(error_code(&error), "locked");

    // 锁定清空了 pwd_wrap：必须重新派生密码才能解锁。
    let error = store.create_master_key().unwrap_err();
    assert_eq!(error_code(&error), "not_initialized");
}

#[wasm_bindgen_test]
fn wrong_password_returns_invalid_credentials() {
    // 正确密码创建 MK，拿到真实结构的 wrapped_mk。
    let mut owner = KeyStore::new();
    let salt = [0x42u8; 16];
    owner
        .derive_password_keys("correct horse battery staple", DEFAULT_KDF_PARAMS, &salt)
        .unwrap();
    let wrapped_mk = owner.create_master_key().unwrap();

    // 错误密码派生出的 pwd_wrap 无法解包：只有这里应返回 invalid_credentials。
    let mut attacker = KeyStore::new();
    attacker
        .derive_password_keys("wrong password", DEFAULT_KDF_PARAMS, &salt)
        .unwrap();
    let error = attacker.unlock_with_password(&wrapped_mk).unwrap_err();
    assert_eq!(error_code(&error), "invalid_credentials");
}

#[wasm_bindgen_test]
fn tampered_chunk_returns_integrity_error() {
    let mut store = unlocked_store();
    store.generate_node_key(NODE_ID).unwrap();

    let mut ciphertext = store
        .encrypt_chunk(NODE_ID, CONTENT_ID, 0, 1, true, b"chunk data")
        .unwrap();
    flip_last_byte(&mut ciphertext);

    let error = store
        .decrypt_chunk(NODE_ID, CONTENT_ID, 0, 1, true, &ciphertext)
        .unwrap_err();
    assert_eq!(error_code(&error), "integrity_error");
}

#[wasm_bindgen_test]
fn tampered_metadata_returns_integrity_error() {
    let mut store = unlocked_store();
    store.generate_node_key(NODE_ID).unwrap();

    // 节点元数据。
    let mut node_metadata = store.encrypt_node_metadata(NODE_ID, "报告.pdf").unwrap();
    flip_last_byte(&mut node_metadata);
    let error = store
        .decrypt_node_metadata(NODE_ID, &node_metadata)
        .unwrap_err();
    assert_eq!(error_code(&error), "integrity_error");

    // 版本元数据。
    let mut version_metadata = store
        .encrypt_version_metadata(
            NODE_ID,
            VERSION_ID,
            CONTENT_ID,
            10.0,
            1_700_000_000.0,
            "text/plain",
        )
        .unwrap();
    flip_last_byte(&mut version_metadata);
    let error = store
        .decrypt_version_metadata_for_content(NODE_ID, VERSION_ID, CONTENT_ID, &version_metadata)
        .err()
        .unwrap();
    assert_eq!(error_code(&error), "integrity_error");
}

#[wasm_bindgen_test]
fn swapped_node_key_returns_integrity_error() {
    let mut store = unlocked_store();

    // A 的包裹密钥带着 node_id 的 AAD；把它当成 B 的密钥解包必须失败。
    let wrapped_a = store.generate_node_key(NODE_ID).unwrap();
    let error = store.load_node_key(OTHER_NODE_ID, &wrapped_a).unwrap_err();
    assert_eq!(error_code(&error), "integrity_error");
}

#[wasm_bindgen_test]
fn wrong_recovery_key_returns_invalid_recovery_key() {
    let mut owner = unlocked_store();
    let material = owner.create_recovery_key().unwrap().to_rust().unwrap();

    // 正确的恢复密钥可以解锁。
    let mut unlocked = KeyStore::new();
    unlocked
        .unlock_with_recovery(&material.display, &material.wrapped_mk_recovery)
        .unwrap();
    assert!(!unlocked.is_locked());

    // 另一把格式合法的恢复密钥无法解包 → invalid_recovery_key。
    let mut other = unlocked_store();
    let other_material = other.create_recovery_key().unwrap().to_rust().unwrap();
    let mut store = KeyStore::new();
    let error = store
        .unlock_with_recovery(&other_material.display, &material.wrapped_mk_recovery)
        .unwrap_err();
    assert_eq!(error_code(&error), "invalid_recovery_key");

    // 抄写错误（编码非法）同样映射为 invalid_recovery_key。
    let mut store = KeyStore::new();
    let error = store
        .unlock_with_recovery("not-a-recovery-key", &material.wrapped_mk_recovery)
        .unwrap_err();
    assert_eq!(error_code(&error), "invalid_recovery_key");
}

// ---------------------------------------------------------------------------
// 测试辅助
// ---------------------------------------------------------------------------

const NODE_ID: &str = "550e8400-e29b-41d4-a716-446655440000";
const OTHER_NODE_ID: &str = "550e8400-e29b-41d4-a716-446655440001";
const VERSION_ID: &str = "6ba7b810-9dad-41d1-80b4-00c04fd430c8";
const CONTENT_ID: &str = "6ba7b811-9dad-41d1-80b4-00c04fd430c8";

fn flip_last_byte(bytes: &mut [u8]) {
    let last = bytes.len() - 1;
    bytes[last] ^= 0x01;
}

fn unlocked_store() -> KeyStore {
    let mut store = KeyStore::new();
    let salt = [0x42u8; 16];
    store
        .derive_password_keys("correct horse battery staple", DEFAULT_KDF_PARAMS, &salt)
        .unwrap();
    store.create_master_key().unwrap();
    store
}
