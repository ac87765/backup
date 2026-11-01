//! 本地诊断日志：写到应用数据目录下 `logs/`，按大小滚动，只留几个小文件。
//!
//! 只放行本项目自身的日志目标：reqwest / hyper 等依赖在调试级别会输出完整 URL，
//! 而预签名 URL 的签名不得落盘（AGENTS.md）。调用方同样不得记录令牌、密钥、
//! 加密元数据、请求体、本地路径、微信账号目录名或文件名；错误只记稳定错误码。

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tracing::Subscriber;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::filter::Targets;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::SubscriberExt;

/// 应用数据目录下的日志子目录（Dart 侧“打开日志文件夹”使用同一名称）。
pub const LOG_DIR_NAME: &str = "logs";
/// 当前日志文件名；滚动后依次为 `client.log.1`、`client.log.2`……
pub const LOG_FILE_NAME: &str = "client.log";
/// 单个日志文件的大小上限。
pub const MAX_LOG_FILE_BYTES: u64 = 1024 * 1024;
/// 保留的已滚动文件数（加上当前文件，总占用不超过 4 MiB）。
pub const KEPT_ROTATED_FILES: u32 = 3;

/// 按大小滚动的日志文件写入器。
#[derive(Clone)]
pub struct RotatingLogWriter {
    inner: Arc<Mutex<RotatingLogFile>>,
}

impl RotatingLogWriter {
    /// 在 `directory` 下打开（必要时创建）日志文件。
    pub fn open(directory: &Path, max_bytes: u64, kept_rotated: u32) -> io::Result<Self> {
        fs::create_dir_all(directory)?;
        let path = directory.join(LOG_FILE_NAME);
        let file = open_append(&path)?;
        let written = file.metadata()?.len();
        Ok(Self {
            inner: Arc::new(Mutex::new(RotatingLogFile {
                path,
                max_bytes,
                kept_rotated,
                file: Some(file),
                written,
            })),
        })
    }
}

impl Write for RotatingLogWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        // 日志永远不能影响备份：锁损坏时丢弃这一行。
        match self.inner.lock() {
            Ok(mut file) => file.write_line(buffer),
            Err(_) => Ok(buffer.len()),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for RotatingLogWriter {
    type Writer = Self;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

struct RotatingLogFile {
    path: PathBuf,
    max_bytes: u64,
    kept_rotated: u32,
    file: Option<File>,
    written: u64,
}

impl RotatingLogFile {
    /// fmt 层把一条事件格式化后一次写入，因此滚动不会把一行拆到两个文件。
    fn write_line(&mut self, line: &[u8]) -> io::Result<usize> {
        if self.written > 0 && self.written.saturating_add(line.len() as u64) > self.max_bytes {
            self.rotate();
        }
        if self.file.is_none() {
            self.file = Some(open_append(&self.path)?);
        }
        if let Some(file) = self.file.as_mut() {
            file.write_all(line)?;
            self.written = self.written.saturating_add(line.len() as u64);
        }
        Ok(line.len())
    }

    fn rotate(&mut self) {
        // Windows 不能重命名仍打开的文件，先关闭当前句柄。
        self.file = None;
        if self.shift_files().is_err() {
            // 滚动失败（例如安全软件占用）时清空当前文件，保证占用仍有上限。
            self.file = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&self.path)
                .ok();
        }
        self.written = 0;
    }

    fn shift_files(&self) -> io::Result<()> {
        if self.kept_rotated == 0 {
            return fs::remove_file(&self.path);
        }
        for index in (1..self.kept_rotated).rev() {
            match fs::rename(self.rotated(index), self.rotated(index + 1)) {
                Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
                _ => {}
            }
        }
        fs::rename(&self.path, self.rotated(1))
    }

    fn rotated(&self, index: u32) -> PathBuf {
        let mut name = self.path.clone().into_os_string();
        name.push(format!(".{index}"));
        PathBuf::from(name)
    }
}

fn open_append(path: &Path) -> io::Result<File> {
    OpenOptions::new().create(true).append(true).open(path)
}

/// 构造写入 `writer` 的订阅者：只放行本项目的目标，INFO 及以上。
pub fn file_subscriber(writer: RotatingLogWriter) -> impl Subscriber + Send + Sync {
    let targets = Targets::new()
        .with_target("client_core", LevelFilter::INFO)
        .with_target("rust_lib_client", LevelFilter::INFO);
    let layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .log_internal_errors(false)
        .with_writer(writer);
    tracing_subscriber::registry().with(layer).with(targets)
}

/// 安装进程级日志订阅者。重复调用时保留第一次安装的订阅者（返回 `Ok`）。
pub fn install_file_logging(app_data_dir: &Path) -> io::Result<()> {
    let writer = RotatingLogWriter::open(
        &app_data_dir.join(LOG_DIR_NAME),
        MAX_LOG_FILE_BYTES,
        KEPT_ROTATED_FILES,
    )?;
    let _ = tracing::subscriber::set_global_default(file_subscriber(writer));
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;

    fn log_files(directory: &Path) -> Vec<(String, u64)> {
        let mut files: Vec<(String, u64)> = fs::read_dir(directory)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (
                    entry.file_name().to_string_lossy().into_owned(),
                    // Windows 上目录项的大小在文件关闭前不更新，这里要查文件本身。
                    fs::metadata(entry.path()).unwrap().len(),
                )
            })
            .collect();
        files.sort();
        files
    }

    #[test]
    fn rotation_keeps_a_bounded_number_of_small_files() {
        let directory = tempfile::tempdir().unwrap();
        let mut writer = RotatingLogWriter::open(directory.path(), 100, 2).unwrap();
        for index in 0..100 {
            writer
                .write_all(format!("line {index:04} ..\n").as_bytes())
                .unwrap();
        }
        let files = log_files(directory.path());
        let names: Vec<&str> = files.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, ["client.log", "client.log.1", "client.log.2"]);
        assert!(files.iter().all(|(_, size)| *size <= 100));

        // 最新一行在当前文件，最旧的内容已被丢弃。
        let current = fs::read_to_string(directory.path().join("client.log")).unwrap();
        assert!(current.ends_with("line 0099 ..\n"));
        let oldest = fs::read_to_string(directory.path().join("client.log.2")).unwrap();
        assert!(!oldest.contains("line 0000"));
    }

    #[test]
    fn reopening_continues_counting_existing_size() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join(LOG_FILE_NAME), vec![b'x'; 95]).unwrap();
        let mut writer = RotatingLogWriter::open(directory.path(), 100, 1).unwrap();
        writer.write_all(b"0123456789\n").unwrap();
        let files = log_files(directory.path());
        assert_eq!(
            files,
            [
                ("client.log".to_owned(), 11),
                ("client.log.1".to_owned(), 95)
            ]
        );
    }

    #[test]
    fn subscriber_writes_project_events_and_drops_dependency_targets() {
        let directory = tempfile::tempdir().unwrap();
        let writer = RotatingLogWriter::open(directory.path(), 4096, 1).unwrap();
        tracing::subscriber::with_default(file_subscriber(writer), || {
            tracing::warn!(target: "client_core::transfer", status = 403, "project event");
            tracing::info!(target: "rust_lib_client::api", "bridge event");
            tracing::debug!(target: "client_core::transfer", "too verbose");
            tracing::warn!(target: "reqwest::connect", "https://oss.example/o?Signature=abc");
            tracing::warn!(target: "hyper_util::client", "dependency event");
        });
        let content = fs::read_to_string(directory.path().join(LOG_FILE_NAME)).unwrap();
        assert!(content.contains("project event"));
        assert!(content.contains("status=403"));
        assert!(content.contains("bridge event"));
        assert!(!content.contains("too verbose"));
        assert!(!content.contains("Signature"));
        assert!(!content.contains("dependency event"));
        assert!(!content.contains('\u{1b}'), "日志文件不含 ANSI 转义");
    }
}
