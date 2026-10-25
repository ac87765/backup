//! 密码学错误的统一类型。
//!
//! 安全约束：`Display`/`Debug` 输出绝不能包含明文、密钥、口令派生值或元数据内容，
//! 否则一旦被上位应用写入日志就会违反 AGENTS.md 的日志规则。
//! 因此错误只描述“发生了什么”，不携带任何数据字节。

use thiserror::Error;

/// `crypto-core` 的全部错误。
#[derive(Debug, Error, PartialEq, Eq)]
pub enum CryptoError {
    /// 密文帧的版本号不是本实现支持的版本，应提示用户升级客户端。
    #[error("不支持的密文格式版本 {0:#04x}，请升级客户端")]
    UnsupportedFormatVersion(u8),

    /// 密文帧短于最小长度（`version + nonce + tag`）。
    #[error("密文帧被截断：至少需要 {min} 字节")]
    TruncatedFrame {
        /// 最小合法帧长度。
        min: usize,
    },

    /// AEAD 认证失败：密钥、AAD 或密文不匹配（不区分具体原因）。
    #[error("认证失败：密文或附加数据不完整/被篡改")]
    AuthenticationFailed,

    /// 字节长度不符合密钥要求。
    #[error("密钥材料长度错误")]
    InvalidKeyLength,

    /// KDF 参数缺失或超出安全范围（防止服务端下发极端参数造成客户端 DoS）。
    #[error("KDF 参数非法或超出允许范围")]
    InvalidKdfParams,

    /// Argon2 计算失败。
    #[error("Argon2 密钥派生失败")]
    KdfFailure,

    /// UUID 格式非法。
    #[error("非法的 {kind} ID：要求 UUID 文本")]
    InvalidId {
        /// ID 种类（node / content / version）。
        kind: &'static str,
    },

    /// 恢复密钥的分组/字符集/长度不合法。
    #[error("恢复密钥编码非法")]
    InvalidRecoveryEncoding,

    /// 恢复密钥校验位不匹配（通常是抄写错误）。
    #[error("恢复密钥校验位不匹配，请核对后重新输入")]
    RecoveryChecksumMismatch,

    /// 分块参数自相矛盾（index/count/is_last/块大小）。
    #[error("分块参数非法：index/count/is_last/块大小不一致")]
    InvalidChunkLayout,

    /// 元数据 JSON 序列化或反序列化失败。
    #[error("元数据编码失败")]
    MetadataEncoding,

    /// KDF 参数中的算法标识缺失或不是本实现支持的算法。
    #[error("不支持的 KDF 算法：参数缺失或需要升级客户端")]
    UnsupportedKdfAlgorithm,

    /// 版本元数据里的 content_id 与服务端给出的内容不一致（可能是回滚攻击）。
    #[error("版本元数据中的内容 ID 与服务端给出的内容不一致")]
    ContentBindingMismatch,

    /// 解密后的明文总长度与版本元数据记录的大小不一致。
    #[error("解密后的明文总长度与版本元数据记录的大小不一致")]
    PlaintextSizeMismatch,

    /// 名称令牌的十六进制表示非法。
    #[error("名称令牌格式非法")]
    InvalidNameToken,

    /// 明文内容 SHA-256 字段格式非法（要求 64 位小写十六进制）。
    #[error("content_sha256 必须是 64 位小写十六进制")]
    InvalidContentSha256,

    /// 系统随机数源不可用。
    #[error("无法获取安全随机数")]
    Randomness,
}
