//! macOS Keychain 实现（design D3）。
//!
//! 使用 generic password：
//! - 保护级别 `kSecAttrAccessibleWhenUnlockedThisDeviceOnly`；
//! - `kSecAttrSynchronizable = false`，不随 iCloud 同步、不进备份；
//! - service/account 区分会话令牌与主密钥。

use crypto_core::MasterKey;
use security_framework::access_control::{ProtectionMode, SecAccessControl};
use security_framework::passwords::{
    PasswordOptions, delete_generic_password, get_generic_password, set_generic_password_options,
};

use super::{SecretStore, SecretStoreError, SessionToken};

const SERVICE: &str = "com.encryptedbackup.client";
const SESSION_ACCOUNT: &str = "session-token";
const MASTER_KEY_ACCOUNT: &str = "master-key";

/// `errSecItemNotFound`（macOS Security.framework）。
const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;

/// macOS 钥匙串实现。
pub struct MacOsKeychainStore;

impl MacOsKeychainStore {
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    fn load_entry(account: &str) -> Result<Option<Vec<u8>>, SecretStoreError> {
        match get_generic_password(SERVICE, account) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(None),
            Err(error) => Err(SecretStoreError::Backend(error.to_string())),
        }
    }

    fn store_entry(account: &str, bytes: &[u8]) -> Result<(), SecretStoreError> {
        let mut options = PasswordOptions::new_generic_password(SERVICE, account);
        let access_control = SecAccessControl::create_with_protection(
            Some(ProtectionMode::AccessibleWhenUnlockedThisDeviceOnly),
            0,
        )
        .map_err(|error| SecretStoreError::Backend(error.to_string()))?;
        options.set_access_control(access_control);
        options.set_access_synchronized(Some(false));
        set_generic_password_options(bytes, options)
            .map_err(|error| SecretStoreError::Backend(error.to_string()))
    }

    fn delete_entry(account: &str) -> Result<(), SecretStoreError> {
        match delete_generic_password(SERVICE, account) {
            Ok(()) => Ok(()),
            Err(error) if error.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(()),
            Err(error) => Err(SecretStoreError::Backend(error.to_string())),
        }
    }
}

impl Default for MacOsKeychainStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SecretStore for MacOsKeychainStore {
    fn store_session(&self, token: &SessionToken) -> Result<(), SecretStoreError> {
        Self::store_entry(SESSION_ACCOUNT, token.as_bytes())
    }

    fn load_session(&self) -> Result<Option<SessionToken>, SecretStoreError> {
        Self::load_entry(SESSION_ACCOUNT)?
            .map(SessionToken::from_bytes)
            .transpose()
    }

    fn delete_session(&self) -> Result<(), SecretStoreError> {
        Self::delete_entry(SESSION_ACCOUNT)
    }

    fn store_master_key(&self, key: &MasterKey) -> Result<(), SecretStoreError> {
        Self::store_entry(MASTER_KEY_ACCOUNT, key.expose_for_secret_store())
    }

    fn load_master_key(&self) -> Result<Option<MasterKey>, SecretStoreError> {
        let Some(bytes) = Self::load_entry(MASTER_KEY_ACCOUNT)? else {
            return Ok(None);
        };
        MasterKey::try_from(bytes.as_slice())
            .map(Some)
            .map_err(|_| SecretStoreError::Corrupt)
    }

    fn delete_master_key(&self) -> Result<(), SecretStoreError> {
        Self::delete_entry(MASTER_KEY_ACCOUNT)
    }
}
