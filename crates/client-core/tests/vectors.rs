//! 跨实现测试向量（不需要 Docker）。
//!
//! 直接读取仓库根目录 `testdata/crypto-vectors/` 的 Web/WASM 固定向量，
//! 用 `crypto-core` 的公开 API 解密，确保客户端与 Web 使用同一份格式。

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::fs;
use std::path::PathBuf;

use crypto_core::{
    ContentId, MasterKey, NodeId, NodeKey, VersionId, compute_name_token, decrypt_chunk,
    decrypt_node_metadata, decrypt_version_metadata_for_content, derive_content_key,
    derive_name_key, unwrap_node_key,
};
use serde_json::Value;

fn vector(name: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/crypto-vectors")
        .join(name);
    serde_json::from_str(&fs::read_to_string(path).expect("读取测试向量")).expect("解析测试向量")
}

fn hex(value: &str) -> Vec<u8> {
    assert_eq!(value.len() % 2, 0);
    let mut output = Vec::with_capacity(value.len() / 2);
    let (pairs, _) = value.as_bytes().as_chunks::<2>();
    for pair in pairs {
        let text = std::str::from_utf8(pair).expect("十六进制字符");
        output.push(u8::from_str_radix(text, 16).expect("十六进制数字"));
    }
    output
}

fn hex_array_32(value: &str) -> [u8; 32] {
    hex(value).try_into().expect("32 字节")
}

#[test]
fn chunk_vectors_decrypt_to_expected_plaintext() {
    let value = vector("chunk.json");
    for case in value["cases"].as_array().expect("cases") {
        let content_id = case["content_id"].as_str().expect("content_id");
        let content = ContentId::parse(content_id).expect("content id");
        let node_key =
            NodeKey::try_from(hex_array_32(case["node_key_hex"].as_str().unwrap()).as_slice())
                .expect("node key");
        let content_key = derive_content_key(&node_key, &content).expect("content key");
        let index = case["index"].as_u64().unwrap() as u32;
        let count = case["count"].as_u64().unwrap() as u32;
        let is_last = case["is_last"].as_bool().unwrap();
        let frame = hex(case["frame_hex"].as_str().unwrap());
        let plaintext = decrypt_chunk(&content_key, &content, index, count, is_last, &frame)
            .expect("向量必须能解密");
        assert_eq!(
            plaintext.as_slice(),
            hex(case["plaintext_hex"].as_str().unwrap()).as_slice(),
            "{}",
            case["name"]
        );
    }
}

#[test]
fn node_vectors_decrypt_names_metadata_and_name_tokens() {
    let value = vector("node.json");
    for case in value["cases"].as_array().expect("cases") {
        let master = MasterKey::from_bytes(hex_array_32(case["master_key_hex"].as_str().unwrap()));
        let node_id = NodeId::parse(case["node_id"].as_str().unwrap()).unwrap();
        let version_id = VersionId::parse(case["version_id"].as_str().unwrap()).unwrap();
        let content_id = ContentId::parse(case["content_id"].as_str().unwrap()).unwrap();

        let wrapped = hex(case["wrapped_node_key_hex"].as_str().unwrap());
        let node_key = unwrap_node_key(&master, &node_id, &wrapped).expect("节点密钥必须能解包");

        let node_meta = decrypt_node_metadata(
            &node_key,
            &node_id,
            &hex(case["node_meta_frame_hex"].as_str().unwrap()),
        )
        .expect("节点元数据必须能解密");
        assert_eq!(node_meta.name, case["file_name"].as_str().unwrap());

        let version_meta = decrypt_version_metadata_for_content(
            &node_key,
            &version_id,
            &content_id,
            &hex(case["version_meta_frame_hex"].as_str().unwrap()),
        )
        .expect("版本元数据必须通过 content 绑定校验");
        assert_eq!(version_meta.content_id, content_id);
        assert_eq!(version_meta.content_sha256, None);

        let name_key = derive_name_key(&master).unwrap();
        let parent = NodeId::parse(case["parent_id"].as_str().unwrap()).unwrap();
        let token = compute_name_token(&name_key, &parent, &node_meta.name).unwrap();
        assert_eq!(token.to_hex(), case["name_token_hex"].as_str().unwrap());
    }
}

#[test]
fn aead_vectors_are_compatible() {
    let value = vector("aead.json");
    for case in value["cases"].as_array().expect("cases") {
        let key = hex_array_32(case["key_hex"].as_str().unwrap());
        let aad = hex(case["aad_hex"].as_str().unwrap());
        let frame = hex(case["frame_hex"].as_str().unwrap());
        let plaintext = crypto_core::aead::open(&key, &aad, &frame).expect("AEAD 向量");
        assert_eq!(
            plaintext,
            hex(case["plaintext_hex"].as_str().unwrap()),
            "{}",
            case["name"]
        );
    }
}
