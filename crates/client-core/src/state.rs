//! 本地 SQLite 状态库（design D5）。
//!
//! 只保存**本机元数据**：传输会话 ID、目标 ID、本地路径、大小、修改时间与
//! 已确认分块。绝不保存主密钥、节点密钥、加密元数据、名称令牌、预签名 URL
//! 或文件内容。
//!
//! 迁移用 `PRAGMA user_version` 记录版本；每次升级只追加一个迁移函数。
//! 数据库损坏时调用方可以显式调用 [`StateStore::recreate`] 删除并重建
//! （未完成上传会丢失本地记录，但服务端会话会按 24 小时过期清理）。

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, params};
use uuid::Uuid;

use crate::io_path::{StatePathError, path_for_io};
use crate::model::{PendingUpload, PendingUploadStatus};

/// 当前 schema 版本。
pub const SCHEMA_VERSION: i64 = 3;
/// 状态库文件名（位于调用方传入的应用数据目录）。
pub const DATABASE_FILE_NAME: &str = "client-state.sqlite3";

/// 本地状态库错误。
#[derive(Debug, thiserror::Error)]
pub enum StateError {
    #[error("无法创建本地状态目录")]
    Io(#[from] std::io::Error),
    #[error("本地状态库操作失败")]
    Sqlite(#[from] rusqlite::Error),
    #[error("本地状态库已损坏：{0}")]
    Corrupt(&'static str),
    #[error("本地状态库路径不是有效 UTF-8")]
    PathEncoding,
    #[error("本地状态库锁已损坏")]
    LockPoisoned,
}

impl From<StatePathError> for StateError {
    fn from(_: StatePathError) -> Self {
        Self::PathEncoding
    }
}

/// 上传类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UploadMode {
    NewFile,
    NewVersion,
}

impl UploadMode {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::NewFile => "new_file",
            Self::NewVersion => "new_version",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "new_file" => Some(Self::NewFile),
            "new_version" => Some(Self::NewVersion),
            _ => None,
        }
    }
}

/// 一条待续传上传记录（内部字段与数据库一一对应）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UploadRecord {
    pub session_id: Uuid,
    pub user_id: Uuid,
    pub local_path: PathBuf,
    pub mode: UploadMode,
    pub parent_id: Option<Uuid>,
    pub node_id: Uuid,
    pub content_id: Uuid,
    pub version_id: Uuid,
    pub chunk_count: u32,
    pub declared_size: u64,
    pub file_size: u64,
    pub file_mtime_ns: i64,
    pub status: PendingUploadStatus,
}

impl UploadRecord {
    pub(crate) fn into_pending(self) -> PendingUpload {
        PendingUpload {
            session_id: self.session_id,
            parent_id: self.parent_id,
            node_id: self.node_id,
            content_id: self.content_id,
            version_id: self.version_id,
            local_path: self.local_path,
            file_size: self.file_size,
            file_mtime_ns: self.file_mtime_ns,
            chunk_count: self.chunk_count,
            declared_size: self.declared_size,
            status: self.status,
        }
    }
}

/// 已确认分块。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConfirmedChunk {
    pub idx: u32,
    pub declared_size: i64,
    pub sha256: String,
}

/// 备份文件夹配置（design D5/D6）。只存本机元数据，不含任何密钥。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BackupFolderRecord {
    pub id: Uuid,
    pub user_id: Uuid,
    pub local_path: PathBuf,
    /// 云端显示用的本地文件夹名（NFC）。
    pub local_name: String,
    /// 用户指定的云端目标文件夹节点；`None` 表示使用默认路径。
    pub cloud_target_id: Option<Uuid>,
    /// 已解析的云端叶子文件夹节点；首次备份前为 `None`。
    pub cloud_folder_id: Option<Uuid>,
    /// 默认目标的展示路径（`全部文件/电脑备份/<设备名>/<名称>`）；自定义目标为 `None`。
    pub cloud_path: Option<String>,
    pub exclude_globs: Vec<String>,
    pub exclude_subfolders: Vec<String>,
    pub enabled: bool,
    pub last_backup_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// 本地增量索引：大小/修改时间未变即可跳过，不读文件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BackupIndexRecord {
    pub user_id: Uuid,
    pub folder_id: Uuid,
    pub relative_path: String,
    pub size: u64,
    pub mtime_ns: i64,
    pub cloud_node_id: Uuid,
    pub content_sha256: String,
    pub updated_at: i64,
}

/// 最近活动（只存本机；界面展示文件相对路径）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BackupActivityRecord {
    pub id: i64,
    pub user_id: Uuid,
    pub folder_id: Uuid,
    pub relative_path: String,
    pub kind: String,
    pub occurred_at: i64,
    pub message: Option<String>,
}

/// SQLite 状态库。`Clone` 共享同一个连接（连接由 `Mutex` 串行化）。
#[derive(Clone)]
pub struct StateStore {
    connection: Arc<Mutex<Connection>>,
    location: Option<PathBuf>,
}

impl std::fmt::Debug for StateStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StateStore")
            .field("location", &self.location)
            .field("schema_version", &SCHEMA_VERSION)
            .finish()
    }
}

impl StateStore {
    /// 在应用数据目录打开（不存在时创建）状态库并执行迁移。
    pub fn open(app_data_dir: &Path) -> Result<Self, StateError> {
        std::fs::create_dir_all(path_for_io(app_data_dir)?)?;
        let path = app_data_dir.join(DATABASE_FILE_NAME);
        Self::open_path(&path)
    }

    /// 内存状态库：仅用于测试或调用方明确不需要重启续传的场景。
    pub fn open_in_memory() -> Result<Self, StateError> {
        let connection = Connection::open_in_memory()?;
        Self::initialize(connection, None)
    }

    /// 打开指定文件。
    pub fn open_path(path: &Path) -> Result<Self, StateError> {
        let io_path = path_for_io(path)?;
        let connection = Connection::open(io_path)?;
        Self::initialize(connection, Some(path.to_path_buf()))
    }

    /// 删除并重建状态库。仅在调用方确认数据库损坏、愿意放弃未完成上传记录时使用。
    pub fn recreate(app_data_dir: &Path) -> Result<Self, StateError> {
        std::fs::create_dir_all(path_for_io(app_data_dir)?)?;
        let path = app_data_dir.join(DATABASE_FILE_NAME);
        for suffix in ["", "-wal", "-shm"] {
            let target = if suffix.is_empty() {
                path.clone()
            } else {
                PathBuf::from(format!("{}{suffix}", path.display()))
            };
            match std::fs::remove_file(path_for_io(&target)?) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(StateError::Io(error)),
            }
        }
        Self::open_path(&path)
    }

    fn initialize(connection: Connection, location: Option<PathBuf>) -> Result<Self, StateError> {
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        connection.execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA journal_mode = WAL;
             PRAGMA synchronous = FULL;",
        )?;

        let integrity: String =
            connection.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
        if integrity != "ok" {
            return Err(StateError::Corrupt("integrity_check 未通过"));
        }

        let store = Self {
            connection: Arc::new(Mutex::new(connection)),
            location,
        };
        store.migrate()?;
        Ok(store)
    }

    fn lock(&self) -> Result<MutexGuard<'_, Connection>, StateError> {
        self.connection.lock().map_err(|_| StateError::LockPoisoned)
    }

    /// 当前 schema 版本。
    pub fn schema_version(&self) -> Result<i64, StateError> {
        let connection = self.lock()?;
        Ok(connection.pragma_query_value(None, "user_version", |row| row.get(0))?)
    }

    fn migrate(&self) -> Result<(), StateError> {
        let mut connection = self.lock()?;
        let current: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if current > SCHEMA_VERSION {
            return Err(StateError::Corrupt("schema 版本高于当前客户端"));
        }
        if current < 1 {
            let tx = connection.transaction()?;
            tx.execute_batch(
                r#"
                CREATE TABLE pending_uploads (
                    session_id      TEXT PRIMARY KEY NOT NULL,
                    user_id         TEXT NOT NULL,
                    local_path      TEXT NOT NULL,
                    mode            TEXT NOT NULL CHECK (mode IN ('new_file', 'new_version')),
                    parent_id       TEXT,
                    node_id         TEXT NOT NULL,
                    content_id      TEXT NOT NULL,
                    version_id      TEXT NOT NULL,
                    chunk_count     INTEGER NOT NULL CHECK (chunk_count > 0),
                    declared_size   INTEGER NOT NULL CHECK (declared_size >= 0),
                    file_size       INTEGER NOT NULL CHECK (file_size >= 0),
                    file_mtime_ns   INTEGER NOT NULL,
                    status          TEXT NOT NULL CHECK (status IN ('uploading', 'paused', 'failed')),
                    created_at      INTEGER NOT NULL,
                    updated_at      INTEGER NOT NULL
                );

                CREATE INDEX pending_uploads_user_idx
                    ON pending_uploads (user_id, status, updated_at);

                CREATE TABLE confirmed_chunks (
                    session_id      TEXT NOT NULL,
                    idx             INTEGER NOT NULL,
                    declared_size   INTEGER NOT NULL CHECK (declared_size >= 0),
                    sha256          TEXT NOT NULL,
                    confirmed_at    TEXT NOT NULL,
                    PRIMARY KEY (session_id, idx),
                    FOREIGN KEY (session_id)
                        REFERENCES pending_uploads (session_id) ON DELETE CASCADE
                );

                PRAGMA user_version = 1;
                "#,
            )?;
            tx.commit()?;
        }
        if current < 2 {
            // 第 3 组：桌面备份文件夹、增量索引、最近活动与全局设置。
            // 仍然只存本机元数据；路径属于用户本机信息，设计 D5 已说明在
            // “安全与隐私”中告知用户。云端名称仍是密文。
            let tx = connection.transaction()?;
            tx.execute_batch(
                r#"
                CREATE TABLE backup_folders (
                    id                  TEXT PRIMARY KEY NOT NULL,
                    user_id             TEXT NOT NULL,
                    local_path          TEXT NOT NULL,
                    local_name          TEXT NOT NULL,
                    cloud_target_id     TEXT,
                    cloud_folder_id     TEXT,
                    cloud_path          TEXT,
                    exclude_globs       TEXT NOT NULL DEFAULT '[]',
                    exclude_subfolders  TEXT NOT NULL DEFAULT '[]',
                    enabled             INTEGER NOT NULL DEFAULT 1,
                    last_backup_at      INTEGER,
                    created_at          INTEGER NOT NULL,
                    updated_at          INTEGER NOT NULL,
                    UNIQUE (user_id, local_path)
                );

                CREATE INDEX backup_folders_user_idx
                    ON backup_folders (user_id, enabled);

                CREATE TABLE backup_index (
                    user_id         TEXT NOT NULL,
                    folder_id       TEXT NOT NULL,
                    relative_path   TEXT NOT NULL,
                    size            INTEGER NOT NULL CHECK (size >= 0),
                    mtime_ns        INTEGER NOT NULL,
                    cloud_node_id   TEXT NOT NULL,
                    content_sha256  TEXT NOT NULL,
                    updated_at      INTEGER NOT NULL,
                    PRIMARY KEY (folder_id, relative_path),
                    FOREIGN KEY (folder_id)
                        REFERENCES backup_folders (id) ON DELETE CASCADE
                );

                CREATE INDEX backup_index_user_idx
                    ON backup_index (user_id, folder_id);

                CREATE TABLE backup_activity (
                    id              INTEGER PRIMARY KEY AUTOINCREMENT,
                    user_id         TEXT NOT NULL,
                    folder_id       TEXT NOT NULL,
                    relative_path   TEXT NOT NULL,
                    kind            TEXT NOT NULL,
                    occurred_at     INTEGER NOT NULL,
                    message         TEXT,
                    FOREIGN KEY (folder_id)
                        REFERENCES backup_folders (id) ON DELETE CASCADE
                );

                CREATE INDEX backup_activity_user_idx
                    ON backup_activity (user_id, id DESC);

                CREATE TABLE backup_settings (
                    user_id     TEXT NOT NULL,
                    key         TEXT NOT NULL,
                    value       TEXT NOT NULL,
                    updated_at  INTEGER NOT NULL,
                    PRIMARY KEY (user_id, key)
                );

                PRAGMA user_version = 2;
                "#,
            )?;
            tx.commit()?;
        }
        if current < 3 {
            // 微信备份（add-wechat-backup 第 1 组）：账号本地备注名。
            // 键是数据根目录 + 账号目录名；不含 user_id（这是本机目录的显示
            // 备注，与云端账号无关），也不含任何聊天内容或文件内容。
            let tx = connection.transaction()?;
            tx.execute_batch(
                r#"
                CREATE TABLE wechat_accounts (
                    data_root       TEXT NOT NULL,
                    account_name    TEXT NOT NULL,
                    note            TEXT NOT NULL,
                    updated_at      INTEGER NOT NULL,
                    PRIMARY KEY (data_root, account_name)
                );

                PRAGMA user_version = 3;
                "#,
            )?;
            tx.commit()?;
        }
        Ok(())
    }

    /// 插入或覆盖一条上传记录。
    pub(crate) async fn upsert_upload(&self, record: UploadRecord) -> Result<(), StateError> {
        let store = self.clone();
        tokio::task::spawn_blocking(move || store.upsert_upload_blocking(&record))
            .await
            .map_err(|_| StateError::LockPoisoned)?
    }

    fn upsert_upload_blocking(&self, record: &UploadRecord) -> Result<(), StateError> {
        let path = record.local_path.to_str().ok_or(StateError::PathEncoding)?;
        let now = unix_seconds();
        let connection = self.lock()?;
        connection.execute(
            r#"
            INSERT INTO pending_uploads (
                session_id, user_id, local_path, mode, parent_id, node_id,
                content_id, version_id, chunk_count, declared_size, file_size,
                file_mtime_ns, status, created_at, updated_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?14)
            ON CONFLICT (session_id) DO UPDATE SET
                user_id = excluded.user_id,
                local_path = excluded.local_path,
                mode = excluded.mode,
                parent_id = excluded.parent_id,
                node_id = excluded.node_id,
                content_id = excluded.content_id,
                version_id = excluded.version_id,
                chunk_count = excluded.chunk_count,
                declared_size = excluded.declared_size,
                file_size = excluded.file_size,
                file_mtime_ns = excluded.file_mtime_ns,
                status = excluded.status,
                updated_at = excluded.updated_at
            "#,
            params![
                record.session_id.to_string(),
                record.user_id.to_string(),
                path,
                record.mode.as_str(),
                record.parent_id.map(|id| id.to_string()),
                record.node_id.to_string(),
                record.content_id.to_string(),
                record.version_id.to_string(),
                i64::from(record.chunk_count),
                i64::try_from(record.declared_size).unwrap_or(i64::MAX),
                i64::try_from(record.file_size).unwrap_or(i64::MAX),
                record.file_mtime_ns,
                record.status.as_str(),
                now,
            ],
        )?;
        Ok(())
    }

    /// 更新状态（uploading / paused / failed）。
    pub(crate) async fn set_status(
        &self,
        session_id: Uuid,
        status: PendingUploadStatus,
    ) -> Result<(), StateError> {
        let store = self.clone();
        tokio::task::spawn_blocking(move || {
            let connection = store.lock()?;
            connection.execute(
                "UPDATE pending_uploads SET status = ?2, updated_at = ?3 WHERE session_id = ?1",
                params![session_id.to_string(), status.as_str(), unix_seconds()],
            )?;
            Ok::<(), StateError>(())
        })
        .await
        .map_err(|_| StateError::LockPoisoned)?
    }

    /// 删除上传记录与已确认分块（级联）。
    pub(crate) async fn delete_upload(&self, session_id: Uuid) -> Result<(), StateError> {
        let store = self.clone();
        tokio::task::spawn_blocking(move || {
            let connection = store.lock()?;
            connection.execute(
                "DELETE FROM pending_uploads WHERE session_id = ?1",
                params![session_id.to_string()],
            )?;
            Ok::<(), StateError>(())
        })
        .await
        .map_err(|_| StateError::LockPoisoned)?
    }

    /// 按会话 ID 读取一条上传记录。
    pub(crate) async fn get_upload(
        &self,
        session_id: Uuid,
    ) -> Result<Option<UploadRecord>, StateError> {
        let store = self.clone();
        tokio::task::spawn_blocking(move || {
            let connection = store.lock()?;
            let row = connection
                .query_row(
                    r#"
                    SELECT session_id, user_id, local_path, mode, parent_id, node_id,
                           content_id, version_id, chunk_count, declared_size,
                           file_size, file_mtime_ns, status
                    FROM pending_uploads
                    WHERE session_id = ?1
                    "#,
                    params![session_id.to_string()],
                    row_to_record,
                )
                .optional()?;
            match row {
                Some(row) => Ok(Some(row?)),
                None => Ok(None),
            }
        })
        .await
        .map_err(|_| StateError::LockPoisoned)?
    }

    /// 列出某用户未完成的上传（新到旧）。
    pub(crate) async fn list_uploads(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<UploadRecord>, StateError> {
        let store = self.clone();
        tokio::task::spawn_blocking(move || {
            let connection = store.lock()?;
            let mut statement = connection.prepare(
                r#"
                SELECT session_id, user_id, local_path, mode, parent_id, node_id,
                       content_id, version_id, chunk_count, declared_size,
                       file_size, file_mtime_ns, status
                FROM pending_uploads
                WHERE user_id = ?1
                ORDER BY updated_at DESC
                "#,
            )?;
            let rows = statement.query_map(params![user_id.to_string()], row_to_record)?;
            let mut records = Vec::new();
            for row in rows {
                records.push(row??);
            }
            Ok::<_, StateError>(records)
        })
        .await
        .map_err(|_| StateError::LockPoisoned)?
    }

    /// 查找精确匹配的上传记录（用于自动续传）。
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn find_upload(
        &self,
        user_id: Uuid,
        mode: UploadMode,
        parent_id: Option<Uuid>,
        node_id: Option<Uuid>,
        local_path: &Path,
        file_size: u64,
        file_mtime_ns: i64,
    ) -> Result<Option<UploadRecord>, StateError> {
        let path = local_path
            .to_str()
            .ok_or(StateError::PathEncoding)?
            .to_owned();
        let store = self.clone();
        tokio::task::spawn_blocking(move || {
            let connection = store.lock()?;
            let row = connection
                .query_row(
                    r#"
                    SELECT session_id, user_id, local_path, mode, parent_id, node_id,
                           content_id, version_id, chunk_count, declared_size,
                           file_size, file_mtime_ns, status
                    FROM pending_uploads
                    WHERE user_id = ?1
                      AND mode = ?2
                      AND parent_id IS ?3
                      AND (?4 IS NULL OR node_id = ?4)
                      AND local_path = ?5
                      AND file_size = ?6
                      AND file_mtime_ns = ?7
                    ORDER BY updated_at DESC
                    LIMIT 1
                    "#,
                    params![
                        user_id.to_string(),
                        mode.as_str(),
                        parent_id.map(|id| id.to_string()),
                        node_id.map(|id| id.to_string()),
                        path,
                        i64::try_from(file_size).unwrap_or(i64::MAX),
                        file_mtime_ns,
                    ],
                    row_to_record,
                )
                .optional()?;
            match row {
                Some(row) => Ok(Some(row?)),
                None => Ok(None),
            }
        })
        .await
        .map_err(|_| StateError::LockPoisoned)?
    }

    /// 记录一个已确认块（幂等）。
    pub(crate) async fn record_confirmed(
        &self,
        session_id: Uuid,
        chunk: ConfirmedChunk,
    ) -> Result<(), StateError> {
        let store = self.clone();
        tokio::task::spawn_blocking(move || {
            let connection = store.lock()?;
            connection.execute(
                r#"
                INSERT INTO confirmed_chunks (session_id, idx, declared_size, sha256, confirmed_at)
                VALUES (?1, ?2, ?3, ?4, ?5)
                ON CONFLICT (session_id, idx) DO UPDATE SET
                    declared_size = excluded.declared_size,
                    sha256 = excluded.sha256,
                    confirmed_at = excluded.confirmed_at
                "#,
                params![
                    session_id.to_string(),
                    i64::from(chunk.idx),
                    chunk.declared_size,
                    chunk.sha256,
                    unix_seconds().to_string(),
                ],
            )?;
            Ok::<(), StateError>(())
        })
        .await
        .map_err(|_| StateError::LockPoisoned)?
    }

    /// 用服务端权威列表覆盖本地已确认块（服务端是唯一事实来源）。
    pub(crate) async fn replace_confirmed(
        &self,
        session_id: Uuid,
        chunks: Vec<ConfirmedChunk>,
    ) -> Result<(), StateError> {
        let store = self.clone();
        tokio::task::spawn_blocking(move || {
            let mut connection = store.lock()?;
            let tx = connection.transaction()?;
            tx.execute(
                "DELETE FROM confirmed_chunks WHERE session_id = ?1",
                params![session_id.to_string()],
            )?;
            {
                let mut statement = tx.prepare(
                    r#"
                    INSERT INTO confirmed_chunks (session_id, idx, declared_size, sha256, confirmed_at)
                    VALUES (?1, ?2, ?3, ?4, ?5)
                    "#,
                )?;
                for chunk in &chunks {
                    statement.execute(params![
                        session_id.to_string(),
                        i64::from(chunk.idx),
                        chunk.declared_size,
                        chunk.sha256,
                        unix_seconds().to_string(),
                    ])?;
                }
            }
            tx.commit()?;
            Ok::<(), StateError>(())
        })
        .await
        .map_err(|_| StateError::LockPoisoned)?
    }

    /// 读取本地已确认块（仅供测试；续传以服务端为准）。
    #[cfg(test)]
    pub(crate) async fn confirmed_chunks(
        &self,
        session_id: Uuid,
    ) -> Result<Vec<ConfirmedChunk>, StateError> {
        let store = self.clone();
        tokio::task::spawn_blocking(move || {
            let connection = store.lock()?;
            let mut statement = connection.prepare(
                "SELECT idx, declared_size, sha256 FROM confirmed_chunks WHERE session_id = ?1 ORDER BY idx",
            )?;
            let rows = statement.query_map(params![session_id.to_string()], |row| {
                Ok(ConfirmedChunk {
                    idx: row.get::<_, i64>(0)?.try_into().unwrap_or(u32::MAX),
                    declared_size: row.get(1)?,
                    sha256: row.get(2)?,
                })
            })?;
            let mut chunks = Vec::new();
            for row in rows {
                chunks.push(row?);
            }
            Ok::<_, StateError>(chunks)
        })
        .await
        .map_err(|_| StateError::LockPoisoned)?
    }

    /// 在阻塞线程上执行一个数据库任务；所有备份元数据 API 共用。
    async fn run_db<T, F>(&self, job: F) -> Result<T, StateError>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T, StateError> + Send + 'static,
    {
        let connection = self.connection.clone();
        tokio::task::spawn_blocking(move || {
            let mut guard = connection.lock().map_err(|_| StateError::LockPoisoned)?;
            job(&mut guard)
        })
        .await
        .map_err(|_| StateError::LockPoisoned)?
    }

    /// 新建备份文件夹配置。
    pub(crate) async fn insert_backup_folder(
        &self,
        record: BackupFolderRecord,
    ) -> Result<(), StateError> {
        self.run_db(move |connection| {
            connection.execute(
                r#"
                INSERT INTO backup_folders (
                    id, user_id, local_path, local_name, cloud_target_id,
                    cloud_folder_id, cloud_path, exclude_globs, exclude_subfolders,
                    enabled, last_backup_at, created_at, updated_at
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
                "#,
                params![
                    record.id.to_string(),
                    record.user_id.to_string(),
                    record.local_path.to_str().ok_or(StateError::PathEncoding)?,
                    record.local_name,
                    record.cloud_target_id.map(|id| id.to_string()),
                    record.cloud_folder_id.map(|id| id.to_string()),
                    record.cloud_path,
                    json_text(&record.exclude_globs)?,
                    json_text(&record.exclude_subfolders)?,
                    i64::from(record.enabled),
                    record.last_backup_at,
                    record.created_at,
                    record.updated_at,
                ],
            )?;
            Ok(())
        })
        .await
    }

    /// 列出某用户的全部备份文件夹（新到旧）。
    pub(crate) async fn list_backup_folders(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<BackupFolderRecord>, StateError> {
        self.run_db(move |connection| {
            let mut statement = connection.prepare(
                r#"
                SELECT id, user_id, local_path, local_name, cloud_target_id,
                       cloud_folder_id, cloud_path, exclude_globs, exclude_subfolders,
                       enabled, last_backup_at, created_at, updated_at
                FROM backup_folders
                WHERE user_id = ?1
                ORDER BY created_at DESC
                "#,
            )?;
            let rows = statement.query_map(params![user_id.to_string()], row_to_backup_folder)?;
            let mut folders = Vec::new();
            for row in rows {
                folders.push(row??);
            }
            Ok(folders)
        })
        .await
    }

    /// 读取单个备份文件夹；不存在或不属于该用户时返回 `None`。
    pub(crate) async fn get_backup_folder(
        &self,
        user_id: Uuid,
        folder_id: Uuid,
    ) -> Result<Option<BackupFolderRecord>, StateError> {
        self.run_db(move |connection| {
            let row = connection
                .query_row(
                    r#"
                    SELECT id, user_id, local_path, local_name, cloud_target_id,
                           cloud_folder_id, cloud_path, exclude_globs, exclude_subfolders,
                           enabled, last_backup_at, created_at, updated_at
                    FROM backup_folders
                    WHERE user_id = ?1 AND id = ?2
                    "#,
                    params![user_id.to_string(), folder_id.to_string()],
                    row_to_backup_folder,
                )
                .optional()?;
            match row {
                Some(row) => Ok(Some(row?)),
                None => Ok(None),
            }
        })
        .await
    }

    /// 覆盖备份文件夹的可编辑字段（不改 id/created_at）。
    pub(crate) async fn replace_backup_folder(
        &self,
        record: BackupFolderRecord,
    ) -> Result<(), StateError> {
        self.run_db(move |connection| {
            connection.execute(
                r#"
                UPDATE backup_folders SET
                    local_path = ?3,
                    local_name = ?4,
                    cloud_target_id = ?5,
                    cloud_folder_id = ?6,
                    cloud_path = ?7,
                    exclude_globs = ?8,
                    exclude_subfolders = ?9,
                    enabled = ?10,
                    last_backup_at = ?11,
                    updated_at = ?12
                WHERE user_id = ?1 AND id = ?2
                "#,
                params![
                    record.user_id.to_string(),
                    record.id.to_string(),
                    record.local_path.to_str().ok_or(StateError::PathEncoding)?,
                    record.local_name,
                    record.cloud_target_id.map(|id| id.to_string()),
                    record.cloud_folder_id.map(|id| id.to_string()),
                    record.cloud_path,
                    json_text(&record.exclude_globs)?,
                    json_text(&record.exclude_subfolders)?,
                    i64::from(record.enabled),
                    record.last_backup_at,
                    record.updated_at,
                ],
            )?;
            Ok(())
        })
        .await
    }

    /// 只更新已解析的云端目标与最近备份时间（每次扫描后的轻量写回）。
    pub(crate) async fn update_backup_folder_cloud(
        &self,
        user_id: Uuid,
        folder_id: Uuid,
        cloud_folder_id: Option<Uuid>,
        cloud_path: Option<String>,
        last_backup_at: Option<i64>,
        updated_at: i64,
    ) -> Result<(), StateError> {
        self.run_db(move |connection| {
            connection.execute(
                r#"
                UPDATE backup_folders SET
                    cloud_folder_id = ?3,
                    cloud_path = COALESCE(?4, cloud_path),
                    last_backup_at = COALESCE(?5, last_backup_at),
                    updated_at = ?6
                WHERE user_id = ?1 AND id = ?2
                "#,
                params![
                    user_id.to_string(),
                    folder_id.to_string(),
                    cloud_folder_id.map(|id| id.to_string()),
                    cloud_path,
                    last_backup_at,
                    updated_at,
                ],
            )?;
            Ok(())
        })
        .await
    }

    /// 只更新备份文件夹的启用/暂停状态。
    pub(crate) async fn set_backup_folder_enabled(
        &self,
        user_id: Uuid,
        folder_id: Uuid,
        enabled: bool,
        updated_at: i64,
    ) -> Result<(), StateError> {
        self.run_db(move |connection| {
            connection.execute(
                r#"
                UPDATE backup_folders
                SET enabled = ?3, updated_at = ?4
                WHERE user_id = ?1 AND id = ?2
                "#,
                params![
                    user_id.to_string(),
                    folder_id.to_string(),
                    i64::from(enabled),
                    updated_at,
                ],
            )?;
            Ok(())
        })
        .await
    }

    /// 删除备份文件夹配置；索引与活动通过外键级联删除。云端内容不受影响。
    pub(crate) async fn delete_backup_folder(
        &self,
        user_id: Uuid,
        folder_id: Uuid,
    ) -> Result<(), StateError> {
        self.run_db(move |connection| {
            connection.execute(
                "DELETE FROM backup_folders WHERE user_id = ?1 AND id = ?2",
                params![user_id.to_string(), folder_id.to_string()],
            )?;
            Ok(())
        })
        .await
    }

    /// 读取某备份文件夹的完整本地索引（用于一次扫描内的快速判定）。
    pub(crate) async fn list_backup_index(
        &self,
        user_id: Uuid,
        folder_id: Uuid,
    ) -> Result<Vec<BackupIndexRecord>, StateError> {
        self.run_db(move |connection| {
            let mut statement = connection.prepare(
                r#"
                SELECT relative_path, size, mtime_ns, cloud_node_id, content_sha256, updated_at
                FROM backup_index
                WHERE user_id = ?1 AND folder_id = ?2
                "#,
            )?;
            let rows = statement.query_map(
                params![user_id.to_string(), folder_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, i64>(5)?,
                    ))
                },
            )?;
            let mut entries = Vec::new();
            for row in rows {
                let (relative_path, size, mtime_ns, cloud_node_id, content_sha256, updated_at) =
                    row?;
                let size = u64::try_from(size)
                    .map_err(|_| StateError::Corrupt("备份索引中的文件大小无效"))?;
                let cloud_node_id = Uuid::parse_str(&cloud_node_id)
                    .map_err(|_| StateError::Corrupt("备份索引中的节点 ID 无效"))?;
                entries.push(BackupIndexRecord {
                    user_id,
                    folder_id,
                    relative_path,
                    size,
                    mtime_ns,
                    cloud_node_id,
                    content_sha256,
                    updated_at,
                });
            }
            Ok(entries)
        })
        .await
    }

    /// 插入或更新一条本地索引。
    pub(crate) async fn upsert_backup_index(
        &self,
        record: BackupIndexRecord,
    ) -> Result<(), StateError> {
        self.run_db(move |connection| {
            connection.execute(
                r#"
                INSERT INTO backup_index (
                    user_id, folder_id, relative_path, size, mtime_ns,
                    cloud_node_id, content_sha256, updated_at
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                ON CONFLICT (folder_id, relative_path) DO UPDATE SET
                    user_id = excluded.user_id,
                    size = excluded.size,
                    mtime_ns = excluded.mtime_ns,
                    cloud_node_id = excluded.cloud_node_id,
                    content_sha256 = excluded.content_sha256,
                    updated_at = excluded.updated_at
                "#,
                params![
                    record.user_id.to_string(),
                    record.folder_id.to_string(),
                    record.relative_path,
                    i64::try_from(record.size).unwrap_or(i64::MAX),
                    record.mtime_ns,
                    record.cloud_node_id.to_string(),
                    record.content_sha256,
                    record.updated_at,
                ],
            )?;
            Ok(())
        })
        .await
    }

    /// 删除一条本地索引（本地文件已删除时调用；不影响云端）。
    pub(crate) async fn delete_backup_index(
        &self,
        user_id: Uuid,
        folder_id: Uuid,
        relative_path: String,
    ) -> Result<(), StateError> {
        self.run_db(move |connection| {
            connection.execute(
                "DELETE FROM backup_index WHERE user_id = ?1 AND folder_id = ?2 AND relative_path = ?3",
                params![
                    user_id.to_string(),
                    folder_id.to_string(),
                    relative_path
                ],
            )?;
            Ok(())
        })
        .await
    }

    /// 清空一个备份文件夹的本地索引（本地路径被更换时使用；云端不删除）。
    pub(crate) async fn clear_backup_index(
        &self,
        user_id: Uuid,
        folder_id: Uuid,
    ) -> Result<(), StateError> {
        self.run_db(move |connection| {
            connection.execute(
                "DELETE FROM backup_index WHERE user_id = ?1 AND folder_id = ?2",
                params![user_id.to_string(), folder_id.to_string()],
            )?;
            Ok(())
        })
        .await
    }

    /// 写入一条最近活动，并只保留该用户最近 200 条。
    pub(crate) async fn insert_backup_activity(
        &self,
        record: BackupActivityRecord,
    ) -> Result<i64, StateError> {
        self.run_db(move |connection| {
            let tx = connection.transaction()?;
            tx.execute(
                r#"
                INSERT INTO backup_activity (
                    user_id, folder_id, relative_path, kind, occurred_at, message
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                "#,
                params![
                    record.user_id.to_string(),
                    record.folder_id.to_string(),
                    record.relative_path,
                    record.kind,
                    record.occurred_at,
                    record.message,
                ],
            )?;
            let id = tx.last_insert_rowid();
            tx.execute(
                r#"
                DELETE FROM backup_activity
                WHERE user_id = ?1 AND id NOT IN (
                    SELECT id FROM backup_activity
                    WHERE user_id = ?1
                    ORDER BY id DESC
                    LIMIT 200
                )
                "#,
                params![record.user_id.to_string()],
            )?;
            tx.commit()?;
            Ok(id)
        })
        .await
    }

    /// 最近活动（新到旧）。
    pub(crate) async fn list_backup_activity(
        &self,
        user_id: Uuid,
        limit: u32,
    ) -> Result<Vec<BackupActivityRecord>, StateError> {
        let limit = i64::from(limit.clamp(1, 200));
        self.run_db(move |connection| {
            let mut statement = connection.prepare(
                r#"
                SELECT id, folder_id, relative_path, kind, occurred_at, message
                FROM backup_activity
                WHERE user_id = ?1
                ORDER BY id DESC
                LIMIT ?2
                "#,
            )?;
            let rows = statement.query_map(params![user_id.to_string(), limit], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, Option<String>>(5)?,
                ))
            })?;
            let mut activities = Vec::new();
            for row in rows {
                let (id, folder_id, relative_path, kind, occurred_at, message) = row?;
                let folder_id = Uuid::parse_str(&folder_id)
                    .map_err(|_| StateError::Corrupt("备份活动中的文件夹 ID 无效"))?;
                activities.push(BackupActivityRecord {
                    id,
                    user_id,
                    folder_id,
                    relative_path,
                    kind,
                    occurred_at,
                    message,
                });
            }
            Ok(activities)
        })
        .await
    }

    /// 读取一个备份设置；不存在时返回 `None`。
    pub(crate) async fn backup_setting(
        &self,
        user_id: Uuid,
        key: String,
    ) -> Result<Option<String>, StateError> {
        self.run_db(move |connection| {
            let value = connection
                .query_row(
                    "SELECT value FROM backup_settings WHERE user_id = ?1 AND key = ?2",
                    params![user_id.to_string(), key],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            Ok(value)
        })
        .await
    }

    /// 写入一个备份设置（幂等）。
    pub(crate) async fn set_backup_setting(
        &self,
        user_id: Uuid,
        key: String,
        value: String,
    ) -> Result<(), StateError> {
        self.run_db(move |connection| {
            connection.execute(
                r#"
                INSERT INTO backup_settings (user_id, key, value, updated_at)
                VALUES (?1, ?2, ?3, ?4)
                ON CONFLICT (user_id, key) DO UPDATE SET
                    value = excluded.value,
                    updated_at = excluded.updated_at
                "#,
                params![user_id.to_string(), key, value, unix_seconds()],
            )?;
            Ok(())
        })
        .await
    }

    /// 读取微信账号的本地备注名；不存在时返回 `None`。
    pub(crate) async fn wechat_account_note(
        &self,
        data_root: String,
        account_name: String,
    ) -> Result<Option<String>, StateError> {
        self.run_db(move |connection| {
            let note = connection
                .query_row(
                    "SELECT note FROM wechat_accounts WHERE data_root = ?1 AND account_name = ?2",
                    params![data_root, account_name],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            Ok(note)
        })
        .await
    }

    /// 设置微信账号的本地备注名（覆盖旧值）。
    pub(crate) async fn set_wechat_account_note(
        &self,
        data_root: String,
        account_name: String,
        note: String,
    ) -> Result<(), StateError> {
        self.run_db(move |connection| {
            connection.execute(
                r#"
                INSERT INTO wechat_accounts (data_root, account_name, note, updated_at)
                VALUES (?1, ?2, ?3, ?4)
                ON CONFLICT (data_root, account_name) DO UPDATE SET
                    note = excluded.note,
                    updated_at = excluded.updated_at
                "#,
                params![data_root, account_name, note, unix_seconds()],
            )?;
            Ok(())
        })
        .await
    }

    /// 清除微信账号的本地备注名；不存在时也成功。
    pub(crate) async fn clear_wechat_account_note(
        &self,
        data_root: String,
        account_name: String,
    ) -> Result<(), StateError> {
        self.run_db(move |connection| {
            connection.execute(
                "DELETE FROM wechat_accounts WHERE data_root = ?1 AND account_name = ?2",
                params![data_root, account_name],
            )?;
            Ok(())
        })
        .await
    }
}

fn row_to_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<Result<UploadRecord, StateError>> {
    let parse_id = |value: String| {
        Uuid::parse_str(&value).map_err(|_| StateError::Corrupt("状态库中的 UUID 无效"))
    };
    let session_id = match parse_id(row.get::<_, String>(0)?) {
        Ok(value) => value,
        Err(error) => return Ok(Err(error)),
    };
    let user_id = match parse_id(row.get::<_, String>(1)?) {
        Ok(value) => value,
        Err(error) => return Ok(Err(error)),
    };
    let local_path = row.get::<_, String>(2)?;
    let mode = match UploadMode::parse(&row.get::<_, String>(3)?) {
        Some(value) => value,
        None => return Ok(Err(StateError::Corrupt("未知的上传类型"))),
    };
    let parent_id = match row.get::<_, Option<String>>(4)? {
        Some(value) => match parse_id(value) {
            Ok(value) => Some(value),
            Err(error) => return Ok(Err(error)),
        },
        None => None,
    };
    let node_id = match parse_id(row.get::<_, String>(5)?) {
        Ok(value) => value,
        Err(error) => return Ok(Err(error)),
    };
    let content_id = match parse_id(row.get::<_, String>(6)?) {
        Ok(value) => value,
        Err(error) => return Ok(Err(error)),
    };
    let version_id = match parse_id(row.get::<_, String>(7)?) {
        Ok(value) => value,
        Err(error) => return Ok(Err(error)),
    };
    let chunk_count = match u32::try_from(row.get::<_, i64>(8)?) {
        Ok(value) if value > 0 => value,
        _ => return Ok(Err(StateError::Corrupt("上传块数无效"))),
    };
    let declared_size = match u64::try_from(row.get::<_, i64>(9)?) {
        Ok(value) => value,
        Err(_) => return Ok(Err(StateError::Corrupt("声明大小无效"))),
    };
    let file_size = match u64::try_from(row.get::<_, i64>(10)?) {
        Ok(value) => value,
        Err(_) => return Ok(Err(StateError::Corrupt("文件大小无效"))),
    };
    let file_mtime_ns = row.get::<_, i64>(11)?;
    let status = match PendingUploadStatus::parse(&row.get::<_, String>(12)?) {
        Some(value) => value,
        None => return Ok(Err(StateError::Corrupt("未知的上传状态"))),
    };

    Ok(Ok(UploadRecord {
        session_id,
        user_id,
        local_path: PathBuf::from(local_path),
        mode,
        parent_id,
        node_id,
        content_id,
        version_id,
        chunk_count,
        declared_size,
        file_size,
        file_mtime_ns,
        status,
    }))
}

fn unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| i64::try_from(duration.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or_default()
}

fn json_text(values: &[String]) -> Result<String, StateError> {
    serde_json::to_string(values).map_err(|_| StateError::Corrupt("排除规则不是有效 JSON"))
}

fn parse_json_list(value: &str) -> Result<Vec<String>, StateError> {
    serde_json::from_str(value).map_err(|_| StateError::Corrupt("排除规则不是有效 JSON"))
}

fn parse_optional_uuid(value: Option<String>) -> Result<Option<Uuid>, StateError> {
    match value {
        Some(value) => Uuid::parse_str(&value)
            .map(Some)
            .map_err(|_| StateError::Corrupt("状态库中的 UUID 无效")),
        None => Ok(None),
    }
}

fn row_to_backup_folder(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<Result<BackupFolderRecord, StateError>> {
    let id = match Uuid::parse_str(&row.get::<_, String>(0)?) {
        Ok(id) => id,
        Err(_) => return Ok(Err(StateError::Corrupt("状态库中的 UUID 无效"))),
    };
    let user_id = match Uuid::parse_str(&row.get::<_, String>(1)?) {
        Ok(id) => id,
        Err(_) => return Ok(Err(StateError::Corrupt("状态库中的 UUID 无效"))),
    };
    let local_path = PathBuf::from(row.get::<_, String>(2)?);
    let local_name = row.get::<_, String>(3)?;
    let cloud_target_id = match parse_optional_uuid(row.get(4)?) {
        Ok(value) => value,
        Err(error) => return Ok(Err(error)),
    };
    let cloud_folder_id = match parse_optional_uuid(row.get(5)?) {
        Ok(value) => value,
        Err(error) => return Ok(Err(error)),
    };
    let cloud_path = row.get::<_, Option<String>>(6)?;
    let exclude_globs = match parse_json_list(&row.get::<_, String>(7)?) {
        Ok(value) => value,
        Err(error) => return Ok(Err(error)),
    };
    let exclude_subfolders = match parse_json_list(&row.get::<_, String>(8)?) {
        Ok(value) => value,
        Err(error) => return Ok(Err(error)),
    };
    let enabled = row.get::<_, i64>(9)? != 0;
    let last_backup_at = row.get::<_, Option<i64>>(10)?;
    let created_at = row.get::<_, i64>(11)?;
    let updated_at = row.get::<_, i64>(12)?;

    Ok(Ok(BackupFolderRecord {
        id,
        user_id,
        local_path,
        local_name,
        cloud_target_id,
        cloud_folder_id,
        cloud_path,
        exclude_globs,
        exclude_subfolders,
        enabled,
        last_backup_at,
        created_at,
        updated_at,
    }))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;

    fn sample_record() -> UploadRecord {
        UploadRecord {
            session_id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            local_path: PathBuf::from("C:/Users/test/Documents/report.pdf"),
            mode: UploadMode::NewFile,
            parent_id: Some(Uuid::new_v4()),
            node_id: Uuid::new_v4(),
            content_id: Uuid::new_v4(),
            version_id: Uuid::new_v4(),
            chunk_count: 3,
            declared_size: 12,
            file_size: 10,
            file_mtime_ns: 1_700_000_000_000_000_000,
            status: PendingUploadStatus::Uploading,
        }
    }

    #[tokio::test]
    async fn migrations_create_only_metadata_tables() {
        let store = StateStore::open_in_memory().unwrap();
        assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
        let connection = store.lock().unwrap();
        let sql: String = connection
            .query_row(
                "SELECT group_concat(sql, ' ') FROM sqlite_master WHERE type = 'table'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(sql.contains("pending_uploads"));
        assert!(sql.contains("confirmed_chunks"));
        assert!(sql.contains("backup_folders"));
        assert!(sql.contains("backup_index"));
        assert!(sql.contains("backup_activity"));
        assert!(sql.contains("backup_settings"));
        assert!(sql.contains("wechat_accounts"));
        // 只存本机元数据：表结构中不得出现密钥/令牌/密文字段。
        for forbidden in ["wrapped", "token", "ciphertext", "plaintext", "secret"] {
            assert!(
                !sql.to_lowercase().contains(forbidden),
                "状态库中不应出现字段 {forbidden}"
            );
        }
    }

    #[tokio::test]
    async fn backup_activity_keeps_only_latest_two_hundred() {
        let store = StateStore::open_in_memory().unwrap();
        let user_id = Uuid::new_v4();
        let folder_id = Uuid::new_v4();
        // 直接插入文件夹行以避开外键；活动只按 user_id 裁剪。
        {
            let connection = store.lock().unwrap();
            connection
                .execute(
                    r#"
                    INSERT INTO backup_folders (
                        id, user_id, local_path, local_name, exclude_globs,
                        exclude_subfolders, enabled, created_at, updated_at
                    )
                    VALUES (?1, ?2, 'C:/backup', 'backup', '[]', '[]', 1, 0, 0)
                    "#,
                    params![folder_id.to_string(), user_id.to_string()],
                )
                .unwrap();
        }
        for index in 0..205_i64 {
            store
                .insert_backup_activity(BackupActivityRecord {
                    id: 0,
                    user_id,
                    folder_id,
                    relative_path: format!("file-{index}.txt"),
                    kind: "added".to_owned(),
                    occurred_at: index,
                    message: None,
                })
                .await
                .unwrap();
        }
        let activities = store.list_backup_activity(user_id, 500).await.unwrap();
        assert_eq!(activities.len(), 200);
        assert_eq!(activities[0].relative_path, "file-204.txt");
        assert_eq!(activities[199].relative_path, "file-5.txt");
    }

    #[tokio::test]
    async fn upload_roundtrip_and_exact_match() {
        let store = StateStore::open_in_memory().unwrap();
        let record = sample_record();
        store.upsert_upload(record.clone()).await.unwrap();

        let pending = store.list_uploads(record.user_id).await.unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].session_id, record.session_id);
        assert_eq!(pending[0].mode, UploadMode::NewFile);

        let found = store
            .find_upload(
                record.user_id,
                record.mode,
                record.parent_id,
                None,
                &record.local_path,
                record.file_size,
                record.file_mtime_ns,
            )
            .await
            .unwrap()
            .expect("应匹配同一文件指纹");
        assert_eq!(found.session_id, record.session_id);

        let different = store
            .find_upload(
                record.user_id,
                record.mode,
                record.parent_id,
                None,
                &record.local_path,
                record.file_size,
                record.file_mtime_ns + 1,
            )
            .await
            .unwrap();
        assert!(different.is_none(), "修改时间不同不得续传");
    }

    #[tokio::test]
    async fn confirmed_chunks_are_replaced_by_server_truth() {
        let store = StateStore::open_in_memory().unwrap();
        let record = sample_record();
        store.upsert_upload(record.clone()).await.unwrap();
        store
            .record_confirmed(
                record.session_id,
                ConfirmedChunk {
                    idx: 0,
                    declared_size: 41,
                    sha256: "a".repeat(64),
                },
            )
            .await
            .unwrap();
        assert_eq!(
            store
                .confirmed_chunks(record.session_id)
                .await
                .unwrap()
                .len(),
            1
        );

        store
            .replace_confirmed(
                record.session_id,
                vec![ConfirmedChunk {
                    idx: 1,
                    declared_size: 42,
                    sha256: "b".repeat(64),
                }],
            )
            .await
            .unwrap();
        let chunks = store.confirmed_chunks(record.session_id).await.unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].idx, 1);
    }

    #[tokio::test]
    async fn deleting_upload_cascades_confirmed_chunks() {
        let store = StateStore::open_in_memory().unwrap();
        let record = sample_record();
        store.upsert_upload(record.clone()).await.unwrap();
        store
            .record_confirmed(
                record.session_id,
                ConfirmedChunk {
                    idx: 0,
                    declared_size: 41,
                    sha256: "c".repeat(64),
                },
            )
            .await
            .unwrap();
        store.delete_upload(record.session_id).await.unwrap();
        assert!(
            store
                .confirmed_chunks(record.session_id)
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn two_gigabyte_resume_bookkeeping_does_not_allocate_the_file() {
        // 2 GiB = 512 个 4 MiB 块；用注入的元数据模拟，不生成 2 GB 文件。
        let store = StateStore::open_in_memory().unwrap();
        let mut record = sample_record();
        record.chunk_count = 512;
        record.file_size = 2 * 1024 * 1024 * 1024;
        record.declared_size = record.file_size + 512 * 41;
        store.upsert_upload(record.clone()).await.unwrap();
        let confirmed: Vec<ConfirmedChunk> = (0..300)
            .map(|idx| ConfirmedChunk {
                idx,
                declared_size: 4 * 1024 * 1024 + 41,
                sha256: "d".repeat(64),
            })
            .collect();
        store
            .replace_confirmed(record.session_id, confirmed)
            .await
            .unwrap();
        let pending = store.list_uploads(record.user_id).await.unwrap();
        assert_eq!(pending[0].chunk_count, 512);
        assert_eq!(
            store
                .confirmed_chunks(record.session_id)
                .await
                .unwrap()
                .len(),
            300
        );
    }

    #[tokio::test]
    async fn wechat_note_roundtrip_and_old_database_migrates() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(DATABASE_FILE_NAME);
        let data_root = "D:/WeChatData/xwechat_files".to_owned();
        let account = "wxid_test0001".to_owned();

        {
            let store = StateStore::open_path(&path).unwrap();
            assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
            assert!(
                store
                    .wechat_account_note(data_root.clone(), account.clone())
                    .await
                    .unwrap()
                    .is_none()
            );
            store
                .set_wechat_account_note(data_root.clone(), account.clone(), "主账号".to_owned())
                .await
                .unwrap();
            assert_eq!(
                store
                    .wechat_account_note(data_root.clone(), account.clone())
                    .await
                    .unwrap(),
                Some("主账号".to_owned())
            );
            store
                .clear_wechat_account_note(data_root.clone(), account.clone())
                .await
                .unwrap();
            assert!(
                store
                    .wechat_account_note(data_root.clone(), account.clone())
                    .await
                    .unwrap()
                    .is_none()
            );
        }

        // 模拟升级前的 v2 旧库：删除新表并把 user_version 拨回 2。
        {
            let connection = Connection::open(&path).unwrap();
            connection
                .execute_batch("DROP TABLE wechat_accounts; PRAGMA user_version = 2;")
                .unwrap();
        }

        // 旧库重新打开时执行 v3 迁移，表恢复且仍可正常读写。
        let store = StateStore::open_path(&path).unwrap();
        assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
        assert!(
            store
                .wechat_account_note(data_root.clone(), account.clone())
                .await
                .unwrap()
                .is_none()
        );
        store
            .set_wechat_account_note(data_root.clone(), account.clone(), "迁移后".to_owned())
            .await
            .unwrap();
        assert_eq!(
            store.wechat_account_note(data_root, account).await.unwrap(),
            Some("迁移后".to_owned())
        );
    }

    #[tokio::test]
    async fn corrupt_database_can_be_recreated() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(DATABASE_FILE_NAME);
        std::fs::write(&path, b"not a sqlite database").unwrap();
        assert!(StateStore::open_path(&path).is_err());
        let store = StateStore::recreate(dir.path()).unwrap();
        assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
    }
}
