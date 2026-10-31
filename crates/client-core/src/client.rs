//! 认证客户端公开 API。
//!
//! 界面层只拿到 [`Account`]（邮箱、状态、是否管理员等展示数据）；
//! 主密钥只保存在本结构的私有状态或系统凭据存储中。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use crypto_core::{MasterKey, NodeKey};
use reqwest::Url;
use uuid::Uuid;

use crate::api::HttpClient;
use crate::auth::{self, LoginRequest, LoginResponse, PreloginResponse, ProfileResponse};
pub use crate::clock::{Clock, SleepFuture, SystemClock};
use crate::error::ClientError;
use crate::secret_store::{SecretStore, SessionToken, platform_secret_store};
use crate::state::StateStore;

/// 账号状态（与 user-accounts / admin-console spec 一致）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountStatus {
    /// 待管理员审核：可以登录，但不能写。
    Pending,
    /// 正常可用。
    Active,
    /// 已停用。
    Suspended,
    /// 已拒绝。
    Rejected,
    /// 服务端新增、客户端尚未识别的状态；按不可写处理。
    Unknown,
}

impl std::fmt::Display for AccountStatus {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl AccountStatus {
    pub(crate) fn from_wire(value: &str) -> Self {
        match value {
            "pending" => Self::Pending,
            // 缺失或空的状态不能当作可写账号。
            "active" => Self::Active,
            "suspended" => Self::Suspended,
            "rejected" => Self::Rejected,
            _ => Self::Unknown,
        }
    }

    /// 服务端使用的状态字符串。
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Active => "active",
            Self::Suspended => "suspended",
            Self::Rejected => "rejected",
            Self::Unknown => "unknown",
        }
    }

    /// 只有 `active` 账号可以创建/修改/删除。
    #[must_use]
    pub fn writes_allowed(self) -> bool {
        matches!(self, Self::Active)
    }
}

/// 服务端 KDF 参数的展示数据（`prelogin` 返回值）。
///
/// 只包含算法与代价参数，不含盐、密钥或任何可派生密钥的材料。界面上通常只在
/// “正在准备安全登录”之类的提示中显示进度，真正的派生仍在 Rust 内完成。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KdfParamsView {
    /// 算法标识，当前固定为 `argon2id`。
    pub algorithm: &'static str,
    /// 内存代价，单位 KiB。
    pub memory_kib: u32,
    /// 迭代次数。
    pub iterations: u32,
    /// 并行度。
    pub parallelism: u32,
}

/// 当前用户的展示数据；不包含任何密钥或可派生密钥的材料。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub user_id: Uuid,
    pub email: String,
    pub email_verified: bool,
    pub root_node_id: Uuid,
    pub status: AccountStatus,
    /// 停用/拒绝原因；其余状态为 `None`。
    pub status_reason: Option<String>,
    pub is_admin: bool,
}

impl Account {
    /// 待审核账号在界面上应显示等待说明，不提供上传/新建入口。
    #[must_use]
    pub fn writes_allowed(&self) -> bool {
        self.status.writes_allowed()
    }
}

/// 已解锁状态：`master_key` 为 `None` 表示只有会话、尚未输入密码解锁。
struct SessionState {
    account: Account,
    master_key: Option<MasterKey>,
}

/// 客户端构造器。
pub struct ClientBuilder {
    base_url: String,
    store: Option<Arc<dyn SecretStore>>,
    clock: Arc<dyn Clock>,
    keep_unlocked: bool,
    app_data_dir: Option<PathBuf>,
    state_store: Option<Arc<StateStore>>,
}

impl ClientBuilder {
    /// 创建构造器；默认使用 [`SystemClock`] 与当前平台凭据存储。
    ///
    /// 未指定应用数据目录时使用内存状态库（不会跨进程续传）。
    #[must_use]
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            store: None,
            clock: Arc::new(SystemClock),
            keep_unlocked: false,
            app_data_dir: None,
            state_store: None,
        }
    }

    /// 注入凭据存储（测试用内存实现，桌面用系统钥匙串）。
    #[must_use]
    pub fn secret_store(mut self, store: Arc<dyn SecretStore>) -> Self {
        self.store = Some(store);
        self
    }

    /// 注入退避时钟（测试用假时钟）。
    #[must_use]
    pub fn clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    /// 是否把主密钥写入系统钥匙串（“在这台电脑上保持解锁”，默认关闭）。
    #[must_use]
    pub fn keep_unlocked(mut self, enabled: bool) -> Self {
        self.keep_unlocked = enabled;
        self
    }

    /// 本地状态库所在的应用数据目录（design D5：由调用方传入）。
    ///
    /// 目录不存在时创建；数据库文件名为 [`crate::state::DATABASE_FILE_NAME`]。
    #[must_use]
    pub fn app_data_dir(mut self, directory: impl Into<PathBuf>) -> Self {
        self.app_data_dir = Some(directory.into());
        self
    }

    /// 注入已打开的状态库（测试或调用方自行处理损坏重建时使用）。
    #[must_use]
    pub fn state_store(mut self, store: Arc<StateStore>) -> Self {
        self.state_store = Some(store);
        self
    }

    /// 构造客户端。
    pub fn build(self) -> Result<Client, ClientError> {
        let mut url = Url::parse(&self.base_url).map_err(|_| ClientError::InvalidBaseUrl)?;
        if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
            return Err(ClientError::InvalidBaseUrl);
        }
        // 统一去掉查询/片段，避免把配置错误带进每个请求。
        url.set_query(None);
        url.set_fragment(None);

        let store = self.store.unwrap_or_else(platform_secret_store);
        let clock = self.clock;
        let api = Arc::new(HttpClient::new(url, clock.clone())?);
        let state_store = match (self.state_store, self.app_data_dir) {
            (Some(store), _) => store,
            (None, Some(directory)) => Arc::new(StateStore::open(&directory)?),
            (None, None) => Arc::new(StateStore::open_in_memory()?),
        };
        let storage_http = reqwest::Client::builder()
            // 预签名 URL 直连对象存储：绝不跟随重定向、绝不携带会话 Cookie。
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(std::time::Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .map_err(ClientError::Network)?;
        Ok(Client {
            api,
            store,
            clock,
            keep_unlocked: Arc::new(AtomicBool::new(self.keep_unlocked)),
            state: Arc::new(Mutex::new(None)),
            node_keys: Arc::new(Mutex::new(HashMap::new())),
            content_hashes: Arc::new(Mutex::new(HashMap::new())),
            state_store,
            storage_http,
        })
    }
}

/// 原生客户端核心。
///
/// `Clone` 共享会话、主密钥内存状态、节点密钥缓存与本地状态库；用于把传输
/// 任务交给 `tokio::spawn`，而不是复制凭据。
#[derive(Clone)]
pub struct Client {
    pub(crate) api: Arc<HttpClient>,
    pub(crate) store: Arc<dyn SecretStore>,
    pub(crate) clock: Arc<dyn Clock>,
    keep_unlocked: Arc<AtomicBool>,
    state: Arc<Mutex<Option<SessionState>>>,
    /// node_id → 解包后的节点密钥；只在 Rust 内存中，不跨桥接层。
    node_keys: Arc<Mutex<HashMap<Uuid, NodeKey>>>,
    /// node_id → 当前版本的明文内容 SHA-256（仅用于增量判定）。
    content_hashes: Arc<Mutex<HashMap<Uuid, Option<String>>>>,
    pub(crate) state_store: Arc<StateStore>,
    /// 直连 OSS/MinIO 的 HTTP 客户端（不带任何会话 Cookie/CSRF 头）。
    pub(crate) storage_http: reqwest::Client,
}

impl Client {
    /// 创建构造器。
    #[must_use]
    pub fn builder(base_url: impl Into<String>) -> ClientBuilder {
        ClientBuilder::new(base_url)
    }

    /// 使用指定凭据存储创建客户端。
    pub fn new(
        base_url: impl Into<String>,
        store: Arc<dyn SecretStore>,
    ) -> Result<Self, ClientError> {
        Self::builder(base_url).secret_store(store).build()
    }

    /// 使用当前平台默认凭据存储创建客户端。
    pub fn new_with_platform_store(base_url: impl Into<String>) -> Result<Self, ClientError> {
        Self::builder(base_url).build()
    }

    /// 当前是否已解锁（内存中有主密钥）。
    #[must_use]
    pub fn is_unlocked(&self) -> bool {
        self.state
            .lock()
            .ok()
            .and_then(|state| state.as_ref().map(|state| state.master_key.is_some()))
            .unwrap_or(false)
    }

    /// 当前已加载的展示数据（即使尚未输入密码解锁也会返回）。
    #[must_use]
    pub fn account(&self) -> Option<Account> {
        self.state
            .lock()
            .ok()
            .and_then(|state| state.as_ref().map(|state| state.account.clone()))
    }

    /// 是否开启“在这台电脑上保持解锁”。
    #[must_use]
    pub fn keep_unlocked(&self) -> bool {
        self.keep_unlocked.load(Ordering::SeqCst)
    }

    /// 修改“保持解锁”设置。
    ///
    /// - 关闭：立即删除钥匙串中的主密钥；
    /// - 开启且当前已解锁：立即把主密钥写入钥匙串。
    pub fn set_keep_unlocked(&self, enabled: bool) -> Result<(), ClientError> {
        if enabled {
            self.keep_unlocked.store(true, Ordering::SeqCst);
            let key = {
                let guard = self.state_guard()?;
                guard.as_ref().and_then(|state| state.master_key.clone())
            };
            if let Some(key) = key
                && let Err(error) = self.store.store_master_key(&key)
            {
                self.keep_unlocked.store(false, Ordering::SeqCst);
                return Err(ClientError::SecretStore(error));
            }
            Ok(())
        } else {
            self.store.delete_master_key()?;
            self.keep_unlocked.store(false, Ordering::SeqCst);
            Ok(())
        }
    }

    /// 查询服务端下发的 KDF 参数（登录前预检）。
    ///
    /// 只返回算法与代价参数（展示数据），不返回盐或任何密钥材料；真正的密码
    /// 派生仍由 [`Client::login`] / [`Client::unlock`] 在 Rust 内完成。界面可以
    /// 在登录按钮上显示“正在准备安全登录”，或据此提示需要升级客户端。
    pub async fn prelogin(&self, email: &str) -> Result<KdfParamsView, ClientError> {
        let prelogin: PreloginResponse = self
            .api
            .post_json(
                "/api/v1/auth/prelogin",
                &serde_json::json!({ "email": email }),
            )
            .await?;
        let params = auth::parse_kdf_params(&prelogin.kdf_params)?;
        Ok(KdfParamsView {
            algorithm: "argon2id",
            memory_kib: params.m_cost_kib,
            iterations: params.t_cost,
            parallelism: params.p_cost,
        })
    }

    /// 邮箱 + 密码登录（设计 D12a：派生、认证与解包都在 Rust 核心内完成）。
    ///
    /// 成功后会话令牌写入凭据存储；主密钥默认只留在内存。
    pub async fn login(&self, email: &str, password: &str) -> Result<Account, ClientError> {
        // 新登录代表新账号：先清掉旧会话与旧主密钥，避免跨账号残留。
        self.clear_local_credentials()?;

        let prelogin: PreloginResponse = self
            .api
            .post_json(
                "/api/v1/auth/prelogin",
                &serde_json::json!({ "email": email }),
            )
            .await?;
        let params = auth::parse_kdf_params(&prelogin.kdf_params)?;
        let salt = auth::decode_kdf_salt(&prelogin.kdf_salt)?;

        let keys = auth::derive_password_keys(password, salt, params).await?;
        let auth_key = auth::auth_key_hex(&keys);
        let request = LoginRequest {
            email,
            auth_key: auth_key.as_str(),
        };
        let (login, token) = self
            .api
            .post_json_capture_session::<_, LoginResponse>("/api/v1/auth/login", &request)
            .await?;
        drop(auth_key);

        let token = token.ok_or(ClientError::InvalidResponse("登录响应未设置会话 Cookie"))?;

        // login 与 prelogin 必须是同一套 KDF 参数，否则 pwd_wrap 会解错。
        if auth::parse_kdf_params(&login.kdf_params)? != params
            || auth::decode_kdf_salt(&login.kdf_salt)? != salt
        {
            return Err(ClientError::InvalidResponse(
                "登录响应的 KDF 参数与 prelogin 不一致",
            ));
        }

        let wrapped_mk = auth::decode_wrapped_mk(&login.wrapped_mk)?;
        let master_key = auth::unwrap_master_key(&keys, &wrapped_mk)?;

        self.api.set_session(Some(token.clone()));
        let profile = match self
            .api
            .get_json::<ProfileResponse>("/api/v1/auth/me")
            .await
        {
            Ok(profile) => profile,
            Err(error) => {
                self.api.clear_session();
                return Err(error);
            }
        };

        let account = match validate_profile_kdf(&profile, params, salt) {
            Ok(account) => account,
            Err(error) => {
                self.api.clear_session();
                return Err(error);
            }
        };

        if let Err(error) = self.store.store_session(&token) {
            self.api.clear_session();
            return Err(ClientError::SecretStore(error));
        }
        if self.keep_unlocked()
            && let Err(error) = self.store.store_master_key(&master_key)
        {
            let _ = self.store.delete_session();
            self.api.clear_session();
            return Err(ClientError::SecretStore(error));
        }

        self.set_state(account.clone(), Some(master_key))?;
        Ok(account)
    }

    /// 仅用密码解锁（已存会话令牌时；对应“重新打开应用”场景）。
    ///
    /// 不发送认证密钥：从 `/auth/me` 取得 KDF 参数/盐/包裹 MK，
    /// 在本地派生 pwd_wrap 并解包；解包失败即密码错误。
    pub async fn unlock(&self, password: &str) -> Result<Account, ClientError> {
        let token = self.store.load_session()?.ok_or(ClientError::NoSession)?;
        self.api.set_session(Some(token));

        let profile = match self
            .api
            .get_json::<ProfileResponse>("/api/v1/auth/me")
            .await
        {
            Ok(profile) => profile,
            Err(error) if error.is_unauthenticated() => {
                self.clear_after_invalid_session();
                return Err(error);
            }
            Err(error) => {
                self.api.clear_session();
                return Err(error);
            }
        };

        let params = auth::parse_kdf_params(&profile.kdf_params)?;
        let salt = auth::decode_kdf_salt(&profile.kdf_salt)?;
        let wrapped_mk = auth::decode_wrapped_mk(&profile.wrapped_mk)?;
        let keys = auth::derive_password_keys(password, salt, params).await?;
        let master_key = auth::unwrap_master_key(&keys, &wrapped_mk)?;

        let account = auth::account_from_profile(&profile);
        if self.keep_unlocked()
            && let Err(error) = self.store.store_master_key(&master_key)
        {
            self.api.clear_session();
            return Err(ClientError::SecretStore(error));
        }

        self.set_state(account.clone(), Some(master_key))?;
        Ok(account)
    }

    /// 如果钥匙串里同时有会话令牌与主密钥（“保持解锁”），直接恢复；
    /// 否则返回 `None`，调用方应显示解锁页。
    pub async fn unlock_with_stored_master_key(&self) -> Result<Option<Account>, ClientError> {
        let Some(token) = self.store.load_session()? else {
            return Ok(None);
        };
        let Some(master_key) = self.store.load_master_key()? else {
            return Ok(None);
        };
        self.api.set_session(Some(token));

        match self
            .api
            .get_json::<ProfileResponse>("/api/v1/auth/me")
            .await
        {
            Ok(profile) => {
                let account = auth::account_from_profile(&profile);
                self.set_state(account.clone(), Some(master_key))?;
                Ok(Some(account))
            }
            Err(error) if error.is_unauthenticated() => {
                self.clear_after_invalid_session();
                Ok(None)
            }
            Err(error) => {
                self.api.clear_session();
                Err(error)
            }
        }
    }

    /// 用已存会话令牌恢复展示数据（不输入密码）。
    ///
    /// 用于重新打开应用后显示带邮箱的解锁页；会话失效时清除本地凭据并返回 `None`。
    pub async fn restore_session(&self) -> Result<Option<Account>, ClientError> {
        let Some(token) = self.store.load_session()? else {
            return Ok(None);
        };
        self.api.set_session(Some(token));

        match self
            .api
            .get_json::<ProfileResponse>("/api/v1/auth/me")
            .await
        {
            Ok(profile) => {
                let account = auth::account_from_profile(&profile);
                // 只刷新展示数据；如果此前已经解锁，保留内存中的主密钥。
                self.set_account_preserving_key(account.clone())?;
                Ok(Some(account))
            }
            Err(error) if error.is_unauthenticated() => {
                self.clear_after_invalid_session();
                Ok(None)
            }
            Err(error) => {
                self.api.clear_session();
                Err(error)
            }
        }
    }

    /// 请求 `/auth/me` 并用服务端最新状态更新本地展示数据。
    ///
    /// 服务端返回会话失效（401 / `unauthenticated`）时，清除内存与凭据存储中的
    /// 会话和主密钥。
    pub async fn current_account(&self) -> Result<Account, ClientError> {
        let Some(token) = self.session_token()? else {
            return Err(ClientError::NoSession);
        };
        self.api.set_session(Some(token));

        match self
            .api
            .get_json::<ProfileResponse>("/api/v1/auth/me")
            .await
        {
            Ok(profile) => {
                let account = auth::account_from_profile(&profile);
                self.set_account_preserving_key(account.clone())?;
                Ok(account)
            }
            Err(error) if error.is_unauthenticated() => {
                self.clear_after_invalid_session();
                Err(error)
            }
            Err(error) => Err(error),
        }
    }

    /// 是否存在已保存的会话令牌。
    pub fn has_stored_session(&self) -> Result<bool, ClientError> {
        Ok(self.session_token()?.is_some())
    }

    /// 取当前主密钥用于本 crate 的加解密。
    ///
    /// `pub(crate)` 且返回克隆：密钥只在 Rust 内部流动，绝不经过公开 API。
    pub(crate) fn master_key(&self) -> Result<MasterKey, ClientError> {
        let guard = self.state_guard()?;
        let state = guard.as_ref().ok_or(ClientError::NoSession)?;
        state.master_key.clone().ok_or(ClientError::Locked)
    }

    /// 写操作前的客户端检查：只有 `active` 账号允许写。
    pub(crate) fn ensure_writable(&self) -> Result<Account, ClientError> {
        let guard = self.state_guard()?;
        let state = guard.as_ref().ok_or(ClientError::NoSession)?;
        if !state.account.writes_allowed() {
            return Err(ClientError::AccountNotWritable(state.account.status));
        }
        Ok(state.account.clone())
    }

    /// 内部缓存一个已解包的节点密钥。
    pub(crate) fn cache_node_key(&self, node_id: Uuid, key: NodeKey) -> Result<(), ClientError> {
        let mut keys = self
            .node_keys
            .lock()
            .map_err(|_| ClientError::Internal("节点密钥缓存锁已损坏"))?;
        keys.insert(node_id, key);
        Ok(())
    }

    /// 读取缓存的节点密钥（克隆后交给调用方清零）。
    pub(crate) fn cached_node_key(&self, node_id: Uuid) -> Result<Option<NodeKey>, ClientError> {
        let keys = self
            .node_keys
            .lock()
            .map_err(|_| ClientError::Internal("节点密钥缓存锁已损坏"))?;
        Ok(keys.get(&node_id).cloned())
    }

    /// 缓存当前版本的明文内容 SHA-256（仅用于增量比较，不跨桥接层）。
    pub(crate) fn cache_content_hash(
        &self,
        node_id: Uuid,
        sha256: Option<String>,
    ) -> Result<(), ClientError> {
        let mut hashes = self
            .content_hashes
            .lock()
            .map_err(|_| ClientError::Internal("内容摘要缓存锁已损坏"))?;
        hashes.insert(node_id, sha256);
        Ok(())
    }

    /// 退出登录：通知服务端撤销当前会话，并**无论请求是否成功**都清除本地
    /// 会话令牌与主密钥（design D3）。
    pub async fn logout(&self) -> Result<(), ClientError> {
        let request = if self.session_token()?.is_some() {
            self.api
                .post_empty::<serde_json::Value>("/api/v1/auth/logout")
                .await
                .map(|_| ())
        } else {
            Ok(())
        };

        let cleanup = self.clear_local_credentials();
        match (request, cleanup) {
            (Err(error), _) => Err(error),
            (Ok(()), Err(error)) => Err(error),
            (Ok(()), Ok(())) => Ok(()),
        }
    }

    /// 只清除内存中的主密钥，保留会话令牌与钥匙串内容（临时锁定）。
    pub fn lock(&self) -> Result<(), ClientError> {
        {
            let mut guard = self.state_guard()?;
            if let Some(state) = guard.as_mut() {
                state.master_key = None;
            }
        }
        self.clear_key_caches()
    }

    fn clear_key_caches(&self) -> Result<(), ClientError> {
        self.node_keys
            .lock()
            .map_err(|_| ClientError::Internal("节点密钥缓存锁已损坏"))?
            .clear();
        self.content_hashes
            .lock()
            .map_err(|_| ClientError::Internal("内容摘要缓存锁已损坏"))?
            .clear();
        Ok(())
    }

    /// 清除本地全部凭据（退出登录、修改密码、会话失效时使用）。
    ///
    /// 修改密码流程（后续组实现）在服务端成功或失败后都必须调用它清除
    /// 旧的会话令牌与主密钥；本组先提供这一唯一入口。
    pub fn forget_credentials(&self) -> Result<(), ClientError> {
        self.clear_local_credentials()
    }

    fn session_token(&self) -> Result<Option<SessionToken>, ClientError> {
        if let Some(token) = self.api.current_session() {
            return Ok(Some(token));
        }
        let token = self.store.load_session()?;
        if let Some(token) = &token {
            self.api.set_session(Some(token.clone()));
        }
        Ok(token)
    }

    fn state_guard(&self) -> Result<MutexGuard<'_, Option<SessionState>>, ClientError> {
        self.state
            .lock()
            .map_err(|_| ClientError::Internal("状态锁已损坏"))
    }

    fn set_state(
        &self,
        account: Account,
        master_key: Option<MasterKey>,
    ) -> Result<(), ClientError> {
        *self.state_guard()? = Some(SessionState {
            account,
            master_key,
        });
        Ok(())
    }

    fn set_account_preserving_key(&self, account: Account) -> Result<(), ClientError> {
        let mut guard = self.state_guard()?;
        let master_key = guard.as_ref().and_then(|state| state.master_key.clone());
        *guard = Some(SessionState {
            account,
            master_key,
        });
        Ok(())
    }

    fn clear_local_credentials(&self) -> Result<(), ClientError> {
        self.api.clear_session();
        {
            let mut guard = self.state_guard()?;
            *guard = None;
        }
        self.clear_key_caches()?;
        self.store.clear()?;
        Ok(())
    }

    /// 会话失效时清除凭据；清理失败只记日志，仍把 401 返回给调用方。
    fn clear_after_invalid_session(&self) {
        if let Err(error) = self.clear_local_credentials() {
            tracing::warn!(error = %error, "会话失效后清除本地凭据失败");
        }
    }
}

/// 校验 `/auth/me` 返回的 KDF 参数与登录时一致，并转换为展示数据。
fn validate_profile_kdf(
    profile: &ProfileResponse,
    params: crypto_core::KdfParams,
    salt: [u8; crypto_core::KDF_SALT_LEN],
) -> Result<Account, ClientError> {
    if auth::parse_kdf_params(&profile.kdf_params)? != params
        || auth::decode_kdf_salt(&profile.kdf_salt)? != salt
    {
        return Err(ClientError::InvalidResponse(
            "/auth/me 的 KDF 参数与登录时不一致",
        ));
    }
    Ok(auth::account_from_profile(profile))
}
