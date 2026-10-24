//! 邮件 outbox 测试（tasks 5.3）。
//!
//! 覆盖 file 模式写文件、SMTP 失败后退避重试成功、重试生成新令牌、
//! outbox/日志中不出现令牌明文。

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod common;

use std::io::{BufRead, BufReader, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use async_trait::async_trait;
use server::config::{MailConfig, SmtpConfig, SmtpTls};
use server::jobs::LeaseConfig;
use server::jobs::email::{
    PURPOSE_TEST, RecipientResolver, claim_batch, enqueue_email, run_batch, run_batch_with_lease,
};
use server::mail::{MailError, Mailer, RenderedMail, build_mailer};
use sqlx::{PgPool, Row};
use tracing_subscriber::fmt::MakeWriter;
use uuid::Uuid;

use common::{environment, migrated_database};

struct Buffer(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Buffer {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("日志缓冲锁").extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Buffer {
    type Writer = Buffer;

    fn make_writer(&'a self) -> Self::Writer {
        Buffer(self.0.clone())
    }
}

static LOGS: OnceLock<Arc<Mutex<Vec<u8>>>> = OnceLock::new();

fn captured_logs() -> Arc<Mutex<Vec<u8>>> {
    LOGS.get_or_init(|| {
        let buffer = Arc::new(Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::fmt()
            .json()
            .with_max_level(tracing::Level::DEBUG)
            .with_writer(Buffer(buffer.clone()))
            .finish();
        let _ = tracing::subscriber::set_global_default(subscriber);
        buffer
    })
    .clone()
}

struct FixedResolver;

#[async_trait]
impl RecipientResolver for FixedResolver {
    async fn resolve(&self, _pool: &PgPool, _user_id: Uuid) -> Result<Option<String>, sqlx::Error> {
        Ok(Some("user@example.com".to_owned()))
    }
}

/// 慢速邮件发送器：模拟一次发送耗时接近租约安全余量的情况。
struct SlowMailer {
    delay: Duration,
}

#[async_trait]
impl Mailer for SlowMailer {
    async fn send(&self, _mail: &RenderedMail) -> Result<(), MailError> {
        tokio::time::sleep(self.delay).await;
        Ok(())
    }
}

async fn enqueue_test_mail(database: &server::Database, user_id: Uuid) {
    let mut tx = database.pool().begin().await.expect("开始事务");
    enqueue_email(
        &mut tx,
        user_id,
        PURPOSE_TEST,
        &serde_json::json!({"language": "zh-CN"}),
    )
    .await
    .expect("登记邮件");
    tx.commit().await.expect("提交事务");
}

fn extract_token(message: &str) -> String {
    // lettre 会对正文做 quoted-printable 编码（`=` → `=3D`），但十六进制令牌本身
    // 保持连续；取第一个至少 64 位的十六进制串。
    message
        .split(|character: char| !character.is_ascii_hexdigit())
        .find(|run| run.len() >= 64)
        .map(|run| run[..64].to_owned())
        .expect("邮件正文中应包含 64 位十六进制令牌")
}

#[tokio::test]
async fn file_mode_writes_mail_without_storing_plaintext_token() {
    let env = environment().await;
    let (_test_db, database) = migrated_database(env).await;
    let logs = captured_logs();
    logs.lock().expect("日志锁").clear();
    let dir = tempfile::tempdir().expect("临时邮件目录");
    let config = MailConfig::File {
        dir: dir.path().to_path_buf(),
    };
    let mailer = build_mailer(&config).expect("构造 file mailer");
    let resolver: Arc<dyn RecipientResolver> = Arc::new(FixedResolver);
    let user_id = Uuid::new_v4();
    enqueue_test_mail(&database, user_id).await;

    let stats = run_batch(&database, &mailer, &resolver, "http://localhost", 10)
        .await
        .expect("处理邮件");
    assert_eq!(stats.sent, 1);

    let files: Vec<_> = std::fs::read_dir(dir.path())
        .expect("读取邮件目录")
        .collect::<Result<Vec<_>, _>>()
        .expect("目录项");
    assert_eq!(files.len(), 1);
    let content = std::fs::read_to_string(files[0].path()).expect("读取邮件文件");
    assert!(content.contains("To: user@example.com"), "{content}");
    let token = extract_token(&content);

    assert_token_not_in_database(&database, user_id, &token).await;
    let logs_text = String::from_utf8(logs.lock().expect("日志锁").clone()).expect("日志是 UTF-8");
    assert!(
        !logs_text.contains(&token),
        "日志中不得出现邮件令牌，实际日志: {logs_text}"
    );
}

#[tokio::test]
async fn smtp_failure_is_retried_with_a_new_token() {
    let env = environment().await;
    let (_test_db, database) = migrated_database(env).await;
    let logs = captured_logs();
    logs.lock().expect("日志锁").clear();
    let smtp = FakeSmtp::start(1);
    let config = MailConfig::Smtp(SmtpConfig {
        host: "127.0.0.1".to_owned(),
        port: smtp.port,
        from: "backup@example.com".to_owned(),
        username: None,
        password: None,
        tls: SmtpTls::None,
        allow_insecure: true,
    });
    let mailer = build_mailer(&config).expect("构造 smtp mailer");
    let resolver: Arc<dyn RecipientResolver> = Arc::new(FixedResolver);
    let user_id = Uuid::new_v4();
    enqueue_test_mail(&database, user_id).await;

    // 第一次：SMTP 连接被立即关闭 → outbox 保留并退避。
    let stats = run_batch(&database, &mailer, &resolver, "http://localhost", 10)
        .await
        .expect("第一次发送");
    assert_eq!(stats.failed, 1);
    let row =
        sqlx::query("SELECT attempts, last_error, not_before > now() AS future FROM email_outbox")
            .fetch_one(database.pool())
            .await
            .expect("读取 outbox");
    let attempts: i32 = row.get("attempts");
    let last_error: Option<String> = row.get("last_error");
    let future: bool = row.get("future");
    assert_eq!(attempts, 1);
    assert_eq!(last_error.as_deref(), Some("smtp"));
    assert!(future);

    // 第二次：到期后重试成功，消息里应有新令牌。
    sqlx::query("UPDATE email_outbox SET not_before = now()")
        .execute(database.pool())
        .await
        .expect("置为到期");
    let stats = run_batch(&database, &mailer, &resolver, "http://localhost", 10)
        .await
        .expect("重试发送");
    assert_eq!(stats.sent, 1, "重试应当成功");
    let outbox_left: i64 = sqlx::query_scalar("SELECT count(*) FROM email_outbox")
        .fetch_one(database.pool())
        .await
        .expect("统计 outbox");
    assert_eq!(outbox_left, 0);

    let messages = smtp.messages.lock().expect("SMTP 消息锁").clone();
    assert_eq!(messages.len(), 1, "只应有一封成功发送的邮件");
    let token = extract_token(&messages[0]);
    assert_token_not_in_database(&database, user_id, &token).await;

    let token_rows: i64 = sqlx::query_scalar("SELECT count(*) FROM email_tokens")
        .fetch_one(database.pool())
        .await
        .expect("统计令牌");
    assert_eq!(token_rows, 1, "重试时应删除旧未使用令牌，只保留新令牌");

    let logs_text = String::from_utf8(logs.lock().expect("日志锁").clone()).expect("日志是 UTF-8");
    assert!(
        !logs_text.contains(&token),
        "日志中不得出现邮件令牌，实际日志: {logs_text}"
    );
}

async fn assert_token_not_in_database(database: &server::Database, user_id: Uuid, token: &str) {
    let outbox_leaks: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM email_outbox WHERE params::text LIKE '%' || $1 || '%'",
    )
    .bind(token)
    .fetch_one(database.pool())
    .await
    .expect("检查 outbox");
    assert_eq!(outbox_leaks, 0, "outbox 中不得出现令牌明文");

    let token_leaks: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM email_tokens WHERE encode(token_hash, 'hex') = $1",
    )
    .bind(token)
    .fetch_one(database.pool())
    .await
    .expect("检查令牌表");
    assert_eq!(token_leaks, 0, "令牌表只能保存哈希");

    let hashed: String =
        sqlx::query_scalar("SELECT encode(token_hash, 'hex') FROM email_tokens WHERE user_id = $1")
            .bind(user_id)
            .fetch_one(database.pool())
            .await
            .expect("读取令牌哈希");
    assert_ne!(hashed, token);
}

#[tokio::test]
async fn lease_aware_email_worker_releases_remaining_mail() {
    let env = environment().await;
    let (_test_db, database) = migrated_database(env).await;
    let resolver: Arc<dyn RecipientResolver> = Arc::new(FixedResolver);
    let user_id = Uuid::new_v4();
    for _ in 0..5 {
        enqueue_test_mail(&database, user_id).await;
    }

    let mailer: Arc<dyn Mailer> = Arc::new(SlowMailer {
        delay: Duration::from_millis(1_800),
    });
    let lease = LeaseConfig::new(Duration::from_secs(3), Duration::from_millis(1_500));
    let stats = run_batch_with_lease(&database, &mailer, &resolver, "http://localhost", 10, lease)
        .await
        .expect("处理邮件批次");
    assert_eq!(stats.claimed, 5);
    assert_eq!(stats.sent, 1, "只应发送第一封邮件");
    assert_eq!(stats.released, 4, "租约接近到期时必须释放剩余邮件");

    let released: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM email_outbox WHERE claimed_until IS NULL AND claim_token IS NULL",
    )
    .fetch_one(database.pool())
    .await
    .expect("统计已释放邮件");
    assert_eq!(released, 4);

    let reclaimed = claim_batch(database.pool(), 10)
        .await
        .expect("另一个 worker 立即领取");
    assert_eq!(reclaimed.len(), 4, "被释放的邮件应可被立即重新领取");
}

struct FakeSmtp {
    port: u16,
    messages: Arc<Mutex<Vec<String>>>,
}

impl FakeSmtp {
    fn start(fail_first: usize) -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("绑定假 SMTP");
        let port = listener.local_addr().expect("假 SMTP 地址").port();
        let messages = Arc::new(Mutex::new(Vec::new()));
        let failures = Arc::new(AtomicUsize::new(fail_first));
        let messages_for_thread = messages.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else {
                    break;
                };
                let should_fail = failures
                    .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                        remaining.checked_sub(1)
                    })
                    .is_ok();
                if should_fail {
                    // 立即关闭连接，模拟 SMTP 不可用。
                    drop(stream);
                    continue;
                }
                let messages = messages_for_thread.clone();
                std::thread::spawn(move || handle_smtp(stream, &messages));
            }
        });
        Self { port, messages }
    }
}

fn handle_smtp(stream: std::net::TcpStream, messages: &Arc<Mutex<Vec<String>>>) {
    let mut writer = stream.try_clone().expect("克隆 SMTP 连接");
    let mut reader = BufReader::new(stream);
    let _ = writer.write_all(b"220 localhost ESMTP\r\n");
    let mut in_data = false;
    let mut data = String::new();
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        if in_data {
            if line.trim_end() == "." {
                messages.lock().expect("SMTP 消息锁").push(data.clone());
                data.clear();
                in_data = false;
                let _ = writer.write_all(b"250 2.0.0 Ok: queued\r\n");
            } else {
                data.push_str(&line);
            }
            continue;
        }
        let command = line
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_ascii_uppercase();
        match command.as_str() {
            "EHLO" | "HELO" => {
                let _ = writer.write_all(b"250-localhost\r\n250 PIPELINING\r\n");
            }
            "DATA" => {
                in_data = true;
                let _ = writer.write_all(b"354 End data with <CR><LF>.<CR><LF>\r\n");
            }
            "QUIT" => {
                let _ = writer.write_all(b"221 Bye\r\n");
                break;
            }
            _ => {
                let _ = writer.write_all(b"250 OK\r\n");
            }
        }
    }
}
