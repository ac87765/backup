//! 认证客户端公开 API。
//!
//! 界面层只拿到 [`Account`]（邮箱、状态、是否管理员等展示数据）；
//! 主密钥只保存在本结构的私有状态或系统凭据存储中。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use crypto_core::MasterKey;
use reqwest::Url;
use uuid::Uuid;

use crate::api::HttpClient;
use crate::auth::{self, LoginRequest, LoginResponse, PreloginResponse, ProfileResponse};
pub use crate::clock::{Clock, SleepFuture, SystemClock};
use crate::error::ClientError;
use crate::secret_store::{SecretStore, SessionToken, platform_secret_store};

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
}

impl ClientBuilder {
    /// 创建构造器；默认使用 [`SystemClock`] 与当前平台凭据存储。
    #[must_use]
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            store: None,
            clock: Arc::new(SystemClock),
            keep_unlocked: false,
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
        let api = HttpClient::new(url, self.clock)?;
        Ok(Client {
            api,
            store,
            keep_unlocked: AtomicBool::new(self.keep_unlocked),
            state: Mutex::new(None),
        })
    }
}

/// 原生客户端核心。
pub struct Client {
    api: HttpClient,
    store: Arc<dyn SecretStore>,
    keep_unlocked: AtomicBool,
    state: Mutex<Option<SessionState>>,
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
        let mut guard = self.state_guard()?;
        if let Some(state) = guard.as_mut() {
            state.master_key = None;
        }
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
