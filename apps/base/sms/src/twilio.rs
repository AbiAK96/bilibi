//! Twilio adapter over Base Integration. No Bilimbi module mirrors this file.
use crate::{
    DeliveryOutcome, Error, GatewayFuture, Message, SmsGateway, ACCOUNT_SID, AUTH_TOKEN,
    FROM_NUMBER, OWNER,
};
use base64::Engine as _;
use bilibi_database::Db;
use bilibi_integration::{Call, Integration, Origin, Outgoing};
use bilibi_settings::Settings;
use std::sync::Arc;

pub struct Twilio {
    db: Db,
    settings: Arc<Settings>,
    integration: Arc<Integration>,
}
impl Twilio {
    pub fn new(db: Arc<Db>, settings: Arc<Settings>, integration: Arc<Integration>) -> Self {
        Self {
            db: db.as_ref().clone(),
            settings,
            integration,
        }
    }
    async fn value(&self, key: &str) -> crate::Result<String> {
        let client = self.db.get().await.map_err(bilibi_database::Error::from)?;
        let value = self
            .settings
            .get_own_global_secret(&**client, OWNER, key)
            .await?;
        value
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .ok_or(Error::Unavailable)
    }
}
impl SmsGateway for Twilio {
    fn deliver<'a>(&'a self, message: &'a Message) -> GatewayFuture<'a> {
        Box::pin(async move {
            let sid = self.value(ACCOUNT_SID).await?;
            let token = self.value(AUTH_TOKEN).await?;
            let from = self.value(FROM_NUMBER).await?;
            if !crate::valid_e164(&from) {
                return Err(Error::Unavailable);
            }
            let credentials =
                base64::engine::general_purpose::STANDARD.encode(format!("{sid}:{token}"));
            let body = serde_urlencoded::to_string([
                ("To", message.to.as_str()),
                ("From", from.as_str()),
                ("Body", message.body.as_str()),
            ])
            .map_err(|_| Error::Invalid)?
            .into_bytes();
            let reply = self
                .integration
                .send(
                    Origin::System,
                    &Call {
                        system: "twilio".into(),
                        provider: Some("twilio".into()),
                        operation: "sms.send".into(),
                        owner_type: Some(OWNER.into()),
                        retain_payload: false,
                        ..Call::default()
                    },
                    Outgoing::post(format!(
                        "https://api.twilio.com/2010-04-01/Accounts/{sid}/Messages.json"
                    ))
                    .header("Authorization", format!("Basic {credentials}"))
                    .header("Content-Type", "application/x-www-form-urlencoded")
                    .bytes(body)
                    .no_redirects()
                    .max_body_bytes(64 * 1024),
                )
                .await
                .map_err(|_| Error::Delivery)?;
            if !(200..300).contains(&reply.status) {
                return Err(Error::Delivery);
            }
            let reference = reply.json().and_then(|body| {
                body.get("sid")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            });
            Ok(DeliveryOutcome {
                provider: "twilio",
                reference,
            })
        })
    }
    fn describe(&self) -> &'static str {
        "Twilio"
    }
}
