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
pub const SCHEMA_VERSION: i64 = 1;
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
        // 只存本机元数据：表结构中不得出现密钥/令牌/密文字段。
        for forbidden in ["wrapped", "token", "ciphertext", "plaintext", "secret"] {
            assert!(
                !sql.to_lowercase().contains(forbidden),
                "状态库中不应出现字段 {forbidden}"
            );
        }
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
    async fn corrupt_database_can_be_recreated() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(DATABASE_FILE_NAME);
        std::fs::write(&path, b"not a sqlite database").unwrap();
        assert!(StateStore::open_path(&path).is_err());
        let store = StateStore::recreate(dir.path()).unwrap();
        assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
    }
}
