//! 凭据存储（design D3）。
//!
//! - 会话令牌：始终保存在系统凭据存储中，关掉应用后不用重新输入邮箱。
//! - 主密钥：默认不持久化；只有用户开启“在这台电脑上保持解锁”时才写入。
//! - 退出登录、修改密码、服务端返回会话失效时调用 [`SecretStore::clear`]。
//!
//! 平台实现：
//! - macOS：`security-framework` generic password，保护级别
//!   `kSecAttrAccessibleWhenUnlockedThisDeviceOnly`，不随 iCloud 同步。
//! - Windows：先用 DPAPI（当前用户）加密，再写入凭据管理器（`windows-sys` 直接调用）。
//! - 其他平台（CI / 审查者的 WSL）：只提供内存实现。

use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard};

use crypto_core::MasterKey;
use zeroize::{Zeroize, ZeroizeOnDrop};

pub use crate::error::SecretStoreError;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

/// 会话令牌。
///
/// 不实现 `Display`/`Serialize`，`Debug` 输出固定脱敏；离开作用域时清零。
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct SessionToken(String);

impl SessionToken {
    /// 包装一个来自可信来源（服务端 Set-Cookie 或系统凭据存储）的令牌。
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// 读取令牌明文；只用于构造 HTTP `Cookie` 头或写入系统凭据存储。
    #[must_use]
    pub fn expose_secret(&self) -> &str {
        &self.0
    }

    // 只有系统钥匙串后端（Windows / macOS）使用；Linux 只有内存实现。
    #[cfg_attr(not(any(target_os = "windows", target_os = "macos")), allow(dead_code))]
    pub(crate) fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }

    /// 从凭据存储读出的字节重建令牌。
    #[cfg_attr(not(any(target_os = "windows", target_os = "macos")), allow(dead_code))]
    pub(crate) fn from_bytes(bytes: Vec<u8>) -> Result<Self, SecretStoreError> {
        if bytes.is_empty() {
            return Err(SecretStoreError::Corrupt);
        }
        String::from_utf8(bytes)
            .map(Self)
            .map_err(|_| SecretStoreError::Corrupt)
    }

    /// HTTP Cookie 头允许的字符集（服务端令牌是 64 位十六进制）。
    pub(crate) fn is_valid_header_value(&self) -> bool {
        !self.0.is_empty()
            && self.0.len() <= 4096
            && self
                .0
                .bytes()
                .all(|byte| byte.is_ascii_graphic() && byte != b';' && byte != b',')
    }
}

impl fmt::Debug for SessionToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SessionToken(<redacted>)")
    }
}

/// 系统凭据存储抽象。
///
/// 所有方法都是同步的：macOS Keychain 与 Windows 凭据管理器都是本地阻塞调用，
/// 由调用方在需要时放到阻塞线程。实现必须是 `Send + Sync`，便于在异步客户端里共享。
pub trait SecretStore: Send + Sync {
    fn store_session(&self, token: &SessionToken) -> Result<(), SecretStoreError>;
    fn load_session(&self) -> Result<Option<SessionToken>, SecretStoreError>;
    fn delete_session(&self) -> Result<(), SecretStoreError>;

    fn store_master_key(&self, key: &MasterKey) -> Result<(), SecretStoreError>;
    fn load_master_key(&self) -> Result<Option<MasterKey>, SecretStoreError>;
    fn delete_master_key(&self) -> Result<(), SecretStoreError>;

    /// 同时删除会话与主密钥。两个删除都会尝试执行；返回第一个错误。
    fn clear(&self) -> Result<(), SecretStoreError> {
        let session = self.delete_session();
        let master_key = self.delete_master_key();
        session.and(master_key)
    }
}

/// 打开当前平台默认的凭据存储。
///
/// Windows / macOS 使用系统钥匙串；其余平台（含 Linux CI）使用进程内内存实现。
#[must_use]
pub fn platform_secret_store() -> Arc<dyn SecretStore> {
    #[cfg(target_os = "windows")]
    {
        Arc::new(windows::WindowsCredentialStore::new())
    }
    #[cfg(target_os = "macos")]
    {
        Arc::new(macos::MacOsKeychainStore::new())
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        Arc::new(MemorySecretStore::new())
    }
}

/// 仅内存实现：用于测试与 Linux/CI（不支持系统钥匙串的平台）。
#[derive(Default)]
pub struct MemorySecretStore {
    session: Mutex<Option<SessionToken>>,
    master_key: Mutex<Option<MasterKey>>,
}

impl MemorySecretStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn lock<T>(mutex: &Mutex<T>) -> Result<MutexGuard<'_, T>, SecretStoreError> {
        mutex.lock().map_err(|_| SecretStoreError::Unavailable)
    }
}

impl SecretStore for MemorySecretStore {
    fn store_session(&self, token: &SessionToken) -> Result<(), SecretStoreError> {
        let mut slot = Self::lock(&self.session)?;
        *slot = Some(token.clone());
        Ok(())
    }

    fn load_session(&self) -> Result<Option<SessionToken>, SecretStoreError> {
        Ok(Self::lock(&self.session)?.clone())
    }

    fn delete_session(&self) -> Result<(), SecretStoreError> {
        *Self::lock(&self.session)? = None;
        Ok(())
    }

    fn store_master_key(&self, key: &MasterKey) -> Result<(), SecretStoreError> {
        let mut slot = Self::lock(&self.master_key)?;
        *slot = Some(key.clone());
        Ok(())
    }

    fn load_master_key(&self) -> Result<Option<MasterKey>, SecretStoreError> {
        Ok(Self::lock(&self.master_key)?.clone())
    }

    fn delete_master_key(&self) -> Result<(), SecretStoreError> {
        *Self::lock(&self.master_key)? = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;

    #[test]
    fn memory_store_roundtrips_and_clears() {
        let store = MemorySecretStore::new();
        let token = SessionToken::new("a".repeat(64));
        let master_key = MasterKey::from_bytes([0x42; crypto_core::KEY_LEN]);

        store.store_session(&token).unwrap();
        store.store_master_key(&master_key).unwrap();
        assert_eq!(
            store.load_session().unwrap().unwrap().expose_secret(),
            token.expose_secret()
        );
        assert_eq!(
            store.load_master_key().unwrap().unwrap(),
            master_key,
            "主密钥应能按原值读回"
        );

        store.clear().unwrap();
        assert!(store.load_session().unwrap().is_none());
        assert!(store.load_master_key().unwrap().is_none());
    }

    #[test]
    fn deleting_missing_entries_is_idempotent() {
        let store = MemorySecretStore::new();
        store.delete_session().unwrap();
        store.delete_master_key().unwrap();
        store.clear().unwrap();
    }

    #[test]
    fn session_token_debug_is_redacted() {
        let secret = "deadbeef".repeat(8);
        let token = SessionToken::new(secret.clone());
        let debug = format!("{token:?}");
        assert_eq!(debug, "SessionToken(<redacted>)");
        assert!(!debug.contains(&secret));
    }

    #[test]
    fn from_bytes_rejects_empty_and_invalid_utf8() {
        assert!(matches!(
            SessionToken::from_bytes(Vec::new()),
            Err(SecretStoreError::Corrupt)
        ));
        assert!(matches!(
            SessionToken::from_bytes(vec![0xff, 0xfe]),
            Err(SecretStoreError::Corrupt)
        ));
    }
}
