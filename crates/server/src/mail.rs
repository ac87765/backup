//! 邮件发送（design D13）。
//!
//! - `file` 模式：把邮件写入本地目录，供开发/测试检查；不把邮件内容写日志。
//! - `smtp` 模式：lettre + rustls（ring 后端）发送。
//!
//! 邮件正文可能包含一次性令牌，因此 `RenderedMail` 不实现 `Debug`/`Display`；
//! 日志只允许记录 outbox id 与 purpose。

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use lettre::message::{Mailbox, header::ContentType};
use lettre::transport::smtp::AsyncSmtpTransport;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncTransport, Message, Tokio1Executor};

use crate::config::{MailConfig, SmtpConfig, SmtpTls};

/// 邮件发送错误。错误信息不包含收件人、正文或令牌。
#[derive(Debug, thiserror::Error)]
pub enum MailError {
    #[error("邮件配置无效")]
    Config(&'static str),
    #[error("邮件内容构建失败")]
    Build(#[source] lettre::error::Error),
    #[error("邮件地址无效")]
    Address(#[source] lettre::address::AddressError),
    #[error("SMTP 发送失败")]
    Smtp(#[source] lettre::transport::smtp::Error),
    #[error("写入本地邮件目录失败")]
    Io(#[source] std::io::Error),
}

impl MailError {
    /// 写入 outbox.last_error 的稳定分类；绝不写入原始错误文本。
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Config(_) => "config",
            Self::Build(_) | Self::Address(_) => "message",
            Self::Smtp(_) => "smtp",
            Self::Io(_) => "io",
        }
    }
}

/// 已渲染、准备发送的邮件。
pub struct RenderedMail {
    /// outbox 行 ID，只用于 file 模式文件名与日志。
    pub outbox_id: i64,
    pub to: String,
    pub subject: String,
    pub body: String,
}

/// 邮件发送器抽象（测试可注入失败实现）。
#[async_trait]
pub trait Mailer: Send + Sync {
    async fn send(&self, mail: &RenderedMail) -> Result<(), MailError>;
}

/// 根据配置构造发送器，并在配置错误时返回可安全记录的分类。
pub fn build_mailer(config: &MailConfig) -> Result<std::sync::Arc<dyn Mailer>, MailError> {
    match config {
        MailConfig::File { dir } => Ok(std::sync::Arc::new(FileMailer { dir: dir.clone() })),
        MailConfig::Smtp(smtp) => Ok(std::sync::Arc::new(SmtpMailer::new(smtp)?)),
    }
}

/// 本地目录邮件发送器。
struct FileMailer {
    dir: PathBuf,
}

#[async_trait]
impl Mailer for FileMailer {
    async fn send(&self, mail: &RenderedMail) -> Result<(), MailError> {
        let message = build_message("backup@localhost", mail)?;
        std::fs::create_dir_all(&self.dir).map_err(MailError::Io)?;
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or(0);
        let path = self
            .dir
            .join(format!("outbox-{}-{timestamp}.eml", mail.outbox_id));
        std::fs::write(path, message.formatted()).map_err(MailError::Io)
    }
}

struct SmtpMailer {
    transport: AsyncSmtpTransport<Tokio1Executor>,
    from: Mailbox,
}

impl SmtpMailer {
    fn new(config: &SmtpConfig) -> Result<Self, MailError> {
        let from = config.from.parse::<Mailbox>().map_err(MailError::Address)?;
        let builder = match config.tls {
            SmtpTls::Starttls => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&config.host)
                .map_err(MailError::Smtp)?
                .port(config.port),
            SmtpTls::None => AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&config.host)
                .port(config.port),
        };
        let builder = match (&config.username, &config.password) {
            (Some(username), Some(password)) => builder.credentials(Credentials::new(
                username.expose().to_owned(),
                password.expose().to_owned(),
            )),
            _ => builder,
        };
        Ok(Self {
            transport: builder.build(),
            from,
        })
    }
}

#[async_trait]
impl Mailer for SmtpMailer {
    async fn send(&self, mail: &RenderedMail) -> Result<(), MailError> {
        let message = build_message_box(&self.from, mail)?;
        self.transport
            .send(message)
            .await
            .map_err(MailError::Smtp)?;
        Ok(())
    }
}

fn build_message(from: &str, mail: &RenderedMail) -> Result<Message, MailError> {
    let from = from.parse::<Mailbox>().map_err(MailError::Address)?;
    build_message_box(&from, mail)
}

fn build_message_box(from: &Mailbox, mail: &RenderedMail) -> Result<Message, MailError> {
    let to = mail.to.parse::<Mailbox>().map_err(MailError::Address)?;
    Message::builder()
        .from(from.clone())
        .to(to)
        .subject(mail.subject.clone())
        .header(ContentType::TEXT_PLAIN)
        .body(mail.body.clone())
        .map_err(MailError::Build)
}

/// 测试模板：本组没有真实业务邮件。令牌放在 URL 片段里（不会进入请求路径/日志）。
pub fn render_test_mail(to: &str, token: &str) -> (String, String) {
    (
        "备份服务测试邮件".to_owned(),
        format!(
            "这是一封测试邮件，收件人：{to}\n\n\
             #token={token}\n"
        ),
    )
}
