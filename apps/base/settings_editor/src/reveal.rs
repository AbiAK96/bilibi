//! Reauthentication and audit edge for showing one stored encrypted value
//! (Bilimbi `BilimbiWeb.SecretReveal`). The capability must be granted by name;
//! the actor re-enters their own password, failures are throttled, and every
//! attempt is recorded before plaintext can leave. No audit row, no value.
use crate::{form::Plan, SECRET_VIEW};
use bilibi_audit::{record_action, Attribution};
use bilibi_authz::{decide, explicitly_granted};
use bilibi_database::{Error, Result};
use bilibi_settings::{Settings, Target};
use bilibi_tenancy::{Authentication, Scope};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};
use tokio_postgres::GenericClient;

/// Belimbing's login throttle: five failed attempts per account in a minute.
const LIMIT: usize = 5;
const WINDOW: Duration = Duration::from_secs(60);

/// Process-local sliding window per tenant and account, like Bilimbi's
/// per-node ETS throttle. A success clears the account's failures.
#[derive(Default)]
pub struct Throttle(Mutex<HashMap<(i64, i64), Vec<Instant>>>);
impl Throttle {
    fn allowed(&self, key: (i64, i64)) -> bool {
        let mut failures = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        let now = Instant::now();
        failures.retain(|_, times| {
            times.retain(|at| now.duration_since(*at) < WINDOW);
            !times.is_empty()
        });
        failures.get(&key).map_or(0, Vec::len) < LIMIT
    }
    fn fail(&self, key: (i64, i64)) {
        let mut failures = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        failures.entry(key).or_default().push(Instant::now());
    }
    fn reset(&self, key: (i64, i64)) {
        let mut failures = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        failures.remove(&key);
    }
}

pub enum Refusal {
    Throttled,
    InvalidPassword,
    AuditUnavailable,
    /// Not granted, not a visible stored secret, or the key is unavailable.
    Refused,
}

/// Presentation only: whether to draw "Show stored value". Reads without a
/// decision-log row; `reveal` decides again, logged, before acting. An
/// impersonated session never sees it, as in Bilimbi: the password it would
/// re-prove is the borrowed account's.
pub async fn available(
    client: &impl GenericClient,
    plan: &Plan,
    settings: &Settings,
    scope: &Scope,
) -> Result<bool> {
    Ok(settings.encryption_available()
        && scope.impersonator().is_none()
        && plan.capabilities.contains(SECRET_VIEW)
        && explicitly_granted(client, scope, SECRET_VIEW).await?)
}

/// Records a refused attempt that never reached a password check, such as a
/// forged request for a field the page did not offer.
pub async fn refuse(client: &(impl GenericClient + Sync), scope: &Scope, key: &str) -> Result<()> {
    audit(client, scope, key, false).await
}

pub struct Reveal<'a> {
    pub settings: &'a Settings,
    pub authentication: &'a Authentication,
    pub throttle: &'a Throttle,
}
impl Reveal<'_> {
    /// The plaintext of `key`'s stored global value, as text for the input.
    pub async fn reveal(
        &self,
        client: &(impl GenericClient + Sync),
        plan: &Plan,
        scope: &Scope,
        key: &str,
        password: &str,
    ) -> std::result::Result<String, Refusal> {
        let result = self.attempt(client, plan, scope, key, password).await;
        match audit(client, scope, key, result.is_ok()).await {
            Ok(()) => result,
            Err(_) => Err(Refusal::AuditUnavailable),
        }
    }
    async fn attempt(
        &self,
        client: &(impl GenericClient + Sync),
        plan: &Plan,
        scope: &Scope,
        key: &str,
        password: &str,
    ) -> std::result::Result<String, Refusal> {
        let throttle = (scope.tenant(), scope.user());
        let granted = scope.impersonator().is_none()
            && decide(client, scope, SECRET_VIEW)
                .await
                .is_ok_and(|decision| decision.allowed)
            && explicitly_granted(client, scope, SECRET_VIEW)
                .await
                .unwrap_or(false);
        let stored = plan
            .field(key)
            .is_some_and(|field| field.encrypted() && field.stored_secret);
        if !granted || !stored || !self.settings.encryption_available() {
            return Err(Refusal::Refused);
        }
        if !self.throttle.allowed(throttle) {
            return Err(Refusal::Throttled);
        }
        match self
            .authentication
            .confirm_password(client, scope, password)
            .await
        {
            Ok(true) => self.throttle.reset(throttle),
            Ok(false) => {
                self.throttle.fail(throttle);
                return Err(Refusal::InvalidPassword);
            }
            Err(_) => return Err(Refusal::Refused),
        }
        match self.settings.get(client, scope, Target::Global, key).await {
            Ok(resolved) => Ok(match resolved.value {
                Value::String(text) => text,
                Value::Null => return Err(Refusal::Refused),
                other => other.to_string(),
            }),
            Err(_) => Err(Refusal::Refused),
        }
    }
}

/// One retained action per attempt, never with the value.
async fn audit(
    client: &(impl GenericClient + Sync),
    scope: &Scope,
    key: &str,
    succeeded: bool,
) -> Result<()> {
    let payload = json!({
        "setting_key": key.chars().take(255).collect::<String>(),
        "scope_type": "global",
        "scope_id": Value::Null,
        "result": if succeeded { "succeeded" } else { "refused" },
    });
    record_action(
        client,
        &Attribution::from_scope(scope),
        None,
        "settings.secret.reveal",
        payload,
        true,
    )
    .await
    .map(|_| ())
    .map_err(|_| Error::Internal)
}
