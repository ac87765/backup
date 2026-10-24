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

/// 单次 SMTP 会话的硬超时。邮件队列会重试，因此宁可失败也不能永久挂住 worker。
pub const SMTP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

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
            SmtpTls::Implicit => AsyncSmtpTransport::<Tokio1Executor>::relay(&config.host)
                .map_err(MailError::Smtp)?
                .port(config.port),
            SmtpTls::None => AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&config.host)
                .port(config.port),
        };
        // 显式设置会话超时：默认的 60 秒会让一个卡住的 SMTP 连接长时间占住 worker。
        let builder = builder.timeout(Some(SMTP_TIMEOUT));
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

/// 邮件用途。与 `email_outbox.purpose`、`email_tokens.purpose` 保持一致。
pub mod purpose {
    /// 邮箱验证（需要一次性令牌）。
    pub const VERIFY_EMAIL: &str = "verify_email";
    /// 注册时邮箱已存在（不需要令牌）。
    pub const EXISTING_ACCOUNT: &str = "existing_account";
    /// 密码已修改通知（不需要令牌）。
    pub const PASSWORD_CHANGED: &str = "password_changed";
    /// 恢复密钥已轮换通知（不需要令牌）。
    pub const RECOVERY_KEY_CHANGED: &str = "recovery_key_changed";
    /// 仅测试使用（需要令牌，覆盖 file/SMTP 发送链路）。
    pub const TEST: &str = "test";
}

/// 该用途是否需要一次性令牌。
pub fn purpose_requires_token(purpose: &str) -> bool {
    matches!(purpose, purpose::VERIFY_EMAIL | purpose::TEST)
}

/// 是否为已知模板（未知用途按失败重试，避免静默丢邮件）。
pub fn is_supported_purpose(purpose: &str) -> bool {
    matches!(
        purpose,
        purpose::VERIFY_EMAIL
            | purpose::EXISTING_ACCOUNT
            | purpose::PASSWORD_CHANGED
            | purpose::RECOVERY_KEY_CHANGED
            | purpose::TEST
    )
}

/// 把用例渲染成 (subject, body)。
///
/// 只有验证/测试邮件需要 `token`；其余通知模板忽略该参数。
/// 任何带令牌的链接都必须把令牌放在 URL 片段（`#token=`）中，
/// 片段不会被浏览器发送到服务端，因此不会进入请求日志。
pub fn render_mail(
    purpose: &str,
    to: &str,
    token: Option<&str>,
    app_url: &str,
) -> Option<(String, String)> {
    let base = app_url.trim_end_matches('/');
    match purpose {
        purpose::VERIFY_EMAIL => {
            let token = token?;
            Some((
                "验证你的备份服务邮箱".to_owned(),
                format!(
                    "请打开以下链接验证邮箱：\n\n\
                     {base}/#token={token}\n\n\
                     链接 24 小时内有效，且只能使用一次。\n\
                     如果这不是你本人的操作，请忽略此邮件。\n"
                ),
            ))
        }
        purpose::EXISTING_ACCOUNT => Some((
            "你尝试注册的邮箱已有账号".to_owned(),
            format!(
                "我们收到了使用 {to} 注册备份服务的请求，但该邮箱已经有账号。\n\n\
                 如果这是你本人，请直接登录：{base}/#/login\n\
                 如果忘记密码，可使用恢复密钥重置密码。\n\
                 如果这不是你本人的操作，请忽略此邮件。\n"
            ),
        )),
        purpose::PASSWORD_CHANGED => Some((
            "备份服务密码已修改".to_owned(),
            format!(
                "你的备份服务密码已修改，其他设备上的会话已失效。\n\n\
                 查看活跃会话：{base}/#/sessions\n\
                 如果这不是你本人的操作，请立即使用恢复密钥重置密码。\n"
            ),
        )),
        purpose::RECOVERY_KEY_CHANGED => Some((
            "备份服务恢复密钥已更新".to_owned(),
            format!(
                "你的恢复密钥已更新，旧恢复密钥立即失效。\n\n\
                 请确认已经把新恢复密钥保存在安全的地方：{base}/#/settings\n\
                 如果这不是你本人的操作，请立即修改密码。\n"
            ),
        )),
        purpose::TEST => {
            let token = token?;
            Some((
                "备份服务测试邮件".to_owned(),
                format!("这是一封测试邮件，收件人：{to}\n\n#token={token}\n"),
            ))
        }
        _ => None,
    }
}

/// 测试模板的兼容入口（第 5 组测试使用）。
pub fn render_test_mail(to: &str, token: &str) -> (String, String) {
    render_mail(purpose::TEST, to, Some(token), "http://localhost")
        .unwrap_or_else(|| ("测试邮件".to_owned(), String::new()))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;
    use crate::config::Secret;

    #[tokio::test]
    async fn all_smtp_tls_modes_construct_a_mailer() {
        // 与 storage::ensure_http_stack 一样，先把进程级 rustls provider 固定为 ring。
        let _ = rustls::crypto::ring::default_provider().install_default();
        for tls in [SmtpTls::Starttls, SmtpTls::Implicit, SmtpTls::None] {
            let config = SmtpConfig {
                host: "smtp.example.com".to_owned(),
                port: 2525,
                from: "backup@example.com".to_owned(),
                username: Some(Secret::new("user")),
                password: Some(Secret::new("password")),
                tls,
                allow_insecure: tls == SmtpTls::None,
            };
            SmtpMailer::new(&config)
                .unwrap_or_else(|error| panic!("{tls:?} 模式应能构造发送器: {error}"));
        }
    }

    #[test]
    fn smtp_session_timeout_is_fifteen_seconds() {
        assert_eq!(SMTP_TIMEOUT, std::time::Duration::from_secs(15));
    }

    #[test]
    fn mail_error_kinds_are_stable() {
        assert_eq!(MailError::Config("x").kind(), "config");
        assert_eq!(MailError::Io(std::io::Error::other("x")).kind(), "io");
    }

    #[test]
    fn verification_link_keeps_token_in_fragment_not_path() {
        let token = "a".repeat(64);
        let (_subject, body) = render_mail(
            purpose::VERIFY_EMAIL,
            "user@example.com",
            Some(&token),
            "https://backup.example.com/",
        )
        .expect("验证模板");
        assert!(body.contains(&format!("https://backup.example.com/#token={token}")));
        assert!(!body.contains(&format!("/token/{token}")));
        assert!(!body.contains(&format!("?token={token}")));
    }

    #[test]
    fn notification_templates_do_not_need_a_token() {
        for purpose in [
            purpose::EXISTING_ACCOUNT,
            purpose::PASSWORD_CHANGED,
            purpose::RECOVERY_KEY_CHANGED,
        ] {
            let rendered = render_mail(
                purpose,
                "user@example.com",
                None,
                "https://backup.example.com",
            );
            assert!(rendered.is_some(), "{purpose} 模板必须存在");
            let (subject, body) = rendered.expect("模板");
            assert!(!subject.is_empty());
            assert!(!body.is_empty());
        }
        assert!(render_mail("unknown", "user@example.com", None, "https://x").is_none());
    }

    #[test]
    fn token_is_required_for_verification_template() {
        assert!(
            render_mail(
                purpose::VERIFY_EMAIL,
                "user@example.com",
                None,
                "https://backup.example.com"
            )
            .is_none()
        );
        assert!(purpose_requires_token(purpose::VERIFY_EMAIL));
        assert!(purpose_requires_token(purpose::TEST));
        assert!(!purpose_requires_token(purpose::PASSWORD_CHANGED));
        assert!(is_supported_purpose(purpose::EXISTING_ACCOUNT));
    }
}
