//! `crypto-core` —— 端对端加密核心。
//!
//! 本 crate 只包含纯粹的密码学与格式逻辑：不做任何 IO，不依赖运行时，
//! 以便同时被 `crypto-wasm`（浏览器 Web Worker）与将来的桌面端 / CLI 复用。
//!
//! 密钥层级、AEAD 帧格式与 AAD 域的权威定义见
//! `openspec/changes/bootstrap-e2ee-backup-mvp/design.md` 的 D5，
//! 具体实现按 `tasks.md` 第 2 组任务逐项补齐。
//!
//! 目前处于工程脚手架阶段：只暴露不可变的格式版本号，其余接口在第 2 组实现。
#![forbid(unsafe_code)]

/// 密文帧格式版本号（1 字节，见 design D5）。
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
