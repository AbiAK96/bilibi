//! Disposable end-to-end walkthrough of SMS as a phone-verification,
//! step-up and pending-login proof. It uses the development SMS simulator and
//! real services; it is an example, not a test or a demo seed.
//!
//! Run against a freshly migrated disposable database with
//! `BILIBI_ENV=development`, `BILIBI_SMS_DIR` set to a disposable inbox,
//! `BILIBI_SETTINGS_KEY` set to a one-off 32-byte key, and
//! `BILIBI_SMS_TWO_FACTOR_DEMO=disposable`. The fixture administrator uses a
//! authenticator proof to authorize company settings; no credential or SMS code is
//! printed. No Bilimbi module mirrors this file.
use bilibi_authz::authorize_key;
use bilibi_database::pool;
use bilibi_party_ids::OrganisationId;
use bilibi_security::Scope;
use bilibi_settings::{Settings, Target};
use bilibi_step_up::POLICY_UPDATE;
use bilibi_user::{AccountField, Login, LoginMethod, LoginOutcome, UserService};
use hmac::{Hmac, KeyInit, Mac};
use serde_json::json;
use sha1::Sha1;
use std::time::{Duration, SystemTime};

bilibi_walkthrough_fixture::walkthrough_vocabulary!();
bilibi_walkthrough_fixture::walkthrough_module_roles!();

type Failure = Box<dyn std::error::Error>;
const COMPANY: i64 = 1;
const MANAGER: i64 = 1;
const PASSWORD: &str = bilibi_walkthrough_fixture::PASSWORD;
const SMS_ENABLED: &str = bilibi_step_up::SMS_ENABLED_KEY;
const POLICY: &str = "step_up.policy";
const MANAGE: &str = "admin.authz.step-up.manage";

fn totp(secret: &str) -> Result<String, Failure> {
    let encoded: String = secret
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect();
    let key = data_encoding::BASE32_NOPAD.decode(encoded.as_bytes())?;
    let step = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)?
        .as_secs()
        / 30;
    let mut mac = Hmac::<Sha1>::new_from_slice(&key)?;
    mac.update(&step.to_be_bytes());
    let digest = mac.finalize().into_bytes();
    let offset = usize::from(digest[digest.len() - 1] & 0x0f);
    let binary = u32::from_be_bytes([
        digest[offset] & 0x7f,
        digest[offset + 1],
        digest[offset + 2],
        digest[offset + 3],
    ]);
    Ok(format!("{:06}", binary % 1_000_000))
}

fn wrong_code(code: &str) -> &'static str {
    if code == "000000" {
        "000001"
    } else {
        "000000"
    }
}

async fn newest_code(sms: &bilibi_sms::Sms) -> Result<String, Failure> {
    let entry = sms
        .simulator()
        .ok_or("the development SMS simulator is not selected")?
        .entries()
        .await?
        .into_iter()
        .next()
        .ok_or("no SMS is present in the disposable inbox")?;
    let prefix = "Your Bilibi verification code is ";
    let rest = entry
        .body
        .strip_prefix(prefix)
        .ok_or("the simulator message has an unexpected shape")?;
    let code = rest
        .get(..6)
        .filter(|candidate| candidate.bytes().all(|byte| byte.is_ascii_digit()))
        .ok_or("the simulator message contains no six-digit code")?;
    Ok(code.to_owned())
}

async fn set_company_setting(
    app: &bilibi_web::App,
    admin: &Scope,
    key: &str,
    value: serde_json::Value,
) -> Result<(), Failure> {
    let mut client = app.db.get().await?;
    let mut tx = app
        .mutations
        .begin(&mut client, admin.mutation_context(POLICY_UPDATE))
        .await?;
    authorize_key(tx.sql(), admin, MANAGE).await?;
    app.step_up
        .require(admin, POLICY_UPDATE, &format!("company:{COMPANY} {key}"))
        .await?;
    let settings = app.services.get::<Settings>("base/settings.store")?;
    settings
        .put(
            &mut tx,
            admin,
            Target::Company(OrganisationId::new(COMPANY)?),
            key,
            value,
        )
        .await?;
    tx.commit().await?;
    Ok(())
}

fn login(outcome: LoginOutcome) -> Result<Login, Failure> {
    match outcome {
        LoginOutcome::Ready(login) => Ok(login),
        LoginOutcome::StepUp(_) => Err("login unexpectedly required step-up".into()),
    }
}

#[tokio::main]
async fn main() -> Result<(), Failure> {
    if std::env::var("BILIBI_SMS_TWO_FACTOR_DEMO").as_deref() != Ok("disposable")
        || std::env::var("BILIBI_ENV").as_deref() != Ok("development")
    {
        return Err("opt in with BILIBI_SMS_TWO_FACTOR_DEMO=disposable and BILIBI_ENV=development on a disposable migrated database".into());
    }
    bilibi_base::link();
    bilibi_core::link();
    let db = pool(&std::env::var("DATABASE_URL")?)?;
    let modules = bilibi_module_registry::discover()?;
    bilibi_database::migrate(&db, &modules).await?;
    bilibi_walkthrough_fixture::install(&db).await?;
    let app = bilibi_web::App::new(db.clone(), "http://localhost:35587")?;
    app.step_up.set_enforced(true);
    let grants = app
        .services
        .get::<bilibi_authz::grants::Grants>("base/authz.grants")?;
    grants.reconcile().await?;
    let users = app.services.get::<UserService>("core/user.accounts")?;
    let sms = app.services.get::<bilibi_sms::Sms>(bilibi_sms::SERVICE)?;
    let admin_login = users.login("admin@example.test", PASSWORD).await?;
    let admin = app.security.guard(&admin_login.token).await?;

    println!("== authenticator proof ==");
    let enrollment = users.begin_two_factor(&admin, PASSWORD).await?;
    users
        .confirm_two_factor(&admin, &totp(&enrollment.key)?)
        .await?;
    println!("authenticator enrolled; no secret or code was printed");

    set_company_setting(&app, &admin, SMS_ENABLED, json!(true)).await?;
    println!("company SMS policy enabled through the authorized, audited setting boundary");

    println!("\n== phone verification and SMS step-up ==");
    let manager_login = users.login("manager@example.test", PASSWORD).await?;
    let manager = app.security.guard(&manager_login.token).await?;
    let sent = users.request_phone_verification(&manager).await?;
    println!("verification sent to {}", sent.masked_phone);
    let code = newest_code(&sms).await?;
    println!(
        "wrong phone code refused: {}",
        users
            .confirm_phone(&manager, wrong_code(&code))
            .await
            .is_err()
    );
    users.confirm_phone(&manager, &code).await?;
    println!(
        "phone verified: {}",
        users.sms_factor_status(&manager).await?.phone_verified
    );
    tokio::time::sleep(Duration::from_secs(1)).await;
    users.request_browser_step_up_sms(&manager).await?;
    let code = newest_code(&sms).await?;
    users.step_up_with_sms(&manager, &code).await?;
    println!("SMS accepted for browser step-up; the phone-verification code was not reused");

    println!("\n== login policy off, then on ==");
    let ordinary = login(users.begin_login("manager@example.test", PASSWORD).await?)?;
    bilibi_user::logout(&app.db, &ordinary.token, &ordinary.csrf, &users.sessions).await?;
    println!("policy off: normal session issued");
    set_company_setting(&app, &admin, POLICY, json!({ "core.user.login": "on" })).await?;
    tokio::time::sleep(Duration::from_secs(1)).await;

    let pending = match users.begin_login("manager@example.test", PASSWORD).await? {
        LoginOutcome::Ready(_) => {
            return Err("login policy on did not create a pending login".into())
        }
        LoginOutcome::StepUp(state) => state,
    };
    println!(
        "policy on: pending handle only; SMS available={}",
        pending.sms_available
    );
    users.send_pending_login_sms(&pending.handle).await?;
    let code = newest_code(&sms).await?;
    println!(
        "wrong pending-login code refused: {}",
        users
            .complete_pending_login(&pending.handle, LoginMethod::Sms, wrong_code(&code))
            .await
            .is_err()
    );
    let promoted = users
        .complete_pending_login(&pending.handle, LoginMethod::Sms, &code)
        .await?;
    let promoted_scope = app.security.guard(&promoted.token).await?;
    println!(
        "correct SMS promoted to a normal session: {}",
        promoted_scope.user() == MANAGER
    );
    println!(
        "reused login proof refused after promotion: {}",
        users
            .complete_pending_login(&pending.handle, LoginMethod::Sms, &code)
            .await
            .is_err()
    );

    println!("\n== authorization, company isolation and phone change ==");
    let client = db.get().await?;
    println!(
        "non-admin policy change refused: {}",
        authorize_key(&**client, &manager, MANAGE).await.is_err()
    );
    let other = app
        .security
        .guard(
            &users
                .login("manager@other.example.test", PASSWORD)
                .await?
                .token,
        )
        .await?;
    println!(
        "SMS remains off for the other company: {}",
        !users.sms_factor_status(&other).await?.company_enabled
    );
    let version: i64 = client
        .query_one("SELECT version FROM users WHERE tenant_id=1 AND id=1", &[])
        .await?
        .get(0);
    drop(client);
    users
        .update_account(
            &admin,
            MANAGER,
            version,
            AccountField::Phone,
            "+15555550199",
        )
        .await?;
    println!(
        "changing the account phone clears verification: {}",
        !users.sms_factor_status(&manager).await?.phone_verified
    );

    set_company_setting(&app, &admin, POLICY, json!({ "core.user.login": "off" })).await?;
    let ordinary = login(users.begin_login("manager@example.test", PASSWORD).await?)?;
    bilibi_user::logout(&app.db, &ordinary.token, &ordinary.csrf, &users.sessions).await?;
    println!("policy off restored: normal session issued");
    Ok(())
}
