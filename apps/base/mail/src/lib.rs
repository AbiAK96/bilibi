//! Outgoing mail: one validated message type and two transports. SMTP is used
//! when `MAIL_HOST` is configured; otherwise the dev mailbox keeps every message
//! as an `.eml` file so a reset link can be read without a mail server.
mod mailbox;
mod message;
mod smtp;

pub use mailbox::{Mailbox, MailboxEntry};
pub use message::{Address, Message};
pub use smtp::{Smtp, SmtpConfig, Tls};

use bilibi_module_registry::{Module, Service};
use std::{future::Future, pin::Pin, sync::Arc};

pub type Result<T> = std::result::Result<T, Error>;
pub type SendFuture<'a> = Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Invalid email message")]
    Invalid,
    #[error("Mail is not configured: {0}")]
    Config(String),
    /// Transport detail for operators' logs; callers show a generic failure.
    #[error("Mail delivery failed: {0}")]
    Delivery(String),
}

/// A transport receives a message already validated and rendered to RFC 5322.
pub trait Transport: Send + Sync {
    fn send<'a>(&'a self, message: &'a lettre::Message) -> SendFuture<'a>;
    /// Secret-free description for the boot log.
    fn describe(&self) -> String;
}

pub struct Mailer {
    sender: Address,
    transport: Arc<dyn Transport>,
}
impl Mailer {
    pub fn new(sender: Address, transport: Arc<dyn Transport>) -> Self {
        Self { sender, transport }
    }
    pub fn describe(&self) -> String {
        format!("{} from {}", self.transport.describe(), self.sender.email)
    }
    pub async fn deliver(&self, message: &Message) -> Result<()> {
        let rendered = message.render(&self.sender)?;
        self.transport.send(&rendered).await
    }
    /// SMTP when `MAIL_HOST` is set, with every other `MAIL_*` variable then
    /// required; the dev mailbox otherwise. A half-configured relay fails boot
    /// rather than silently falling back to files.
    pub fn from_env() -> Result<Self> {
        Self::from_lookup(|name| std::env::var(name).ok())
    }
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Self> {
        let value = |name: &str| get(name).filter(|value| !value.trim().is_empty());
        let required =
            |name: &str| value(name).ok_or_else(|| Error::Config(format!("{name} must be set")));
        if value("MAIL_HOST").is_none() {
            let sender = Address::new(
                Some(&value("MAIL_FROM_NAME").unwrap_or_else(|| "Bilibi".into())),
                &value("MAIL_FROM_ADDRESS").unwrap_or_else(|| "noreply@bilibi.localhost".into()),
            )?;
            let dir = value("BILIBI_MAIL_DIR").unwrap_or_else(|| ".local/mailbox".into());
            return Ok(Self::new(sender, Arc::new(Mailbox::new(dir.into(), 100))));
        }
        let sender = Address::new(
            Some(&required("MAIL_FROM_NAME")?),
            &required("MAIL_FROM_ADDRESS")?,
        )?;
        let port = required("MAIL_PORT")?
            .parse::<u16>()
            .ok()
            .filter(|port| *port > 0)
            .ok_or_else(|| {
                Error::Config("MAIL_PORT must be an integer between 1 and 65535".into())
            })?;
        let tls = match required("MAIL_TLS_MODE")?.as_str() {
            "starttls" => Tls::StartTls,
            "implicit_tls" => Tls::Implicit,
            _ => {
                return Err(Error::Config(
                    "MAIL_TLS_MODE must be either starttls or implicit_tls".into(),
                ))
            }
        };
        let smtp = Smtp::new(SmtpConfig {
            host: required("MAIL_HOST")?,
            port,
            username: required("MAIL_USERNAME")?,
            password: required("MAIL_PASSWORD")?,
            tls,
        })?;
        Ok(Self::new(sender, Arc::new(smtp)))
    }
}

inventory::submit! {
    Module {
        id: "base/mail", layer: bilibi_module_registry::Layer::Base,
        dependencies: &["base/module_registry"], migrations: &[],
        services: &[Service { id: "base/mail.mailer", factory: |_| {
            let mailer = Mailer::from_env().map_err(|error| bilibi_module_registry::RegistryError::Service(error.to_string()))?;
            eprintln!("mail: {}", mailer.describe());
            Ok(Arc::new(mailer))
        }}],
    }
}
