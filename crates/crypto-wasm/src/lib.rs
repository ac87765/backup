//! `crypto-wasm` —— `crypto-core` 的 wasm-bindgen 绑定。
//!
//! 浏览器侧的所有加解密都经由本 crate 编译出的 WASM 完成，**只允许在 Web Worker 中加载**
//! （design D1/D12）：密钥只存在于 WASM 线性内存里，JS 侧拿到的是不透明的 `KeyStore`
//! 对象与包裹后的密文，永远拿不到 MK、节点密钥、内容密钥、名称密钥、密码包裹密钥或恢复包裹密钥。
//!
//! # 对外契约
//!
//! - 唯一允许“原样返回字节”的密钥是 `AuthKey` 与 `RecoveryAuthKey`：它们本来就要发送给
//!   服务端（服务端只存其含 pepper 的 HMAC / 校验值）。
//! - 其余方法只返回：包裹后的密文、明文（文件名/版本元数据/文件块）、名称令牌或布尔/空值。
//! - 版本元数据只导出带 content 校验的 [`KeyStore::decrypt_version_metadata_for_content`]。
//! - [`KeyStore::lock`] 会清零并释放全部密钥，之后任何需要密钥的调用都返回 `locked` 错误。
//!
//! 错误通过带 `code` 属性的 JS `Error` 抛出，Worker 据此把稳定错误码透传给主线程。

#![forbid(unsafe_code)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

use std::collections::HashMap;

use crypto_core::{
    ContentId, CryptoError, KDF_SALT_LEN, KdfParams, MasterKey, NodeId, NodeKey, NodeMetadata,
    PasswordKeys, PasswordWrapKey, RecoveryKey, VersionId, VersionMetadata, compute_name_token,
    decrypt_chunk as core_decrypt_chunk, decrypt_node_metadata as core_decrypt_node_metadata,
    decrypt_version_metadata_for_content as core_decrypt_version_metadata_for_content,
    derive_content_key, derive_keys_from_password, derive_name_key, derive_recovery_keys,
    encode_recovery_key, encrypt_chunk as core_encrypt_chunk,
    encrypt_node_metadata as core_encrypt_node_metadata,
    encrypt_version_metadata as core_encrypt_version_metadata, generate_salt, parse_kdf_params,
    parse_recovery_key, unwrap_mk_with_password, unwrap_mk_with_recovery, unwrap_node_key,
    wrap_mk_with_password, wrap_mk_with_recovery, wrap_node_key,
};
use js_sys::{Error as JsError, Reflect};
use tsify::{Ts, Tsify};
use wasm_bindgen::prelude::*;
use zeroize::Zeroize;

/// 返回当前密文帧格式版本号（见 design D5）。
#[wasm_bindgen]
pub fn ciphertext_format_version() -> u8 {
    crypto_core::CIPHERTEXT_FORMAT_VERSION
}

/// JS 安全整数上界：超过它的 `number` 不再精确。
const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

/// 注册 prepare 阶段返回给 Worker 的材料。
///
/// 所有字节字段用 `Vec<u8>` getter 暴露为 `Uint8Array`；Worker 在请求体构造后立即
/// `fill(0)`。这些值只在 WASM → Worker 之间传递，绝不经过 Comlink。
#[wasm_bindgen]
pub struct PreparedRegistration {
    display: String,
    wrapped_mk: Vec<u8>,
    auth_key: Vec<u8>,
    recovery_wrapped_mk: Vec<u8>,
    recovery_auth_key: Vec<u8>,
    kdf_params_json: String,
    kdf_salt: Vec<u8>,
}

#[wasm_bindgen]
impl PreparedRegistration {
    #[wasm_bindgen(getter)]
    pub fn display(&self) -> String {
        self.display.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn wrapped_mk(&self) -> Vec<u8> {
        self.wrapped_mk.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn auth_key(&self) -> Vec<u8> {
        self.auth_key.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn recovery_wrapped_mk(&self) -> Vec<u8> {
        self.recovery_wrapped_mk.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn recovery_auth_key(&self) -> Vec<u8> {
        self.recovery_auth_key.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn kdf_params_json(&self) -> String {
        self.kdf_params_json.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn kdf_salt(&self) -> Vec<u8> {
        self.kdf_salt.clone()
    }
}

impl Drop for PreparedRegistration {
    fn drop(&mut self) {
        self.auth_key.zeroize();
        self.recovery_auth_key.zeroize();
        self.recovery_wrapped_mk.zeroize();
        self.kdf_salt.zeroize();
    }
}

/// 修改密码 prepare 阶段返回给 Worker 的材料。
#[wasm_bindgen]
pub struct PreparedPasswordChange {
    old_auth_key: Vec<u8>,
    new_auth_key: Vec<u8>,
    new_kdf_params_json: String,
    new_kdf_salt: Vec<u8>,
    new_wrapped_mk: Vec<u8>,
}

#[wasm_bindgen]
impl PreparedPasswordChange {
    #[wasm_bindgen(getter)]
    pub fn old_auth_key(&self) -> Vec<u8> {
        self.old_auth_key.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn new_auth_key(&self) -> Vec<u8> {
        self.new_auth_key.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn new_kdf_params_json(&self) -> String {
        self.new_kdf_params_json.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn new_kdf_salt(&self) -> Vec<u8> {
        self.new_kdf_salt.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn new_wrapped_mk(&self) -> Vec<u8> {
        self.new_wrapped_mk.clone()
    }
}

impl Drop for PreparedPasswordChange {
    fn drop(&mut self) {
        self.old_auth_key.zeroize();
        self.new_auth_key.zeroize();
        self.new_kdf_salt.zeroize();
        self.new_wrapped_mk.zeroize();
    }
}

/// 恢复重置 prepare 阶段返回给 Worker 的材料。
#[wasm_bindgen]
pub struct PreparedRecoveryReset {
    recovery_auth_key: Vec<u8>,
    new_auth_key: Vec<u8>,
    new_kdf_params_json: String,
    new_kdf_salt: Vec<u8>,
    new_wrapped_mk: Vec<u8>,
}

#[wasm_bindgen]
impl PreparedRecoveryReset {
    #[wasm_bindgen(getter)]
    pub fn recovery_auth_key(&self) -> Vec<u8> {
        self.recovery_auth_key.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn new_auth_key(&self) -> Vec<u8> {
        self.new_auth_key.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn new_kdf_params_json(&self) -> String {
        self.new_kdf_params_json.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn new_kdf_salt(&self) -> Vec<u8> {
        self.new_kdf_salt.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn new_wrapped_mk(&self) -> Vec<u8> {
        self.new_wrapped_mk.clone()
    }
}

impl Drop for PreparedRecoveryReset {
    fn drop(&mut self) {
        self.recovery_auth_key.zeroize();
        self.new_auth_key.zeroize();
        self.new_kdf_salt.zeroize();
        self.new_wrapped_mk.zeroize();
    }
}

/// 轮换恢复密钥 prepare 阶段返回给 Worker 的材料。
#[wasm_bindgen]
pub struct PreparedRecoveryRotation {
    current_auth_key: Vec<u8>,
    recovery_wrapped_mk: Vec<u8>,
    recovery_auth_key: Vec<u8>,
    display: String,
}

#[wasm_bindgen]
impl PreparedRecoveryRotation {
    #[wasm_bindgen(getter)]
    pub fn current_auth_key(&self) -> Vec<u8> {
        self.current_auth_key.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn recovery_wrapped_mk(&self) -> Vec<u8> {
        self.recovery_wrapped_mk.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn recovery_auth_key(&self) -> Vec<u8> {
        self.recovery_auth_key.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn display(&self) -> String {
        self.display.clone()
    }
}

impl Drop for PreparedRecoveryRotation {
    fn drop(&mut self) {
        self.current_auth_key.zeroize();
        self.recovery_wrapped_mk.zeroize();
        self.recovery_auth_key.zeroize();
    }
}

/// WASM 内部唯一待提交对象。新的 prepare 会替换并清零旧对象。
enum PendingOperation {
    Registration {
        master_key: MasterKey,
        pwd_wrap: PasswordWrapKey,
    },
    PasswordChange {
        pwd_wrap: PasswordWrapKey,
    },
    RecoveryReset,
    RecoveryRotation,
}

/// 版本元数据的只读视图（解密后返回给 Worker/UI）。
#[derive(Tsify, serde::Serialize, serde::Deserialize)]
pub struct VersionMetadataView {
    /// 该版本引用的 content ID（已与服务端给出的值校验一致）。
    pub content_id: String,
    /// 明文总大小（字节）。
    pub size: f64,
    /// 原始修改时间（Unix 秒）。
    pub mtime: f64,
    /// MIME 类型。
    pub mime: String,
}

/// 密钥库错误（内部类型，转换成带 code 的 JS Error）。
#[derive(Debug)]
pub enum KeyStoreError {
    /// 密钥库已锁定。
    Locked,
    /// 还没有派生密码包裹密钥（未调用 derive_password_keys）。
    NotInitialized,
    /// 节点密钥没有加载到 WASM 内存。
    NodeKeyMissing,
    /// KDF 盐长度不对。
    InvalidSalt,
    /// JS number 不能安全地转换成 u64/i64。
    InvalidNumber,
    /// 返回值序列化为 JS 对象失败。
    Serialization,
    /// commit 时没有对应类型的待提交对象。
    PendingMissing,
    /// 仅用于 `unlock_with_password`：密码认证失败。
    InvalidCredentials,
    /// 仅用于 `unlock_with_recovery`：恢复密钥无效或认证失败。
    InvalidRecoveryKey,
    /// 其余解包/解密操作（节点密钥、元数据、块）的认证失败。
    ///
    /// 覆盖篡改、重排、调换节点密钥等完整性错误；这些情况下数据本身不可信，
    /// 不应伪装成“凭据错误”。
    Integrity,
    /// crypto-core 抛出的领域错误。
    Crypto(CryptoError),
}

impl From<CryptoError> for KeyStoreError {
    fn from(error: CryptoError) -> Self {
        Self::Crypto(error)
    }
}

impl KeyStoreError {
    /// 解包/解密路径：认证失败 → `integrity_error`，其余错误保持原样。
    fn from_integrity(error: CryptoError) -> Self {
        match error {
            CryptoError::AuthenticationFailed => Self::Integrity,
            other => Self::Crypto(other),
        }
    }

    /// 密码解锁路径：认证失败 → `invalid_credentials`。
    fn from_credentials(error: CryptoError) -> Self {
        match error {
            CryptoError::AuthenticationFailed => Self::InvalidCredentials,
            other => Self::Crypto(other),
        }
    }

    /// 恢复密钥路径：解析失败或认证失败都视为恢复密钥无效。
    fn from_recovery(error: CryptoError) -> Self {
        match error {
            CryptoError::AuthenticationFailed
            | CryptoError::RecoveryChecksumMismatch
            | CryptoError::InvalidRecoveryEncoding => Self::InvalidRecoveryKey,
            other => Self::Crypto(other),
        }
    }

    fn code(&self) -> &'static str {
        match self {
            Self::Locked => "locked",
            Self::NotInitialized => "not_initialized",
            Self::NodeKeyMissing => "node_key_missing",
            Self::InvalidSalt => "invalid_salt",
            Self::InvalidNumber => "invalid_number",
            Self::Serialization => "serialization_error",
            Self::PendingMissing => "pending_missing",
            Self::InvalidCredentials => "invalid_credentials",
            Self::InvalidRecoveryKey => "invalid_recovery_key",
            Self::Integrity => "integrity_error",
            // 认证失败不在全局表里映射：它必须由调用场景决定是凭据错误还是完整性错误。
            Self::Crypto(error) => match error {
                CryptoError::ContentBindingMismatch => "content_binding_mismatch",
                CryptoError::PlaintextSizeMismatch => "plaintext_size_mismatch",
                CryptoError::UnsupportedKdfAlgorithm => "unsupported_kdf_algorithm",
                CryptoError::InvalidKdfParams => "invalid_kdf_params",
                CryptoError::InvalidId { .. } => "invalid_id",
                CryptoError::InvalidChunkLayout => "invalid_chunk_layout",
                CryptoError::UnsupportedFormatVersion(_) => "unsupported_format_version",
                _ => "crypto_error",
            },
        }
    }

    fn message(&self) -> String {
        match self {
            Self::Locked => "密钥库已锁定，请先解锁".to_owned(),
            Self::NotInitialized => "尚未派生密码包裹密钥".to_owned(),
            Self::NodeKeyMissing => "节点密钥尚未加载".to_owned(),
            Self::InvalidSalt => "KDF 盐长度必须为 16 字节".to_owned(),
            Self::InvalidNumber => "数值超出可安全传输的范围".to_owned(),
            Self::Serialization => "返回值序列化失败".to_owned(),
            Self::PendingMissing => "没有待提交的认证材料".to_owned(),
            Self::InvalidCredentials => "密码错误或凭据无效".to_owned(),
            Self::InvalidRecoveryKey => "恢复密钥无效".to_owned(),
            Self::Integrity => "数据完整性校验失败".to_owned(),
            Self::Crypto(error) => error.to_string(),
        }
    }
}

impl From<KeyStoreError> for JsValue {
    fn from(error: KeyStoreError) -> Self {
        let js = JsError::new(&error.message());
        // `code` 是稳定错误码，Worker 会原样透传给主线程。
        let _ = Reflect::set(
            &js,
            &JsValue::from_str("code"),
            &JsValue::from_str(error.code()),
        );
        js.into()
    }
}

/// 把实现了 `Tsify + Serialize` 的结构体转换成可跨 ABI 返回的 TS 对象。
fn to_ts<T>(value: &T) -> Result<Ts<T>, KeyStoreError>
where
    T: Tsify + serde::Serialize,
{
    value.into_ts().map_err(|_| KeyStoreError::Serialization)
}

/// 推荐 KDF 参数的 JSON（新密码一律使用推荐参数）。
fn recommended_params_json() -> Result<String, KeyStoreError> {
    serde_json::to_string(&KdfParams::RECOMMENDED).map_err(|_| KeyStoreError::Serialization)
}

/// 浏览器侧密钥库。
///
/// 构造后处于锁定状态；调用 [`KeyStore::derive_password_keys`] 派生密码包裹密钥，
/// 再用 [`KeyStore::create_master_key`] 或 [`KeyStore::unlock_with_password`] /
/// [`KeyStore::unlock_with_recovery`] 进入解锁状态。解锁后 MK 只保存在 WASM 内存。
#[wasm_bindgen]
#[derive(Default)]
pub struct KeyStore {
    pwd_wrap: Option<PasswordWrapKey>,
    master_key: Option<MasterKey>,
    /// node_id（规范小写 UUID）→ 节点密钥。
    node_keys: HashMap<String, NodeKey>,
    /// 唯一待提交对象；prepare 写入，commit/discard/lock 清零。
    pending: Option<PendingOperation>,
}

impl KeyStore {
    fn require_master_key(&self) -> Result<&MasterKey, KeyStoreError> {
        self.master_key.as_ref().ok_or(KeyStoreError::Locked)
    }

    /// 取节点密钥；同时确认密钥库已解锁（避免“节点密钥还在但 MK 已清零”的歧义）。
    fn lookup_node_key(&self, raw_node_id: &str) -> Result<(NodeId, &NodeKey), KeyStoreError> {
        if self.master_key.is_none() {
            return Err(KeyStoreError::Locked);
        }
        let node_id = NodeId::parse(raw_node_id)?;
        let node_key = self
            .node_keys
            .get(node_id.as_str())
            .ok_or(KeyStoreError::NodeKeyMissing)?;
        Ok((node_id, node_key))
    }
}

#[wasm_bindgen]
impl KeyStore {
    /// 创建空的密钥库（锁定状态）。
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self::default()
    }

    /// 是否处于锁定状态（没有可用的 MK）。
    pub fn is_locked(&self) -> bool {
        self.master_key.is_none()
    }

    /// 清零并释放 WASM 内的所有密钥（MK、密码包裹密钥、全部节点密钥）。
    ///
    /// 依赖各密钥类型的 `ZeroizeOnDrop`：从容器中移除即清零。
    pub fn lock(&mut self) {
        self.master_key = None;
        self.pwd_wrap = None;
        self.node_keys.clear();
        self.pending = None;
    }

    /// 从密码派生密钥，并把 `pwd_wrap` 留在 WASM 内。
    ///
    /// 返回 `auth_key` 字节（允许离开 WASM 的唯一密码派生值，服务端只存其 HMAC）。
    /// KDF 参数与盐来自服务端响应，这里会校验算法与参数范围。
    pub fn derive_password_keys(
        &mut self,
        password: &str,
        kdf_params_json: &str,
        salt: &[u8],
    ) -> Result<Vec<u8>, KeyStoreError> {
        let salt: [u8; KDF_SALT_LEN] = salt.try_into().map_err(|_| KeyStoreError::InvalidSalt)?;
        let params: KdfParams = parse_kdf_params(kdf_params_json)?;
        let keys = derive_keys_from_password(password, &salt, &params)?;
        let auth_key = keys.auth_key.expose_secret().to_vec();
        self.pwd_wrap = Some(keys.pwd_wrap);
        Ok(auth_key)
    }

    /// 生成新的 MK 并用已派生的密码包裹密钥包裹；MK 留在 WASM 内。
    ///
    /// 返回 `wrapped_mk`（密文）供注册请求使用。
    pub fn create_master_key(&mut self) -> Result<Vec<u8>, KeyStoreError> {
        let pwd_wrap = self
            .pwd_wrap
            .as_ref()
            .ok_or(KeyStoreError::NotInitialized)?;
        let master_key = MasterKey::generate()?;
        let wrapped = wrap_mk_with_password(pwd_wrap, &master_key)?;
        self.master_key = Some(master_key);
        Ok(wrapped)
    }

    /// 用密码包裹的 MK 解锁。
    ///
    /// 认证失败映射为 `invalid_credentials`（只有这里使用该错误码）。
    pub fn unlock_with_password(&mut self, wrapped_mk: &[u8]) -> Result<(), KeyStoreError> {
        let pwd_wrap = self
            .pwd_wrap
            .as_ref()
            .ok_or(KeyStoreError::NotInitialized)?;
        let master_key = unwrap_mk_with_password(pwd_wrap, wrapped_mk)
            .map_err(KeyStoreError::from_credentials)?;
        self.master_key = Some(master_key);
        Ok(())
    }

    /// 用恢复密钥解锁（忘记密码场景）。
    ///
    /// 恢复密钥解析失败或认证失败映射为 `invalid_recovery_key`（只有这里使用该错误码）。
    pub fn unlock_with_recovery(
        &mut self,
        recovery_key_display: &str,
        recovery_wrapped_mk: &[u8],
    ) -> Result<(), KeyStoreError> {
        let recovery_key =
            parse_recovery_key(recovery_key_display).map_err(KeyStoreError::from_recovery)?;
        let keys = derive_recovery_keys(&recovery_key)?;
        let master_key = unwrap_mk_with_recovery(&keys.rk_wrap, recovery_wrapped_mk)
            .map_err(KeyStoreError::from_recovery)?;
        self.master_key = Some(master_key);
        Ok(())
    }

    /// 校验用户输入的恢复密钥（校验位/字符集），不发送任何请求。
    pub fn validate_recovery_key(&self, display: &str) -> Result<(), KeyStoreError> {
        parse_recovery_key(display)
            .map(|_| ())
            .map_err(KeyStoreError::from_recovery)
    }

    /// 注册 prepare：生成 MK、密码包裹材料与恢复材料；**不修改现有 KeyStore 状态**。
    pub fn prepare_registration(
        &mut self,
        password: &str,
        kdf_params_json: &str,
        salt: &[u8],
    ) -> Result<PreparedRegistration, KeyStoreError> {
        let salt: [u8; KDF_SALT_LEN] = salt.try_into().map_err(|_| KeyStoreError::InvalidSalt)?;
        let params: KdfParams = parse_kdf_params(kdf_params_json)?;
        let PasswordKeys { pwd_wrap, auth_key } =
            derive_keys_from_password(password, &salt, &params)?;
        let master_key = MasterKey::generate()?;
        let wrapped_mk = wrap_mk_with_password(&pwd_wrap, &master_key)?;

        let recovery_key = RecoveryKey::generate()?;
        let recovery_keys = derive_recovery_keys(&recovery_key)?;
        let recovery_wrapped_mk = wrap_mk_with_recovery(&recovery_keys.rk_wrap, &master_key)?;
        let recovery_auth_key = recovery_keys.rk_auth.expose_secret().to_vec();
        let auth_key = auth_key.expose_secret().to_vec();
        let display = encode_recovery_key(&recovery_key);
        let kdf_params_json = kdf_params_json.to_owned();
        let kdf_salt = salt.to_vec();

        self.pending = Some(PendingOperation::Registration {
            master_key,
            pwd_wrap,
        });

        Ok(PreparedRegistration {
            display,
            wrapped_mk,
            auth_key,
            recovery_wrapped_mk,
            recovery_auth_key,
            kdf_params_json,
            kdf_salt,
        })
    }

    /// 修改密码 prepare：旧 auth_key（用当前参数/盐）+ 新密码的新盐/推荐参数 + 新包裹的当前 MK。
    pub fn prepare_password_change(
        &mut self,
        old_password: &str,
        current_kdf_params_json: &str,
        current_salt: &[u8],
        new_password: &str,
    ) -> Result<PreparedPasswordChange, KeyStoreError> {
        let current_salt: [u8; KDF_SALT_LEN] = current_salt
            .try_into()
            .map_err(|_| KeyStoreError::InvalidSalt)?;
        let current_params: KdfParams = parse_kdf_params(current_kdf_params_json)?;
        let old_keys = derive_keys_from_password(old_password, &current_salt, &current_params)?;

        let new_salt = generate_salt()?;
        let new_params = KdfParams::RECOMMENDED;
        let new_keys = derive_keys_from_password(new_password, &new_salt, &new_params)?;
        let new_params_json = recommended_params_json()?;
        let new_wrapped_mk = {
            let master_key = self.require_master_key()?;
            wrap_mk_with_password(&new_keys.pwd_wrap, master_key)?
        };

        self.pending = Some(PendingOperation::PasswordChange {
            pwd_wrap: new_keys.pwd_wrap,
        });

        Ok(PreparedPasswordChange {
            old_auth_key: old_keys.auth_key.expose_secret().to_vec(),
            new_auth_key: new_keys.auth_key.expose_secret().to_vec(),
            new_kdf_params_json: new_params_json,
            new_kdf_salt: new_salt.to_vec(),
            new_wrapped_mk,
        })
    }

    /// 恢复重置 prepare：用恢复密钥解包 MK，生成新密码材料；MK 只短暂存在于待提交流程中。
    pub fn prepare_recovery_reset(
        &mut self,
        recovery_key_display: &str,
        recovery_wrapped_mk: &[u8],
        new_password: &str,
    ) -> Result<PreparedRecoveryReset, KeyStoreError> {
        let recovery_key =
            parse_recovery_key(recovery_key_display).map_err(KeyStoreError::from_recovery)?;
        let recovery_keys = derive_recovery_keys(&recovery_key)?;
        let master_key = unwrap_mk_with_recovery(&recovery_keys.rk_wrap, recovery_wrapped_mk)
            .map_err(KeyStoreError::from_recovery)?;

        let new_salt = generate_salt()?;
        let new_params = KdfParams::RECOMMENDED;
        let new_keys = derive_keys_from_password(new_password, &new_salt, &new_params)?;
        let new_params_json = recommended_params_json()?;
        let new_wrapped_mk = wrap_mk_with_password(&new_keys.pwd_wrap, &master_key)?;

        self.pending = Some(PendingOperation::RecoveryReset);
        Ok(PreparedRecoveryReset {
            recovery_auth_key: recovery_keys.rk_auth.expose_secret().to_vec(),
            new_auth_key: new_keys.auth_key.expose_secret().to_vec(),
            new_kdf_params_json: new_params_json,
            new_kdf_salt: new_salt.to_vec(),
            new_wrapped_mk,
        })
    }

    /// 轮换恢复密钥 prepare：当前 auth_key（证明知道当前密码）+ 新恢复材料。
    pub fn prepare_recovery_rotation(
        &mut self,
        current_password: &str,
        current_kdf_params_json: &str,
        current_salt: &[u8],
    ) -> Result<PreparedRecoveryRotation, KeyStoreError> {
        let current_salt: [u8; KDF_SALT_LEN] = current_salt
            .try_into()
            .map_err(|_| KeyStoreError::InvalidSalt)?;
        let current_params: KdfParams = parse_kdf_params(current_kdf_params_json)?;
        let current_keys =
            derive_keys_from_password(current_password, &current_salt, &current_params)?;

        let recovery_key = RecoveryKey::generate()?;
        let recovery_keys = derive_recovery_keys(&recovery_key)?;
        let recovery_wrapped_mk = {
            let master_key = self.require_master_key()?;
            wrap_mk_with_recovery(&recovery_keys.rk_wrap, master_key)?
        };
        let display = encode_recovery_key(&recovery_key);

        self.pending = Some(PendingOperation::RecoveryRotation);
        Ok(PreparedRecoveryRotation {
            current_auth_key: current_keys.auth_key.expose_secret().to_vec(),
            recovery_wrapped_mk,
            recovery_auth_key: recovery_keys.rk_auth.expose_secret().to_vec(),
            display,
        })
    }

    /// 服务端确认注册成功后，提交新 MK 与 pwd_wrap。
    pub fn commit_registration(&mut self) -> Result<(), KeyStoreError> {
        let Some(PendingOperation::Registration {
            master_key,
            pwd_wrap,
        }) = self.pending.take()
        else {
            return Err(KeyStoreError::PendingMissing);
        };
        self.master_key = Some(master_key);
        self.pwd_wrap = Some(pwd_wrap);
        Ok(())
    }

    /// 服务端确认改密成功后，替换 pwd_wrap；MK 保持不变。
    pub fn commit_password_change(&mut self) -> Result<(), KeyStoreError> {
        let Some(PendingOperation::PasswordChange { pwd_wrap }) = self.pending.take() else {
            return Err(KeyStoreError::PendingMissing);
        };
        self.pwd_wrap = Some(pwd_wrap);
        Ok(())
    }

    /// 恢复重置成功：丢弃待提交对象并保持锁定，由用户用新密码重新登录。
    pub fn commit_recovery_reset(&mut self) -> Result<(), KeyStoreError> {
        match self.pending.take() {
            Some(PendingOperation::RecoveryReset) => Ok(()),
            other => {
                self.pending = other;
                Err(KeyStoreError::PendingMissing)
            }
        }
    }

    /// 轮换恢复密钥成功：KeyStore 没有需要替换的恢复材料，清除待提交标记。
    pub fn commit_recovery_rotation(&mut self) -> Result<(), KeyStoreError> {
        match self.pending.take() {
            Some(PendingOperation::RecoveryRotation) => Ok(()),
            other => {
                self.pending = other;
                Err(KeyStoreError::PendingMissing)
            }
        }
    }

    /// 丢弃并清零待提交对象。
    pub fn discard_pending(&mut self) {
        self.pending = None;
    }

    /// 为节点生成新密钥、用 MK 包裹后返回密文，密钥留在 WASM 内。
    pub fn generate_node_key(&mut self, node_id: &str) -> Result<Vec<u8>, KeyStoreError> {
        let master_key = self.require_master_key()?;
        let node_id = NodeId::parse(node_id)?;
        let node_key = NodeKey::generate()?;
        let wrapped = wrap_node_key(master_key, &node_id, &node_key)?;
        self.node_keys.insert(node_id.as_str().to_owned(), node_key);
        Ok(wrapped)
    }

    /// 解包服务端返回的节点密钥并留在 WASM 内（刷新后按需加载）。
    ///
    /// 认证失败（node_id 与包裹密钥不匹配）映射为 `integrity_error`。
    pub fn load_node_key(&mut self, node_id: &str, wrapped: &[u8]) -> Result<(), KeyStoreError> {
        let master_key = self.require_master_key()?;
        let node_id = NodeId::parse(node_id)?;
        let node_key = unwrap_node_key(master_key, &node_id, wrapped)
            .map_err(KeyStoreError::from_integrity)?;
        self.node_keys.insert(node_id.as_str().to_owned(), node_key);
        Ok(())
    }

    /// 加密节点元数据（文件名），返回密文。
    pub fn encrypt_node_metadata(
        &mut self,
        node_id: &str,
        name: &str,
    ) -> Result<Vec<u8>, KeyStoreError> {
        let (node_id, node_key) = self.lookup_node_key(node_id)?;
        let metadata = NodeMetadata {
            name: name.to_owned(),
        };
        Ok(core_encrypt_node_metadata(node_key, &node_id, &metadata)?)
    }

    /// 解密节点元数据并返回文件名（明文只返回给 Worker/UI）。
    ///
    /// 认证失败（篡改或节点不匹配）映射为 `integrity_error`。
    pub fn decrypt_node_metadata(
        &mut self,
        node_id: &str,
        ciphertext: &[u8],
    ) -> Result<String, KeyStoreError> {
        let (node_id, node_key) = self.lookup_node_key(node_id)?;
        let metadata = core_decrypt_node_metadata(node_key, &node_id, ciphertext)
            .map_err(KeyStoreError::from_integrity)?;
        Ok(metadata.name)
    }

    /// 计算名称令牌（十六进制），服务端只比较令牌。
    pub fn name_token(&mut self, parent_id: &str, name: &str) -> Result<String, KeyStoreError> {
        let master_key = self.require_master_key()?;
        let parent_id = NodeId::parse(parent_id)?;
        let name_key = derive_name_key(master_key)?;
        let token = compute_name_token(&name_key, &parent_id, name)?;
        Ok(token.to_hex())
    }

    /// 加密版本元数据（含 content_id 与明文大小）。
    #[allow(clippy::too_many_arguments)]
    pub fn encrypt_version_metadata(
        &mut self,
        node_id: &str,
        version_id: &str,
        content_id: &str,
        size: f64,
        mtime: f64,
        mime: &str,
    ) -> Result<Vec<u8>, KeyStoreError> {
        let (_, node_key) = self.lookup_node_key(node_id)?;
        let version_id = VersionId::parse(version_id)?;
        let metadata = VersionMetadata {
            content_id: ContentId::parse(content_id)?,
            size: to_u64(size)?,
            mtime: to_i64(mtime)?,
            mime: mime.to_owned(),
        };
        Ok(core_encrypt_version_metadata(
            node_key,
            &version_id,
            &metadata,
        )?)
    }

    /// 解密版本元数据并校验它确实指向给定的 content（回滚防护）。
    ///
    /// 认证失败（篡改或版本/节点不匹配）映射为 `integrity_error`；
    /// content 绑定不符仍返回 `content_binding_mismatch`。
    pub fn decrypt_version_metadata_for_content(
        &mut self,
        node_id: &str,
        version_id: &str,
        content_id: &str,
        ciphertext: &[u8],
    ) -> Result<Ts<VersionMetadataView>, KeyStoreError> {
        let (_, node_key) = self.lookup_node_key(node_id)?;
        let version_id = VersionId::parse(version_id)?;
        let expected_content_id = ContentId::parse(content_id)?;

        let metadata = core_decrypt_version_metadata_for_content(
            node_key,
            &version_id,
            &expected_content_id,
            ciphertext,
        )
        .map_err(KeyStoreError::from_integrity)?;

        to_ts(&VersionMetadataView {
            content_id: metadata.content_id.as_str().to_owned(),
            size: metadata.size as f64,
            mtime: metadata.mtime as f64,
            mime: metadata.mime,
        })
    }

    /// 计算给定明文大小需要的块数（0 字节文件也有 1 块）。
    pub fn chunk_count(&self, size: f64) -> Result<u32, KeyStoreError> {
        Ok(crypto_core::chunk_count_for_size(to_u64(size)?)?)
    }

    /// 加密一块文件内容，返回可直传对象存储的密文。
    #[allow(clippy::too_many_arguments)]
    pub fn encrypt_chunk(
        &mut self,
        node_id: &str,
        content_id: &str,
        index: u32,
        count: u32,
        is_last: bool,
        plaintext: &[u8],
    ) -> Result<Vec<u8>, KeyStoreError> {
        let (_, node_key) = self.lookup_node_key(node_id)?;
        let content_id = ContentId::parse(content_id)?;
        let content_key = derive_content_key(node_key, &content_id)?;
        Ok(core_encrypt_chunk(
            &content_key,
            &content_id,
            index,
            count,
            is_last,
            plaintext,
        )?)
    }

    /// 解密一块文件内容，返回明文。
    ///
    /// 认证失败（篡改、重排、跨内容）映射为 `integrity_error`。
    #[allow(clippy::too_many_arguments)]
    pub fn decrypt_chunk(
        &mut self,
        node_id: &str,
        content_id: &str,
        index: u32,
        count: u32,
        is_last: bool,
        ciphertext: &[u8],
    ) -> Result<Vec<u8>, KeyStoreError> {
        let (_, node_key) = self.lookup_node_key(node_id)?;
        let content_id = ContentId::parse(content_id)?;
        let content_key = derive_content_key(node_key, &content_id)?;
        let plaintext =
            core_decrypt_chunk(&content_key, &content_id, index, count, is_last, ciphertext)
                .map_err(KeyStoreError::from_integrity)?;
        Ok(plaintext.to_vec())
    }

    /// 校验解密后的明文总长度是否等于版本元数据记录的大小。
    pub fn verify_plaintext_size(&self, expected: f64, actual: f64) -> Result<(), KeyStoreError> {
        let expected = to_u64(expected)?;
        let actual = to_u64(actual)?;
        if expected == actual {
            Ok(())
        } else {
            Err(KeyStoreError::Crypto(CryptoError::PlaintextSizeMismatch))
        }
    }
}

/// 把 JS number 安全转换为 u64（拒绝负数、小数、非有限值与超出安全整数范围的值）。
fn to_u64(value: f64) -> Result<u64, KeyStoreError> {
    if !value.is_finite() || value < 0.0 || value.fract() != 0.0 || value > MAX_SAFE_INTEGER {
        return Err(KeyStoreError::InvalidNumber);
    }
    Ok(value as u64)
}

/// 把 JS number 安全转换为 i64。
fn to_i64(value: f64) -> Result<i64, KeyStoreError> {
    if !value.is_finite() || value.fract() != 0.0 || value.abs() > MAX_SAFE_INTEGER {
        return Err(KeyStoreError::InvalidNumber);
    }
    Ok(value as i64)
}

#[cfg(test)]
mod tests;
