//! 节点密钥、节点/版本元数据与名称令牌（design D5 / D6）。
//!
//! - 节点密钥由 MK 包裹，AAD 绑定 node_id，服务端无法把 A 节点的包裹密钥挪用到 B 节点。
//! - 节点元数据（文件名）与版本元数据（大小/mtime/MIME）用节点密钥加密，AAD 分别绑定
//!   node_id / version_id。
//! - `name_token = HMAC-SHA256(name_key, parent_id ‖ lower(NFC(name)))`：
//!   服务端只比较令牌，从而在不知道名称的前提下保证同一目录内不重名。

use std::fmt;

use hmac::{Hmac, Mac};
use sha2::Sha256;
use unicode_normalization::UnicodeNormalization;
use zeroize::Zeroizing;

use crate::domain::HKDF_INFO_NAME_TOKEN;
use crate::encoding::{decode_hex, encode_hex};
use crate::error::CryptoError;
use crate::ids::{NodeId, VersionId};
use crate::kdf::hkdf_expand;
use crate::keys::{MasterKey, NameKey, NodeKey};
use crate::{aad, aead};

/// 名称令牌长度（HMAC-SHA256 输出）。
pub const NAME_TOKEN_LEN: usize = 32;

/// 节点元数据（加密后存储）。
#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NodeMetadata {
    /// 文件/文件夹名（明文只在客户端内存与密文中出现）。
    pub name: String,
}

impl fmt::Debug for NodeMetadata {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // 名称属于加密元数据，禁止出现在日志里。
        f.write_str("NodeMetadata { name: <redacted> }")
    }
}

/// 版本元数据（加密后存储）。
#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct VersionMetadata {
    /// 明文大小（字节）。
    pub size: u64,
    /// 原始修改时间，Unix 秒。
    pub mtime: i64,
    /// MIME 类型，未知时为空字符串。
    pub mime: String,
}

impl fmt::Debug for VersionMetadata {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "VersionMetadata {{ size: {}, mtime: {}, mime: <redacted> }}",
            self.size, self.mtime
        )
    }
}

/// 名称令牌：服务端只保存/比较这个值。
#[derive(Clone, PartialEq, Eq)]
pub struct NameToken([u8; NAME_TOKEN_LEN]);

impl NameToken {
    /// 令牌字节长度。
    pub const LEN: usize = NAME_TOKEN_LEN;

    /// 原始字节（用于写入数据库或测试向量比对）。
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; NAME_TOKEN_LEN] {
        &self.0
    }

    /// 小写十六进制表示（用于 API 传输）。
    #[must_use]
    pub fn to_hex(&self) -> String {
        encode_hex(&self.0)
    }

    /// 解析十六进制表示。
    pub fn from_hex(input: &str) -> Result<Self, CryptoError> {
        let bytes = decode_hex(input).ok_or(CryptoError::InvalidNameToken)?;
        let bytes: [u8; NAME_TOKEN_LEN] = bytes
            .try_into()
            .map_err(|_| CryptoError::InvalidNameToken)?;
        Ok(Self(bytes))
    }
}

impl fmt::Debug for NameToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("NameToken(<redacted>)")
    }
}

/// 由 MK 派生名称令牌密钥。
pub fn derive_name_key(master_key: &MasterKey) -> Result<NameKey, CryptoError> {
    Ok(NameKey::from_bytes(hkdf_expand(
        master_key.as_bytes(),
        None,
        HKDF_INFO_NAME_TOKEN,
    )?))
}

/// 计算名称令牌。
///
/// 名称先做 Unicode NFC 规范化再转小写，因此同一目录下大小写不同或 NFC/NFD
/// 不同但语义相同的名称会得到相同令牌。
pub fn compute_name_token(
    name_key: &NameKey,
    parent_id: &NodeId,
    name: &str,
) -> Result<NameToken, CryptoError> {
    let normalized = normalize_name(name);

    let mut mac = Hmac::<Sha256>::new_from_slice(name_key.as_bytes())
        .map_err(|_| CryptoError::InvalidKeyLength)?;
    mac.update(parent_id.as_bytes());
    mac.update(normalized.as_bytes());
    let digest = mac.finalize().into_bytes();

    let mut token = [0u8; NAME_TOKEN_LEN];
    token.copy_from_slice(&digest);
    Ok(NameToken(token))
}

/// 名称规范化：NFC 之后再转小写（design D6 的 `lower(NFC(name))`）。
fn normalize_name(name: &str) -> String {
    name.nfc().collect::<String>().to_lowercase()
}

/// 用 MK 包裹节点密钥，AAD 绑定 node_id。
pub fn wrap_node_key(
    master_key: &MasterKey,
    node_id: &NodeId,
    node_key: &NodeKey,
) -> Result<Vec<u8>, CryptoError> {
    aead::seal_random(
        master_key.as_bytes(),
        &aad::node_key(node_id),
        node_key.as_bytes(),
    )
}

/// 解包节点密钥；node_id 不匹配时认证失败。
pub fn unwrap_node_key(
    master_key: &MasterKey,
    node_id: &NodeId,
    wrapped: &[u8],
) -> Result<NodeKey, CryptoError> {
    let plaintext = Zeroizing::new(aead::open(
        master_key.as_bytes(),
        &aad::node_key(node_id),
        wrapped,
    )?);
    NodeKey::try_from(plaintext.as_slice())
}

/// 加密节点元数据。
pub fn encrypt_node_metadata(
    node_key: &NodeKey,
    node_id: &NodeId,
    metadata: &NodeMetadata,
) -> Result<Vec<u8>, CryptoError> {
    let plaintext =
        Zeroizing::new(serde_json::to_vec(metadata).map_err(|_| CryptoError::MetadataEncoding)?);
    aead::seal_random(
        node_key.as_bytes(),
        &aad::node_metadata(node_id),
        &plaintext,
    )
}

/// 解密节点元数据。
pub fn decrypt_node_metadata(
    node_key: &NodeKey,
    node_id: &NodeId,
    ciphertext: &[u8],
) -> Result<NodeMetadata, CryptoError> {
    let plaintext = Zeroizing::new(aead::open(
        node_key.as_bytes(),
        &aad::node_metadata(node_id),
        ciphertext,
    )?);
    serde_json::from_slice(&plaintext).map_err(|_| CryptoError::MetadataEncoding)
}

/// 加密版本元数据。
pub fn encrypt_version_metadata(
    node_key: &NodeKey,
    version_id: &VersionId,
    metadata: &VersionMetadata,
) -> Result<Vec<u8>, CryptoError> {
    let plaintext =
        Zeroizing::new(serde_json::to_vec(metadata).map_err(|_| CryptoError::MetadataEncoding)?);
    aead::seal_random(
        node_key.as_bytes(),
        &aad::version_metadata(version_id),
        &plaintext,
    )
}

/// 解密版本元数据。
pub fn decrypt_version_metadata(
    node_key: &NodeKey,
    version_id: &VersionId,
    ciphertext: &[u8],
) -> Result<VersionMetadata, CryptoError> {
    let plaintext = Zeroizing::new(aead::open(
        node_key.as_bytes(),
        &aad::version_metadata(version_id),
        ciphertext,
    )?);
    serde_json::from_slice(&plaintext).map_err(|_| CryptoError::MetadataEncoding)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids() -> (NodeId, NodeId, VersionId) {
        (
            NodeId::parse("550e8400-e29b-41d4-a716-446655440000").unwrap(),
            NodeId::parse("550e8400-e29b-41d4-a716-446655440001").unwrap(),
            VersionId::parse("6ba7b810-9dad-41d1-80b4-00c04fd430c8").unwrap(),
        )
    }

    #[test]
    fn node_key_wrap_roundtrip() {
        let (node_id, _, _) = ids();
        let master_key = MasterKey::generate().unwrap();
        let node_key = NodeKey::generate().unwrap();

        let wrapped = wrap_node_key(&master_key, &node_id, &node_key).unwrap();
        let unwrapped = unwrap_node_key(&master_key, &node_id, &wrapped).unwrap();

        assert_eq!(unwrapped.as_bytes(), node_key.as_bytes());
    }

    #[test]
    fn wrapped_key_is_bound_to_node_id() {
        let (node_id, other_node_id, _) = ids();
        let master_key = MasterKey::generate().unwrap();
        let node_key = NodeKey::generate().unwrap();

        let wrapped = wrap_node_key(&master_key, &node_id, &node_key).unwrap();

        assert_eq!(
            unwrap_node_key(&master_key, &other_node_id, &wrapped),
            Err(CryptoError::AuthenticationFailed)
        );
    }

    #[test]
    fn node_metadata_roundtrip_and_binding() {
        let (node_id, other_node_id, _) = ids();
        let node_key = NodeKey::generate().unwrap();
        let metadata = NodeMetadata {
            name: "报告.pdf".to_owned(),
        };

        let ciphertext = encrypt_node_metadata(&node_key, &node_id, &metadata).unwrap();
        let decrypted = decrypt_node_metadata(&node_key, &node_id, &ciphertext).unwrap();
        assert_eq!(decrypted, metadata);

        assert_eq!(
            decrypt_node_metadata(&node_key, &other_node_id, &ciphertext),
            Err(CryptoError::AuthenticationFailed)
        );
    }

    #[test]
    fn node_metadata_tamper_detected() {
        let (node_id, _, _) = ids();
        let node_key = NodeKey::generate().unwrap();
        let metadata = NodeMetadata {
            name: "a.txt".to_owned(),
        };
        let mut ciphertext = encrypt_node_metadata(&node_key, &node_id, &metadata).unwrap();
        let last = ciphertext.len() - 1;
        ciphertext[last] ^= 0x01;

        assert!(decrypt_node_metadata(&node_key, &node_id, &ciphertext).is_err());
    }

    #[test]
    fn version_metadata_roundtrip_and_binding() {
        let (_, _, version_id) = ids();
        let other_version = VersionId::parse("6ba7b810-9dad-41d1-80b4-00c04fd430c9").unwrap();
        let node_key = NodeKey::generate().unwrap();
        let metadata = VersionMetadata {
            size: 10 * 1024 * 1024,
            mtime: 1_700_000_000,
            mime: "application/pdf".to_owned(),
        };

        let ciphertext = encrypt_version_metadata(&node_key, &version_id, &metadata).unwrap();
        assert_eq!(
            decrypt_version_metadata(&node_key, &version_id, &ciphertext).unwrap(),
            metadata
        );
        assert_eq!(
            decrypt_version_metadata(&node_key, &other_version, &ciphertext),
            Err(CryptoError::AuthenticationFailed)
        );
    }

    #[test]
    fn metadata_json_matches_design_shape() {
        let metadata = NodeMetadata {
            name: "a.txt".to_owned(),
        };
        assert_eq!(
            serde_json::to_string(&metadata).unwrap(),
            r#"{"name":"a.txt"}"#
        );

        let version = VersionMetadata {
            size: 1,
            mtime: 2,
            mime: "text/plain".to_owned(),
        };
        assert_eq!(
            serde_json::to_string(&version).unwrap(),
            r#"{"size":1,"mtime":2,"mime":"text/plain"}"#
        );
    }

    #[test]
    fn name_token_is_deterministic() {
        let (node_id, _, _) = ids();
        let name_key = derive_name_key(&MasterKey::generate().unwrap()).unwrap();

        let first = compute_name_token(&name_key, &node_id, "Report.pdf").unwrap();
        let second = compute_name_token(&name_key, &node_id, "Report.pdf").unwrap();

        assert_eq!(first, second);
        assert_eq!(first.as_bytes().len(), NAME_TOKEN_LEN);
    }

    #[test]
    fn name_token_ignores_case_and_unicode_form() {
        let (node_id, _, _) = ids();
        let name_key = derive_name_key(&MasterKey::generate().unwrap()).unwrap();

        let lower = compute_name_token(&name_key, &node_id, "report.pdf").unwrap();
        let upper = compute_name_token(&name_key, &node_id, "REPORT.PDF").unwrap();
        assert_eq!(lower, upper);

        let composed = compute_name_token(&name_key, &node_id, "caf\u{e9}.txt").unwrap();
        let decomposed = compute_name_token(&name_key, &node_id, "cafe\u{301}.txt").unwrap();
        assert_eq!(composed, decomposed);
    }

    #[test]
    fn name_token_differs_by_parent_or_name() {
        let (node_id, other_node_id, _) = ids();
        let name_key = derive_name_key(&MasterKey::generate().unwrap()).unwrap();

        let base = compute_name_token(&name_key, &node_id, "report.pdf").unwrap();
        let other_parent = compute_name_token(&name_key, &other_node_id, "report.pdf").unwrap();
        let other_name = compute_name_token(&name_key, &node_id, "report2.pdf").unwrap();

        assert_ne!(base, other_parent);
        assert_ne!(base, other_name);
    }

    #[test]
    fn name_token_hex_roundtrip() {
        let (node_id, _, _) = ids();
        let name_key = derive_name_key(&MasterKey::generate().unwrap()).unwrap();
        let token = compute_name_token(&name_key, &node_id, "a.txt").unwrap();

        let hex = token.to_hex();
        assert_eq!(hex.len(), NAME_TOKEN_LEN * 2);
        assert_eq!(NameToken::from_hex(&hex).unwrap(), token);
        assert_eq!(
            NameToken::from_hex("zz"),
            Err(CryptoError::InvalidNameToken)
        );
    }

    #[test]
    fn debug_does_not_leak_metadata() {
        let metadata = NodeMetadata {
            name: "secret-plan.txt".to_owned(),
        };
        let printed = format!("{metadata:?}");
        assert!(!printed.contains("secret"));
    }
}
