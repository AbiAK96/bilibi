//! Validated SMS delivery through Twilio or a development-only simulator.
//! No Bilimbi module mirrors this file.
mod page;
mod simulator;
mod twilio;

pub use simulator::{Simulator, SimulatorEntry};

use bilibi_module_registry::{Contribution, Module, Provenance, Service};
use bilibi_settings::{Definition, Editor, ScopeKind, Setting, ValueType, DEFINITIONS};
use serde_json::json;
use std::{future::Future, pin::Pin, sync::Arc};

pub const SERVICE: &str = "base/sms.sms";
const PAGES: &str = "base/sms.pages";
pub const ACCOUNT_SID: &str = "sms.twilio.account_sid";
pub const AUTH_TOKEN: &str = "sms.twilio.auth_token";
pub const FROM_NUMBER: &str = "sms.twilio.from_number";
const OWNER: &str = "base/sms";

pub type GatewayFuture<'a> = Pin<Box<dyn Future<Output = Result<DeliveryOutcome>> + Send + 'a>>;
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Invalid SMS message")]
    Invalid,
    #[error("SMS delivery is unavailable")]
    Unavailable,
    #[error("SMS delivery failed")]
    Delivery,
    #[error(transparent)]
    Storage(#[from] bilibi_database::Error),
}

#[derive(Clone)]
pub struct Message {
    pub to: String,
    pub body: String,
}
impl Message {
    pub fn new(to: impl Into<String>, body: impl Into<String>) -> Result<Self> {
        let message = Self {
            to: to.into(),
            body: body.into(),
        };
        if !valid_e164(&message.to)
            || message.body.trim().is_empty()
            || message.body.chars().count() > 480
            || message.body.chars().any(|character| character == '\0')
        {
            return Err(Error::Invalid);
        }
        Ok(message)
    }
}

pub fn valid_e164(value: &str) -> bool {
    let Some(digits) = value.strip_prefix('+') else {
        return false;
    };
    (8..=15).contains(&digits.len())
        && digits
            .as_bytes()
            .first()
            .is_some_and(|digit| *digit != b'0')
        && digits.bytes().all(|digit| digit.is_ascii_digit())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeliveryOutcome {
    pub provider: &'static str,
    pub reference: Option<String>,
}

pub trait SmsGateway: Send + Sync {
    fn deliver<'a>(&'a self, message: &'a Message) -> GatewayFuture<'a>;
    fn describe(&self) -> &'static str;
}

pub struct Sms {
    gateway: Arc<dyn SmsGateway>,
    simulator: Option<Arc<Simulator>>,
}
impl Sms {
    pub fn new(gateway: Arc<dyn SmsGateway>) -> Self {
        Self {
            gateway,
            simulator: None,
        }
    }
    fn development(simulator: Arc<Simulator>) -> Self {
        Self {
            gateway: simulator.clone(),
            simulator: Some(simulator),
        }
    }
    pub async fn deliver(&self, message: &Message) -> Result<DeliveryOutcome> {
        self.gateway.deliver(message).await
    }
    pub fn describe(&self) -> &'static str {
        self.gateway.describe()
    }
    pub fn simulator(&self) -> Option<&Simulator> {
        self.simulator.as_deref()
    }
}

macro_rules! setting {
    ($key:expr, $label:literal, $help:literal, $encrypted:expr) => {
        inventory::submit! { Contribution::new(DEFINITIONS, $key, Provenance { module: OWNER, source: file!(), line: line!() }, &Setting::Definition(Definition {
            value_type: ValueType::String,
            scopes: &[ScopeKind::Global],
            default: || json!(""),
            nullable: false,
            encrypted: $encrypted,
            minimum: None,
            maximum: None,
            validator: None,
            editor: Some(Editor {
                group: "SMS",
                label: $label,
                help: $help,
                capability: None,
                reveal_duration_ms: 5_000,
            }),
        })) }
    };
}
setting!(
    ACCOUNT_SID,
    "Twilio account SID",
    "Account used to send SMS messages.",
    true
);
setting!(
    AUTH_TOKEN,
    "Twilio auth token",
    "Secret used to authenticate SMS delivery.",
    true
);
setting!(
    FROM_NUMBER,
    "Twilio sender",
    "E.164 phone number messages are sent from.",
    true
);

inventory::submit! {
    Module {
        id: OWNER,
        layer: bilibi_module_registry::Layer::Base,
        dependencies: &["base/module_registry", "base/database", "base/settings", "base/integration", "base/ui", "base/tenancy"],
        migrations: &[],
        services: &[Service { id: SERVICE, factory: |services| {
            let environment = std::env::var("BILIBI_ENV").unwrap_or_else(|_| "production".into());
            let sms = if environment == "development" {
                let directory = std::env::var("BILIBI_SMS_DIR").unwrap_or_else(|_| ".local/sms-inbox".into());
                Sms::development(Arc::new(Simulator::new(directory.into(), 100)))
            } else {
                Sms::new(Arc::new(twilio::Twilio::new(
                    services.get("base/database.pool")?,
                    services.get("base/settings.store")?,
                    services.get("base/integration.exchanges")?,
                )))
            };
            eprintln!("sms: {}", sms.describe());
            Ok(Arc::new(sms))
        }}, Service { id: PAGES, factory: |services| {
            Ok(Arc::new(bilibi_ui::page::PageService(Arc::new(page::Pages {
                sms: services.get(SERVICE)?,
            }))))
        }}],
    }
}

inventory::submit! {
    Contribution::new(
        bilibi_module_registry::contributions::DEVELOPMENT_ROUTES,
        "base/sms.development-inbox",
        Provenance { module: OWNER, source: file!(), line: line!() },
        &bilibi_module_registry::contributions::Route {
            method: bilibi_module_registry::contributions::Method::Get,
            path: "/dev/sms-inbox",
            service: PAGES,
            operation: "inbox",
            access: bilibi_module_registry::contributions::Access::Public,
            framed: false,
        },
    )
}
