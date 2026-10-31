//! Windows 凭据存储实现（design D3）。
//!
//! 两步保护：
//! 1. 先用 DPAPI `CryptProtectData`（当前用户范围，不带
//!    `CRYPTPROTECT_LOCAL_MACHINE`）加密明文；
//! 2. 把 DPAPI 密文的十六进制写入当前用户的 Credential Manager
//!    （`CredWriteW` / `CredReadW`，`CRED_PERSIST_LOCAL_MACHINE`），
//!    凭据目标名固定为 `EncryptedBackup/<account>`。
//!
//! 直接使用 `windows-sys` 而不是 `keyring`：依赖更少、凭据目标名与持久化
//! 语义完全可控，且 `windows-sys` 已是项目依赖（cargo deny 也只需审查一个 crate）。
#![allow(unsafe_code)]

use std::ptr;

use crypto_core::MasterKey;
use windows_sys::Win32::Foundation::{ERROR_NOT_FOUND, GetLastError, HLOCAL, LocalFree};
use windows_sys::Win32::Security::Credentials::{
    CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW, CredDeleteW, CredFree, CredReadW,
    CredWriteW,
};
use windows_sys::Win32::Security::Cryptography::{
    CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
};
use zeroize::Zeroizing;

use crate::hex;

use super::{SecretStore, SecretStoreError, SessionToken};

const SERVICE: &str = "EncryptedBackup";
const SESSION_ACCOUNT: &str = "session-token";
const MASTER_KEY_ACCOUNT: &str = "master-key";

/// Windows 凭据管理器实现（零大小；每次操作创建一个凭据目标）。
pub struct WindowsCredentialStore;

impl WindowsCredentialStore {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for WindowsCredentialStore {
    fn default() -> Self {
        Self::new()
    }
}

fn backend_error(context: &str) -> SecretStoreError {
    let error = std::io::Error::last_os_error();
    SecretStoreError::Backend(format!("{context}: {error}"))
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn target_name(account: &str) -> String {
    format!("{SERVICE}/{account}")
}

/// DPAPI 加密（当前用户）。返回的密文可以安全写入凭据管理器。
fn protect(plaintext: &[u8]) -> Result<Vec<u8>, SecretStoreError> {
    let length = u32::try_from(plaintext.len())
        .map_err(|_| SecretStoreError::Backend("凭据长度超出 DPAPI 限制".to_owned()))?;
    let input = CRYPT_INTEGER_BLOB {
        cbData: length,
        pbData: plaintext.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();

    // SAFETY: input 指向本次调用期间有效的 plaintext；output 由 DPAPI 通过
    // LocalAlloc 分配，成功后立即复制并用 LocalFree 释放。
    let ok = unsafe {
        CryptProtectData(
            &input,
            ptr::null(),
            ptr::null(),
            ptr::null(),
            ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    if ok == 0 {
        return Err(backend_error("DPAPI 加密失败"));
    }
    let bytes = if output.cbData == 0 || output.pbData.is_null() {
        Vec::new()
    } else {
        // SAFETY: 成功返回时 output.pbData 指向 cbData 字节的已初始化缓冲区。
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec()
    };
    // SAFETY: output.pbData 由 CryptProtectData 分配，必须用 LocalFree 释放。
    unsafe {
        LocalFree(output.pbData as HLOCAL);
    }
    Ok(bytes)
}

/// DPAPI 解密（当前用户）。返回的明文在离开作用域时清零。
fn unprotect(ciphertext: &[u8]) -> Result<Zeroizing<Vec<u8>>, SecretStoreError> {
    let length = u32::try_from(ciphertext.len())
        .map_err(|_| SecretStoreError::Backend("凭据长度超出 DPAPI 限制".to_owned()))?;
    let input = CRYPT_INTEGER_BLOB {
        cbData: length,
        pbData: ciphertext.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();

    // SAFETY: input 指向本次调用期间有效的 ciphertext；描述字符串指针传 null，
    // DPAPI 不会分配描述字符串。output 成功时由 LocalAlloc 分配。
    let ok = unsafe {
        CryptUnprotectData(
            &input,
            ptr::null_mut(),
            ptr::null(),
            ptr::null(),
            ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    if ok == 0 {
        return Err(backend_error("DPAPI 解密失败"));
    }
    let bytes = if output.cbData == 0 || output.pbData.is_null() {
        Vec::new()
    } else {
        // SAFETY: 成功返回时 output.pbData 指向 cbData 字节的明文。
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec()
    };
    // SAFETY: 明文缓冲区由 CryptUnprotectData 分配，必须释放；复制后清零原缓冲区。
    unsafe {
        if !output.pbData.is_null() {
            ptr::write_bytes(output.pbData, 0, output.cbData as usize);
        }
        LocalFree(output.pbData as HLOCAL);
    }
    Ok(Zeroizing::new(bytes))
}

fn read_secret(account: &str) -> Result<Option<Vec<u8>>, SecretStoreError> {
    let target = wide(&target_name(account));
    let mut credential: *mut CREDENTIALW = ptr::null_mut();
    // SAFETY: target 是以 NUL 结尾的 UTF-16；成功时 credential 指向
    // CredRead 分配的 CREDENTIALW，读取后必须 CredFree。
    let ok = unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut credential) };
    if ok == 0 {
        // SAFETY: GetLastError 无参数。
        let code = unsafe { GetLastError() };
        if code == ERROR_NOT_FOUND {
            return Ok(None);
        }
        return Err(SecretStoreError::Backend(format!(
            "读取凭据失败（Win32 错误 {code}）"
        )));
    }
    if credential.is_null() {
        return Err(SecretStoreError::Backend("读取凭据返回空指针".to_owned()));
    }

    // SAFETY: CredReadW 成功时 credential 非空，字段在 CredFree 前有效。
    let blob = unsafe {
        let credential = &*credential;
        if credential.CredentialBlobSize == 0 || credential.CredentialBlob.is_null() {
            Vec::new()
        } else {
            std::slice::from_raw_parts(
                credential.CredentialBlob,
                credential.CredentialBlobSize as usize,
            )
            .to_vec()
        }
    };
    // SAFETY: credential 由 CredReadW 分配，必须只释放一次。
    unsafe {
        CredFree(credential.cast());
    }

    let text = String::from_utf8(blob).map_err(|_| SecretStoreError::Corrupt)?;
    hex::decode_hex(&text)
        .map(Some)
        .ok_or(SecretStoreError::Corrupt)
}

fn write_secret(account: &str, value: &[u8]) -> Result<(), SecretStoreError> {
    let target = wide(&target_name(account));
    let encoded = hex::encode_hex(value);
    let mut blob = encoded.into_bytes();
    let blob_len = u32::try_from(blob.len()).map_err(|_| {
        SecretStoreError::Backend("凭据长度超出 Credential Manager 限制".to_owned())
    })?;
    let credential = CREDENTIALW {
        Flags: 0,
        Type: CRED_TYPE_GENERIC,
        TargetName: target.as_ptr().cast_mut(),
        Comment: ptr::null_mut(),
        LastWritten: Default::default(),
        CredentialBlobSize: blob_len,
        CredentialBlob: blob.as_mut_ptr(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        AttributeCount: 0,
        Attributes: ptr::null_mut(),
        TargetAlias: ptr::null_mut(),
        UserName: ptr::null_mut(),
    };
    // SAFETY: target 与 blob 在调用期间有效；CredentialBlob 指向 blob_len 字节。
    let ok = unsafe { CredWriteW(&credential, 0) };
    if ok == 0 {
        return Err(backend_error("写入凭据失败"));
    }
    Ok(())
}

fn delete_secret(account: &str) -> Result<(), SecretStoreError> {
    let target = wide(&target_name(account));
    // SAFETY: target 是以 NUL 结尾的 UTF-16。
    let ok = unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) };
    if ok == 0 {
        // SAFETY: GetLastError 无参数。
        let code = unsafe { GetLastError() };
        if code == ERROR_NOT_FOUND {
            return Ok(());
        }
        return Err(SecretStoreError::Backend(format!(
            "删除凭据失败（Win32 错误 {code}）"
        )));
    }
    Ok(())
}

fn store_protected(account: &str, plaintext: &[u8]) -> Result<(), SecretStoreError> {
    let protected = protect(plaintext)?;
    write_secret(account, &protected)
}

fn load_unprotected(account: &str) -> Result<Option<Zeroizing<Vec<u8>>>, SecretStoreError> {
    match read_secret(account)? {
        Some(protected) => unprotect(&protected).map(Some),
        None => Ok(None),
    }
}

impl SecretStore for WindowsCredentialStore {
    fn store_session(&self, token: &SessionToken) -> Result<(), SecretStoreError> {
        store_protected(SESSION_ACCOUNT, token.as_bytes())
    }

    fn load_session(&self) -> Result<Option<SessionToken>, SecretStoreError> {
        let Some(plaintext) = load_unprotected(SESSION_ACCOUNT)? else {
            return Ok(None);
        };
        SessionToken::from_bytes(plaintext.to_vec()).map(Some)
    }

    fn delete_session(&self) -> Result<(), SecretStoreError> {
        delete_secret(SESSION_ACCOUNT)
    }

    fn store_master_key(&self, key: &MasterKey) -> Result<(), SecretStoreError> {
        store_protected(MASTER_KEY_ACCOUNT, key.expose_for_secret_store())
    }

    fn load_master_key(&self) -> Result<Option<MasterKey>, SecretStoreError> {
        let Some(plaintext) = load_unprotected(MASTER_KEY_ACCOUNT)? else {
            return Ok(None);
        };
        MasterKey::try_from(plaintext.as_slice())
            .map(Some)
            .map_err(|_| SecretStoreError::Corrupt)
    }

    fn delete_master_key(&self) -> Result<(), SecretStoreError> {
        delete_secret(MASTER_KEY_ACCOUNT)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;

    #[test]
    fn dpapi_roundtrip_preserves_bytes_and_hides_plaintext() {
        let plaintext = [0x5a_u8; crypto_core::KEY_LEN];
        let protected = protect(&plaintext).unwrap();
        assert_ne!(protected, plaintext);
        let decoded = unprotect(&protected).unwrap();
        assert_eq!(decoded.as_slice(), plaintext);
    }

    #[test]
    fn dpapi_rejects_tampered_ciphertext() {
        let plaintext = b"session-token";
        let mut protected = protect(plaintext).unwrap();
        let last = protected.len() - 1;
        protected[last] ^= 0xff;
        assert!(unprotect(&protected).is_err());
    }
}
