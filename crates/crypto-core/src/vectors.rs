//! 跨实现测试向量（design D5 / D16）。
//!
//! 向量文件位于仓库根目录 `testdata/crypto-vectors/`，用 JSON 描述固定输入与期望输出。
//! 浏览器 WASM、Rust 桌面端/CLI 都必须能通过同一组向量。
//!
//! 重新生成（仅在确认了解影响时执行，并在 PR 中说明原因）：
//!
//! ```text
//! cargo test -p crypto-core -- --ignored regenerate_vectors
//! ```

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::encoding::{decode_hex, encode_hex};
use crate::{
    ContentId, CryptoError, KdfParams, MasterKey, NameToken, NodeId, NodeKey, NodeMetadata,
    RecoveryKey, VersionId, VersionMetadata, aad, aead, compute_name_token, decrypt_chunk,
    decrypt_node_metadata, decrypt_version_metadata_for_content, derive_content_key,
    derive_keys_from_password, derive_name_key, derive_password_keys, derive_recovery_keys,
    derive_root_key, encode_recovery_key, parse_recovery_key, unwrap_node_key,
    verify_plaintext_size,
};

const AEAD_FILE: &str = "aead.json";
const KDF_FILE: &str = "kdf.json";
const NODE_FILE: &str = "node.json";
const CHUNK_FILE: &str = "chunk.json";
const RECOVERY_FILE: &str = "recovery.json";

fn vectors_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/crypto-vectors")
}

type AeadInput<'a> = (&'a str, [u8; 24], &'a [u8], &'a [u8]);
type ChunkInput<'a> = (&'a str, u32, u32, bool, [u8; 24], &'a [u8]);

fn read_vectors<T: for<'de> Deserialize<'de>>(file: &str) -> T {
    let path = vectors_dir().join(file);
    let raw = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("读取测试向量 {} 失败：{error}", path.display()));
    serde_json::from_str(&raw)
        .unwrap_or_else(|error| panic!("解析测试向量 {} 失败：{error}", path.display()))
}

fn write_vectors<T: Serialize>(file: &str, vectors: &T) {
    let dir = vectors_dir();
    fs::create_dir_all(&dir).unwrap();
    let mut serialized = serde_json::to_string_pretty(vectors).unwrap();
    serialized.push('\n');
    fs::write(dir.join(file), serialized).unwrap();
}

fn bytes_by_hex(hex: &str) -> Vec<u8> {
    decode_hex(hex).unwrap_or_else(|| panic!("非法十六进制：{hex}"))
}

fn key_by_hex<const N: usize>(hex: &str) -> [u8; N] {
    bytes_by_hex(hex).try_into().unwrap()
}

// ---------------------------------------------------------------------------
// AEAD 向量
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize)]
struct AeadVectors {
    format: String,
    format_version: u8,
    cases: Vec<AeadCase>,
}

#[derive(Serialize, Deserialize)]
struct AeadCase {
    name: String,
    key_hex: String,
    nonce_hex: String,
    aad_hex: String,
    plaintext_hex: String,
    frame_hex: String,
}

fn build_aead_vectors() -> AeadVectors {
    let key = [0x11u8; 32];
    let mut cases = Vec::new();

    let inputs: [AeadInput<'_>; 4] = [
        ("empty-plaintext", [0x21u8; 24], b"bk/v1/mk", b""),
        (
            "ascii",
            [0x22u8; 24],
            b"bk/v1/mk",
            b"hello, encrypted world",
        ),
        (
            "unicode-aad",
            [0x23u8; 24],
            "bk/v1/node-meta|550e8400-e29b-41d4-a716-446655440000".as_bytes(),
            "报告.pdf".as_bytes(),
        ),
        (
            "empty-aad",
            [0x24u8; 24],
            b"",
            b"0123456789abcdef0123456789abcdef",
        ),
    ];

    for (name, nonce, aad_bytes, plaintext) in inputs {
        let frame = aead::seal_with_nonce(&key, &nonce, aad_bytes, plaintext).unwrap();
        cases.push(AeadCase {
            name: name.to_owned(),
            key_hex: encode_hex(&key),
            nonce_hex: encode_hex(&nonce),
            aad_hex: encode_hex(aad_bytes),
            plaintext_hex: encode_hex(plaintext),
            frame_hex: encode_hex(&frame),
        });
    }

    AeadVectors {
        format: "bk-crypto-vectors/aead@1".to_owned(),
        format_version: crate::CIPHERTEXT_FORMAT_VERSION,
        cases,
    }
}

#[test]
fn aead_vectors_match() {
    let vectors: AeadVectors = read_vectors(AEAD_FILE);
    assert_eq!(vectors.format, "bk-crypto-vectors/aead@1");
    assert_eq!(vectors.format_version, crate::CIPHERTEXT_FORMAT_VERSION);
    assert!(!vectors.cases.is_empty());

    for case in &vectors.cases {
        let key: [u8; 32] = key_by_hex(&case.key_hex);
        let nonce: [u8; 24] = key_by_hex(&case.nonce_hex);
        let aad_bytes = bytes_by_hex(&case.aad_hex);
        let plaintext = bytes_by_hex(&case.plaintext_hex);
        let expected_frame = bytes_by_hex(&case.frame_hex);

        let frame = aead::seal_with_nonce(&key, &nonce, &aad_bytes, &plaintext).unwrap();
        assert_eq!(frame, expected_frame, "case {}", case.name);
        assert_eq!(frame[0], crate::CIPHERTEXT_FORMAT_VERSION);
        assert_eq!(
            aead::open(&key, &aad_bytes, &frame).unwrap(),
            plaintext,
            "case {}",
            case.name
        );
    }
}

// ---------------------------------------------------------------------------
// KDF 向量
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize)]
struct KdfVectors {
    format: String,
    cases: Vec<KdfCase>,
}

#[derive(Serialize, Deserialize)]
struct KdfCase {
    name: String,
    password: String,
    salt_hex: String,
    params: KdfParams,
    root_hex: String,
    pwd_wrap_hex: String,
    auth_key_hex: String,
}

fn build_kdf_vectors() -> KdfVectors {
    let cases = [
        (
            "default-params-ascii",
            "correct horse battery staple",
            [0x31u8; 16],
            KdfParams::RECOMMENDED,
        ),
        (
            // 全角字符：验证实现先做 NFKC 规范化。
            "nfkc-fullwidth",
            "ｐａｓｓｗｏｒｄ１２３",
            [0x32u8; 16],
            // 参数必须在生产允许范围内（64–256 MiB、t 3–8、p=1）。
            KdfParams::new(128 * 1024, 3, 1).unwrap(),
        ),
    ];

    let mut vectors = Vec::new();
    for (name, password, salt, params) in cases {
        let root = derive_root_key(password, &salt, &params).unwrap();
        let keys = derive_password_keys(&root).unwrap();
        vectors.push(KdfCase {
            name: name.to_owned(),
            password: password.to_owned(),
            salt_hex: encode_hex(&salt),
            params,
            root_hex: encode_hex(root.as_bytes()),
            pwd_wrap_hex: encode_hex(keys.pwd_wrap.as_bytes()),
            auth_key_hex: encode_hex(keys.auth_key.as_bytes()),
        });
    }

    KdfVectors {
        format: "bk-crypto-vectors/kdf@1".to_owned(),
        cases: vectors,
    }
}

#[test]
fn kdf_vectors_match() {
    let vectors: KdfVectors = read_vectors(KDF_FILE);
    assert_eq!(vectors.format, "bk-crypto-vectors/kdf@1");

    for case in &vectors.cases {
        let salt: [u8; 16] = key_by_hex(&case.salt_hex);
        let root = derive_root_key(&case.password, &salt, &case.params).unwrap();
        assert_eq!(
            encode_hex(root.as_bytes()),
            case.root_hex,
            "case {}",
            case.name
        );

        let keys = derive_password_keys(&root).unwrap();
        assert_eq!(
            encode_hex(keys.pwd_wrap.as_bytes()),
            case.pwd_wrap_hex,
            "case {}",
            case.name
        );
        assert_eq!(
            encode_hex(keys.auth_key.expose_secret()),
            case.auth_key_hex,
            "case {}",
            case.name
        );

        // 便捷入口必须与两步派生一致。
        let direct = derive_keys_from_password(&case.password, &salt, &case.params).unwrap();
        assert_eq!(encode_hex(direct.pwd_wrap.as_bytes()), case.pwd_wrap_hex);
    }
}

// ---------------------------------------------------------------------------
// 节点 / 元数据 / 名称令牌向量
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize)]
struct NodeVectors {
    format: String,
    cases: Vec<NodeCase>,
}

#[derive(Serialize, Deserialize)]
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
    node_meta_aad_hex: String,
    node_meta_frame_hex: String,
    version_meta_plaintext: String,
    version_meta_aad_hex: String,
    version_meta_frame_hex: String,
    file_name: String,
    name_token_hex: String,
}

const NODE_ID: &str = "550e8400-e29b-41d4-a716-446655440000";
const PARENT_ID: &str = "550e8400-e29b-41d4-a716-44665544000a";
const VERSION_ID: &str = "6ba7b810-9dad-41d1-80b4-00c04fd430c8";
const CONTENT_ID: &str = "6ba7b811-9dad-41d1-80b4-00c04fd430c8";

fn build_node_vectors() -> NodeVectors {
    let node_id = NodeId::parse(NODE_ID).unwrap();
    let parent_id = NodeId::parse(PARENT_ID).unwrap();
    let version_id = VersionId::parse(VERSION_ID).unwrap();
    let content_id = ContentId::parse(CONTENT_ID).unwrap();

    let master_key = MasterKey::from_bytes([0x41u8; 32]);
    let node_key = NodeKey::from_bytes([0x42u8; 32]);
    // 含大写、土耳其语 İ 与中文：覆盖 NFC(lower(NFC(name))) 的大小写折叠路径。
    let file_name = "İstanbul 报告 2026.PDF".to_owned();

    let wrap_nonce = [0x51u8; 24];
    let node_meta_nonce = [0x52u8; 24];
    let version_meta_nonce = [0x53u8; 24];

    let wrapped_node_key = aead::seal_with_nonce(
        master_key.as_bytes(),
        &wrap_nonce,
        &aad::node_key(&node_id),
        node_key.as_bytes(),
    )
    .unwrap();

    let node_metadata = NodeMetadata {
        name: file_name.clone(),
    };
    let node_meta_plaintext = serde_json::to_vec(&node_metadata).unwrap();
    let node_meta_frame = aead::seal_with_nonce(
        node_key.as_bytes(),
        &node_meta_nonce,
        &aad::node_metadata(&node_id),
        &node_meta_plaintext,
    )
    .unwrap();

    let version_metadata = VersionMetadata {
        content_id: content_id.clone(),
        size: 10 * 1024 * 1024,
        mtime: 1_700_000_000,
        mime: "application/pdf".to_owned(),
        content_sha256: None,
    };
    let version_meta_plaintext = serde_json::to_vec(&version_metadata).unwrap();
    let version_meta_frame = aead::seal_with_nonce(
        node_key.as_bytes(),
        &version_meta_nonce,
        &aad::version_metadata(&version_id),
        &version_meta_plaintext,
    )
    .unwrap();

    let name_key = derive_name_key(&master_key).unwrap();
    let name_token = compute_name_token(&name_key, &parent_id, &file_name).unwrap();

    NodeVectors {
        format: "bk-crypto-vectors/node@1".to_owned(),
        cases: vec![NodeCase {
            name: "unicode-name".to_owned(),
            node_id: node_id.as_str().to_owned(),
            parent_id: parent_id.as_str().to_owned(),
            version_id: version_id.as_str().to_owned(),
            content_id: content_id.as_str().to_owned(),
            master_key_hex: encode_hex(master_key.as_bytes()),
            node_key_hex: encode_hex(node_key.as_bytes()),
            wrapped_node_key_hex: encode_hex(&wrapped_node_key),
            node_meta_plaintext: String::from_utf8(node_meta_plaintext).unwrap(),
            node_meta_aad_hex: encode_hex(&aad::node_metadata(&node_id)),
            node_meta_frame_hex: encode_hex(&node_meta_frame),
            version_meta_plaintext: String::from_utf8(version_meta_plaintext).unwrap(),
            version_meta_aad_hex: encode_hex(&aad::version_metadata(&version_id)),
            version_meta_frame_hex: encode_hex(&version_meta_frame),
            file_name,
            name_token_hex: name_token.to_hex(),
        }],
    }
}

#[test]
fn node_vectors_match() {
    let vectors: NodeVectors = read_vectors(NODE_FILE);
    assert_eq!(vectors.format, "bk-crypto-vectors/node@1");

    for case in &vectors.cases {
        let node_id = NodeId::parse(&case.node_id).unwrap();
        let parent_id = NodeId::parse(&case.parent_id).unwrap();
        let version_id = VersionId::parse(&case.version_id).unwrap();
        let master_key = MasterKey::from_bytes(key_by_hex(&case.master_key_hex));
        let node_key = NodeKey::from_bytes(key_by_hex(&case.node_key_hex));

        // 节点密钥包裹：AAD 绑定 node_id。
        let wrapped = bytes_by_hex(&case.wrapped_node_key_hex);
        assert_eq!(
            unwrap_node_key(&master_key, &node_id, &wrapped).unwrap(),
            node_key,
            "case {}",
            case.name
        );

        // 节点元数据。
        assert_eq!(
            encode_hex(&aad::node_metadata(&node_id)),
            case.node_meta_aad_hex
        );
        let node_meta_frame = bytes_by_hex(&case.node_meta_frame_hex);
        let metadata = decrypt_node_metadata(&node_key, &node_id, &node_meta_frame).unwrap();
        assert_eq!(metadata.name, case.file_name);
        assert_eq!(
            String::from_utf8(
                aead::open(
                    node_key.as_bytes(),
                    &aad::node_metadata(&node_id),
                    &node_meta_frame
                )
                .unwrap()
            )
            .unwrap(),
            case.node_meta_plaintext
        );

        // 版本元数据：必须绑定 content_id，并提供大小校验。
        assert_eq!(
            encode_hex(&aad::version_metadata(&version_id)),
            case.version_meta_aad_hex
        );
        let content_id = ContentId::parse(&case.content_id).unwrap();
        let version_meta_frame = bytes_by_hex(&case.version_meta_frame_hex);
        let version_metadata = decrypt_version_metadata_for_content(
            &node_key,
            &version_id,
            &content_id,
            &version_meta_frame,
        )
        .unwrap();
        assert_eq!(version_metadata.content_id, content_id);
        assert_eq!(version_metadata.size, 10 * 1024 * 1024);
        assert_eq!(version_metadata.mtime, 1_700_000_000);
        assert_eq!(version_metadata.mime, "application/pdf");
        assert_eq!(
            verify_plaintext_size(&version_metadata, 10 * 1024 * 1024),
            Ok(())
        );
        assert_eq!(
            verify_plaintext_size(&version_metadata, 10 * 1024 * 1024 - 1),
            Err(CryptoError::PlaintextSizeMismatch)
        );

        // 指向另一个 content 时必须被拒绝（回滚攻击）。
        let other_content = ContentId::parse("6ba7b811-9dad-41d1-80b4-00c04fd430c9").unwrap();
        assert_eq!(
            decrypt_version_metadata_for_content(
                &node_key,
                &version_id,
                &other_content,
                &version_meta_frame
            ),
            Err(CryptoError::ContentBindingMismatch)
        );

        assert_eq!(
            String::from_utf8(
                aead::open(
                    node_key.as_bytes(),
                    &aad::version_metadata(&version_id),
                    &version_meta_frame
                )
                .unwrap()
            )
            .unwrap(),
            case.version_meta_plaintext
        );

        // 名称令牌。
        let name_key = derive_name_key(&master_key).unwrap();
        let token = compute_name_token(&name_key, &parent_id, &case.file_name).unwrap();
        assert_eq!(token.to_hex(), case.name_token_hex);
        assert_eq!(NameToken::from_hex(&case.name_token_hex).unwrap(), token);
    }
}

// ---------------------------------------------------------------------------
// 分块向量
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize)]
struct ChunkVectors {
    format: String,
    cases: Vec<ChunkCase>,
}

#[derive(Serialize, Deserialize)]
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

fn build_chunk_vectors() -> ChunkVectors {
    let content_id = ContentId::parse(CONTENT_ID).unwrap();
    let node_key = NodeKey::from_bytes([0x61u8; 32]);
    let content_key = derive_content_key(&node_key, &content_id).unwrap();

    let cases: [ChunkInput<'_>; 3] = [
        ("empty-file-single-chunk", 0, 1, true, [0x71u8; 24], b""),
        (
            "single-small-chunk",
            0,
            1,
            true,
            [0x72u8; 24],
            b"hello chunk",
        ),
        ("last-of-two", 1, 2, true, [0x73u8; 24], b"tail bytes"),
    ];

    let mut vectors = Vec::new();
    for (name, index, count, is_last, nonce, plaintext) in cases {
        let aad_bytes = aad::chunk(&content_id, index, count, is_last);
        let frame =
            aead::seal_with_nonce(content_key.as_bytes(), &nonce, &aad_bytes, plaintext).unwrap();
        vectors.push(ChunkCase {
            name: name.to_owned(),
            content_id: content_id.as_str().to_owned(),
            node_key_hex: encode_hex(node_key.as_bytes()),
            content_key_hex: encode_hex(content_key.as_bytes()),
            index,
            count,
            is_last,
            aad_hex: encode_hex(&aad_bytes),
            nonce_hex: encode_hex(&nonce),
            plaintext_hex: encode_hex(plaintext),
            frame_hex: encode_hex(&frame),
        });
    }

    ChunkVectors {
        format: "bk-crypto-vectors/chunk@1".to_owned(),
        cases: vectors,
    }
}

#[test]
fn chunk_vectors_match() {
    let vectors: ChunkVectors = read_vectors(CHUNK_FILE);
    assert_eq!(vectors.format, "bk-crypto-vectors/chunk@1");

    for case in &vectors.cases {
        let content_id = ContentId::parse(&case.content_id).unwrap();
        let node_key = NodeKey::from_bytes(key_by_hex(&case.node_key_hex));
        let content_key = derive_content_key(&node_key, &content_id).unwrap();
        assert_eq!(
            encode_hex(content_key.as_bytes()),
            case.content_key_hex,
            "case {}",
            case.name
        );

        assert_eq!(
            encode_hex(&aad::chunk(
                &content_id,
                case.index,
                case.count,
                case.is_last
            )),
            case.aad_hex,
            "case {}",
            case.name
        );

        let frame = bytes_by_hex(&case.frame_hex);
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
            bytes_by_hex(&case.plaintext_hex),
            "case {}",
            case.name
        );

        // 确定性加密结果必须与向量一致。
        let nonce: [u8; 24] = key_by_hex(&case.nonce_hex);
        assert_eq!(
            aead::seal_with_nonce(
                content_key.as_bytes(),
                &nonce,
                &aad::chunk(&content_id, case.index, case.count, case.is_last),
                plaintext.as_slice()
            )
            .unwrap(),
            frame,
            "case {}",
            case.name
        );
    }
}

// ---------------------------------------------------------------------------
// 恢复密钥向量
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize)]
struct RecoveryVectors {
    format: String,
    cases: Vec<RecoveryCase>,
}

#[derive(Serialize, Deserialize)]
struct RecoveryCase {
    name: String,
    recovery_key_hex: String,
    display: String,
    rk_wrap_hex: String,
    rk_auth_hex: String,
}

fn build_recovery_vectors() -> RecoveryVectors {
    let keys = [
        (
            "sequential-bytes",
            RecoveryKey::from_bytes([
                0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22,
                23, 24, 25, 26, 27, 28, 29, 30, 31,
            ]),
        ),
        ("repeated-bytes", RecoveryKey::from_bytes([0x7fu8; 32])),
    ];

    let mut cases = Vec::new();
    for (name, recovery_key) in keys {
        let derived = derive_recovery_keys(&recovery_key).unwrap();
        cases.push(RecoveryCase {
            name: name.to_owned(),
            recovery_key_hex: encode_hex(recovery_key.as_bytes()),
            display: encode_recovery_key(&recovery_key),
            rk_wrap_hex: encode_hex(derived.rk_wrap.as_bytes()),
            rk_auth_hex: encode_hex(derived.rk_auth.as_bytes()),
        });
    }

    RecoveryVectors {
        format: "bk-crypto-vectors/recovery@1".to_owned(),
        cases,
    }
}

#[test]
fn recovery_vectors_match() {
    let vectors: RecoveryVectors = read_vectors(RECOVERY_FILE);
    assert_eq!(vectors.format, "bk-crypto-vectors/recovery@1");

    for case in &vectors.cases {
        let recovery_key = parse_recovery_key(&case.display).unwrap();
        assert_eq!(
            encode_hex(recovery_key.as_bytes()),
            case.recovery_key_hex,
            "case {}",
            case.name
        );
        assert_eq!(
            encode_recovery_key(&recovery_key),
            case.display,
            "展示编码必须稳定（case {}）",
            case.name
        );

        let derived = derive_recovery_keys(&recovery_key).unwrap();
        assert_eq!(encode_hex(derived.rk_wrap.as_bytes()), case.rk_wrap_hex);
        assert_eq!(
            encode_hex(derived.rk_auth.expose_secret()),
            case.rk_auth_hex
        );
    }
}

// ---------------------------------------------------------------------------
// 向量再生成（默认不跑）
// ---------------------------------------------------------------------------

#[test]
#[ignore = "重新生成 testdata/crypto-vectors 下的已提交向量"]
fn regenerate_vectors() {
    write_vectors(AEAD_FILE, &build_aead_vectors());
    write_vectors(KDF_FILE, &build_kdf_vectors());
    write_vectors(NODE_FILE, &build_node_vectors());
    write_vectors(CHUNK_FILE, &build_chunk_vectors());
    write_vectors(RECOVERY_FILE, &build_recovery_vectors());

    // 重新生成后立即自检，避免把不一致的向量提交进仓库。
    aead_vectors_match();
    kdf_vectors_match();
    node_vectors_match();
    chunk_vectors_match();
    recovery_vectors_match();
}
