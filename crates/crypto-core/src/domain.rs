//! 域分隔常量（AAD 前缀与 HKDF info），权威定义见 design D5。
//!
//! **这些字符串一旦随格式版本 `0x01` 发布就永久不可变。**
//! 任何修改都会让已存入对象存储的密文无法解密，或让不同实现之间互不兼容。
//! 修改前必须新增密文格式版本，并同步更新 `docs/crypto-format.md` 与测试向量。

/// 密码包裹密钥包裹 MK 时使用的 AAD。
pub const AAD_MK: &[u8] = b"bk/v1/mk";

/// 恢复密钥包裹密钥包裹 MK 时使用的 AAD。
pub const AAD_MK_RECOVERY: &[u8] = b"bk/v1/mk-recovery";

/// 节点密钥包裹的 AAD 前缀，其后拼接 node_id（UUID 规范小写字符串）。
pub const AAD_NODE_KEY_PREFIX: &[u8] = b"bk/v1/node-key|";

/// 节点元数据加密的 AAD 前缀，其后拼接 node_id。
pub const AAD_NODE_META_PREFIX: &[u8] = b"bk/v1/node-meta|";

/// 版本元数据加密的 AAD 前缀，其后拼接 version_id。
pub const AAD_VERSION_META_PREFIX: &[u8] = b"bk/v1/ver-meta|";

/// 文件块加密的 AAD 前缀，其后依次拼接 content_id、u32be(idx)、u32be(count)、u8(is_last)。
pub const AAD_CHUNK_PREFIX: &[u8] = b"bk/v1/chunk|";

/// HKDF info：从根密钥派生密码包裹密钥。
pub const HKDF_INFO_PWD_WRAP: &[u8] = b"bk/v1/pwd-wrap";

/// HKDF info：从根密钥派生认证密钥。
pub const HKDF_INFO_AUTH: &[u8] = b"bk/v1/auth";

/// HKDF info：从 MK 派生名称令牌密钥。
pub const HKDF_INFO_NAME_TOKEN: &[u8] = b"bk/v1/name-token";

/// HKDF info：从节点密钥派生内容密钥（salt 为 content_id）。
pub const HKDF_INFO_CONTENT: &[u8] = b"bk/v1/content";

/// HKDF info：从恢复密钥派生包裹密钥。
pub const HKDF_INFO_RK_WRAP: &[u8] = b"bk/v1/rk-wrap";

/// HKDF info：从恢复密钥派生认证值。
pub const HKDF_INFO_RK_AUTH: &[u8] = b"bk/v1/rk-auth";

/// 全部域分隔字符串的清单。
///
/// 用于文档一致性测试（`docs/crypto-format.md` 必须逐字包含这些值），
/// 以及未来跨实现测试向量的自检。新增域时**只能追加**，不能修改已有项。
pub const ALL_DOMAIN_STRINGS: &[&[u8]] = &[
    AAD_MK,
    AAD_MK_RECOVERY,
    AAD_NODE_KEY_PREFIX,
    AAD_NODE_META_PREFIX,
    AAD_VERSION_META_PREFIX,
    AAD_CHUNK_PREFIX,
    HKDF_INFO_PWD_WRAP,
    HKDF_INFO_AUTH,
    HKDF_INFO_NAME_TOKEN,
    HKDF_INFO_CONTENT,
    HKDF_INFO_RK_WRAP,
    HKDF_INFO_RK_AUTH,
];
