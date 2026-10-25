//! 明文内容摘要（design add-folder-backup D3）。
//!
//! 文件夹增量备份需要比较本地文件与云端版本元数据里的明文 SHA-256。文件可能很大，
//! 因此这里提供**流式** SHA-256：调用方按块 `update`，最后取小写十六进制结果。
//!
//! 该摘要不是密钥、也不是服务端可见元数据：它只写入加密的版本元数据，服务端只能
//! 看到密文。哈希状态会缓存部分明文，因此复用同一实例时应在 `finish` 后丢弃。

use sha2::{Digest, Sha256};

use crate::encoding::encode_hex;

/// 流式 SHA-256（明文内容摘要）。
#[derive(Clone, Default)]
pub struct ContentHasher {
    inner: Sha256,
}

impl ContentHasher {
    /// 创建新的流式哈希器。
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Sha256::new(),
        }
    }

    /// 追加一段明文。
    pub fn update(&mut self, bytes: &[u8]) {
        self.inner.update(bytes);
    }

    /// 结束并返回 64 位小写十六进制摘要。
    ///
    /// 只读取内部状态的克隆，因此重复调用结果一致；调用方仍应在用完后丢弃实例。
    #[must_use]
    pub fn finish_hex(&self) -> String {
        encode_hex(&self.inner.clone().finalize())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_matches_known_vector() {
        let mut hasher = ContentHasher::new();
        hasher.update(b"abc");
        assert_eq!(
            hasher.finish_hex(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn streaming_matches_single_update() {
        let data = b"the quick brown fox jumps over the lazy dog";
        let mut whole = ContentHasher::new();
        whole.update(data);

        let mut streamed = ContentHasher::new();
        for chunk in data.chunks(5) {
            streamed.update(chunk);
        }

        assert_eq!(whole.finish_hex(), streamed.finish_hex());
        // 重复 finish 不应改变结果（只读取克隆）。
        assert_eq!(whole.finish_hex(), streamed.finish_hex());
    }
}
