//! AAD 构造函数。
//!
//! 所有 AEAD 调用都必须带 AAD，且 AAD 来自本模块，保证同一密钥在不同用途、
//! 不同对象之间不能互相替换密文（design D5 的“域分隔”）。

use crate::domain::{
    AAD_CHUNK_PREFIX, AAD_MK, AAD_MK_RECOVERY, AAD_NODE_KEY_PREFIX, AAD_NODE_META_PREFIX,
    AAD_VERSION_META_PREFIX,
};
use crate::ids::{ContentId, NodeId, VersionId};

/// 包裹 MK（密码路径）的 AAD。
#[must_use]
pub fn master_key() -> &'static [u8] {
    AAD_MK
}

/// 包裹 MK（恢复密钥路径）的 AAD。
#[must_use]
pub fn master_key_recovery() -> &'static [u8] {
    AAD_MK_RECOVERY
}

/// 包裹节点密钥的 AAD：`"bk/v1/node-key|" ‖ node_id`。
#[must_use]
pub fn node_key(node_id: &NodeId) -> Vec<u8> {
    concat_prefix(AAD_NODE_KEY_PREFIX, node_id.as_bytes())
}

/// 节点元数据的 AAD：`"bk/v1/node-meta|" ‖ node_id`。
#[must_use]
pub fn node_metadata(node_id: &NodeId) -> Vec<u8> {
    concat_prefix(AAD_NODE_META_PREFIX, node_id.as_bytes())
}

/// 版本元数据的 AAD：`"bk/v1/ver-meta|" ‖ version_id`。
#[must_use]
pub fn version_metadata(version_id: &VersionId) -> Vec<u8> {
    concat_prefix(AAD_VERSION_META_PREFIX, version_id.as_bytes())
}

/// 文件块的 AAD：
/// `"bk/v1/chunk|" ‖ content_id ‖ u32be(index) ‖ u32be(count) ‖ u8(is_last)`。
#[must_use]
pub fn chunk(content_id: &ContentId, index: u32, count: u32, is_last: bool) -> Vec<u8> {
    let mut aad = Vec::with_capacity(AAD_CHUNK_PREFIX.len() + content_id.as_bytes().len() + 9);
    aad.extend_from_slice(AAD_CHUNK_PREFIX);
    aad.extend_from_slice(content_id.as_bytes());
    aad.extend_from_slice(&index.to_be_bytes());
    aad.extend_from_slice(&count.to_be_bytes());
    aad.push(u8::from(is_last));
    aad
}

fn concat_prefix(prefix: &[u8], id: &[u8]) -> Vec<u8> {
    let mut aad = Vec::with_capacity(prefix.len() + id.len());
    aad.extend_from_slice(prefix);
    aad.extend_from_slice(id);
    aad
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_aad_binds_id() {
        let first = NodeId::parse("550e8400-e29b-41d4-a716-446655440000").unwrap();
        let second = NodeId::parse("550e8400-e29b-41d4-a716-446655440001").unwrap();

        assert_eq!(
            node_key(&first),
            b"bk/v1/node-key|550e8400-e29b-41d4-a716-446655440000"
        );
        assert_ne!(node_key(&first), node_key(&second));
        assert_ne!(node_metadata(&first), node_key(&first));
    }

    #[test]
    fn chunk_aad_layout_matches_design() {
        let content = ContentId::parse("550e8400-e29b-41d4-a716-446655440000").unwrap();
        let aad = chunk(&content, 2, 5, false);

        let mut expected = Vec::new();
        expected.extend_from_slice(b"bk/v1/chunk|");
        expected.extend_from_slice(b"550e8400-e29b-41d4-a716-446655440000");
        expected.extend_from_slice(&2u32.to_be_bytes());
        expected.extend_from_slice(&5u32.to_be_bytes());
        expected.push(0);
        assert_eq!(aad, expected);

        // is_last 必须真实进入 AAD。
        assert_ne!(aad, chunk(&content, 2, 5, true));
    }

    #[test]
    fn mk_aad_is_domain_separated() {
        assert_ne!(master_key(), master_key_recovery());
        assert_eq!(master_key(), b"bk/v1/mk");
        assert_eq!(master_key_recovery(), b"bk/v1/mk-recovery");
    }
}
