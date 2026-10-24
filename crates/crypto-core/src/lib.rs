//! `crypto-core` —— 端对端加密核心。
//!
//! 本 crate 只包含纯粹的密码学与格式逻辑：不做任何 IO，不依赖运行时，
//! 以便同时被 `crypto-wasm`（浏览器 Web Worker）与将来的桌面端 / CLI 复用。
//!
//! 密钥层级、AEAD 帧格式与 AAD 域的权威定义见
//! `openspec/changes/bootstrap-e2ee-backup-mvp/design.md` 的 D4/D5/D6，
//! 人类可读的格式说明见仓库根目录的 `docs/crypto-format.md`。
//!
//! # 密钥层级
//!
//! ```text
//! password ──Argon2id──▶ root ──HKDF──▶ pwd_wrap / auth_key
//! rk (32B) ──HKDF──▶ rk_wrap / rk_auth
//!
//! MK (32B) ──AEAD(pwd_wrap, aad="bk/v1/mk")──────────────▶ wrapped_mk
//!          ──AEAD(rk_wrap,  aad="bk/v1/mk-recovery")─────▶ recovery_wrapped_mk
//!          ──HKDF(info="bk/v1/name-token")───────────────▶ name_key
//!          └─每个节点 node_key ──AEAD(MK, aad="bk/v1/node-key|"+node_id)─▶ wrapped_key
//!                 ├─节点元数据 AEAD(aad="bk/v1/node-meta|"+node_id)
//!                 ├─版本元数据 AEAD(aad="bk/v1/ver-meta|"+version_id)
//!                 └─content_key (HKDF salt=content_id) ──▶ 每个 4 MiB 块一块密文
//! ```
//!
//! # 格式稳定性
//!
//! 密文帧版本号 [`CIPHERTEXT_FORMAT_VERSION`] 与 [`domain`] 中的全部字符串
//! 一经发布即永久不可变；新格式只能新增版本号，不能修改 `0x01` 的语义。
#![forbid(unsafe_code)]
// 测试代码允许 unwrap/expect：断言失败即测试失败，不需要生产级的错误传播。
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod aad;
pub mod aead;
pub mod chunk;
pub mod domain;
pub(crate) mod encoding;
pub mod error;
pub mod ids;
pub mod kdf;
pub mod keys;
pub mod mk;
pub mod node;
pub mod recovery;

#[cfg(test)]
mod format_docs;
#[cfg(test)]
mod proptests;
#[cfg(test)]
mod vectors;

pub use aead::{FRAME_OVERHEAD, NONCE_LEN, TAG_LEN};
pub use chunk::{
    CHUNK_CIPHERTEXT_OVERHEAD, CHUNK_PLAINTEXT_SIZE, MAX_CHUNK_CIPHERTEXT_SIZE,
    chunk_count_for_size, decrypt_chunk, derive_content_key, encrypt_chunk,
};
pub use domain::*;
pub use error::CryptoError;
pub use ids::{ContentId, NodeId, VersionId};
pub use kdf::{
    KDF_OUTPUT_LEN, KDF_SALT_LEN, KdfAlgorithm, KdfParams, MAX_M_COST_KIB, MAX_T_COST,
    MIN_M_COST_KIB, MIN_T_COST, PasswordKeys, REQUIRED_P_COST, derive_keys_from_password,
    derive_password_keys, derive_root_key, generate_salt, parse_kdf_params,
};
pub use keys::{
    AuthKey, ContentKey, KEY_LEN, MasterKey, NameKey, NodeKey, PasswordWrapKey, RecoveryAuthKey,
    RecoveryKey, RecoveryWrapKey, RootKey,
};
pub use mk::{
    unwrap_mk_with_password, unwrap_mk_with_recovery, wrap_mk_with_password, wrap_mk_with_recovery,
};
pub use node::{
    NAME_TOKEN_LEN, NameToken, NodeMetadata, VersionMetadata, compute_name_token,
    decrypt_node_metadata, decrypt_version_metadata_for_content, derive_name_key,
    encrypt_node_metadata, encrypt_version_metadata, unwrap_node_key, verify_plaintext_size,
    wrap_node_key,
};
pub use recovery::{
    RECOVERY_CHECKSUM_LEN, RECOVERY_DISPLAY_GROUP_SIZE, RECOVERY_KEY_LEN, RECOVERY_SYMBOL_COUNT,
    RecoveryKeys, derive_recovery_keys, encode_recovery_key, parse_recovery_key,
};

/// 密文帧格式版本号（1 字节）。
///
/// 帧布局：`version(1B) ‖ nonce(24B) ‖ ciphertext ‖ tag(16B)`。
///
/// 该值一旦发布即永久不可变：已写入对象存储的密文必须永远可解，
/// 后续格式只能新增版本号，不能修改 `0x01` 的语义。
pub const CIPHERTEXT_FORMAT_VERSION: u8 = 0x01;

#[cfg(test)]
mod tests {
    use super::CIPHERTEXT_FORMAT_VERSION;

    #[test]
    fn format_version_is_frozen() {
        // 修改此断言等同于破坏已存储的密文，必须走新的格式版本。
        assert_eq!(CIPHERTEXT_FORMAT_VERSION, 0x01);
    }
}
