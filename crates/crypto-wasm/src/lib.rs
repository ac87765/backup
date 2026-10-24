//! `crypto-wasm` —— `crypto-core` 的 wasm-bindgen 绑定。
//!
//! 浏览器侧的所有加解密都经由本 crate 编译出的 WASM 完成，并且只允许在
//! Web Worker 中加载：密钥不得进入主线程（见 design D1/D12）。
//!
//! 目前处于工程脚手架阶段：只暴露格式版本号，用来验证 wasm 构建链路，
//! Worker 所需的完整 API（`unlock`/`lock`/`upload`/`download` 等）在第 3 组实现。
#![forbid(unsafe_code)]

/// 返回当前密文帧格式版本号（见 design D5）。
///
/// 导出此函数的目的：让 CI 的 wasm 构建与浏览器侧契约测试有真实可调用的入口。
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn ciphertext_format_version() -> u8 {
    crypto_core::CIPHERTEXT_FORMAT_VERSION
}

#[cfg(test)]
mod tests {
    use super::ciphertext_format_version;

    #[test]
    fn exposes_core_format_version() {
        assert_eq!(ciphertext_format_version(), 0x01);
    }
}
