//! The anonymous credential pages: sign-in (the homepage, as in Bilimbi),
//! forgotten password and reset. Bilimbi's LiveViews are plain forms here: a
//! POST re-renders the card with its errors, and a completed step redirects
//! with a one-shot flash cookie that names a message, never carries one. The
//! validation mirrors the source changesets; the credential lookup stays
//! tenant + email until K9 binds the global login, so each form carries the
//! tenant that Bilimbi does not need. Sign-in has no attempt throttle yet.
use crate::{api, document_titled, origin, token, App, HttpError};
use askama::Template;
use axum::{
    extract::{ConnectInfo, Extension, Form, Path, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Redirect, Response},
};
use bilibi_database::Error;
use bilibi_ui::{
    AuthLayout, FlashMap, ForgotForm, ForgotPasswordPage, Html, LoginForm, LoginPage,
    LoginWorkspace, ResetForm, ResetPasswordPage, CREDENTIALS_REJECTED, PASSWORD_RESET,
    RESET_LINK_INVALID, RESET_REQUESTED,
};
use bilibi_user::{password::check_policy, CredentialError};
use serde::Deserialize;
use std::{collections::HashMap, net::SocketAddr};

const TITLE_SUFFIX: &str = " · Business application platform";
const BLANK: &str = "can't be blank";
const INVALID: &str = "is invalid";
const EMAIL_FORMAT: &str = "must be a valid email address";
const CONFIRMATION_MISMATCH: &str = "does not match confirmation";
/// Where a signed-in visitor to a credential page is sent: the dashboard, as
/// Bilimbi's `UserAuth` signs every account in.
const WORKSPACE: &str = "/dashboard";

/// The one-shot flash cookie carries one of these keys; the sign-in page maps
/// the key to its copy and clears the cookie. Free text never travels in it.
const FLASH_COOKIE: &str = "bilibi_flash";
pub const FLASH_RESET: &str = "reset";
pub const FLASH_EXPIRED: &str = "expired";
const FLASH_KEYS: [&str; 2] = [FLASH_RESET, FLASH_EXPIRED];

pub fn set_flash(app: &App, response: &mut Response, key: &'static str) {
    debug_assert!(FLASH_KEYS.contains(&key));
    let cookie = format!(
        "{FLASH_COOKIE}={key}; HttpOnly; SameSite=Lax; Path=/; Max-Age=60{}",
        secure(app)
    );
    if let Ok(value) = HeaderValue::from_str(&cookie) {
        response.headers_mut().append(header::SET_COOKIE, value);
    }
}
fn clear_flash(response: &mut Response) {
    response.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_static("bilibi_flash=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0"),
    );
}
fn flash_key(headers: &HeaderMap) -> Option<&'static str> {
    let value = cookie(headers, FLASH_COOKIE)?;
    FLASH_KEYS.into_iter().find(|key| *key == value)
}
fn cookie<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get(header::COOKIE)
        .and_then(|h| h.to_str().ok())?
        .split(';')
        .find_map(|cookie| cookie.trim().strip_prefix(name)?.strip_prefix('='))
}
fn secure(app: &App) -> &'static str {
    if app.origin.starts_with("https://") {
        "; Secure"
    } else {
        ""
    }
}

/// Bilimbi forwards a signed-in visitor from every anonymous page.
async fn signed_in(app: &App, headers: &HeaderMap) -> bool {
    match token(headers) {
        Ok(token) => app.security.guard(&token).await.is_ok(),
        Err(_) => false,
    }
}

/// Where an unauthenticated page request goes: the sign-in page, which says
/// the session expired only when a session cookie was presented.
pub fn sign_in_redirect(app: &App, had_credential: bool) -> Response {
    let mut response = Redirect::to("/").into_response();
    if had_credential {
        set_flash(app, &mut response, FLASH_EXPIRED);
    }
    response
}

fn card<T: Template>(
    title: &str,
    card: T,
    flash: FlashMap<'_>,
    status: StatusCode,
) -> std::result::Result<Response, HttpError> {
    let body = AuthLayout {
        body: Html::render(&card).map_err(|_| Error::Internal)?,
        flash,
    }
    .render()
    .map_err(|_| Error::Internal)?;
    let mut response = document_titled(&format!("{title}{TITLE_SUFFIX}"), &body)?.into_response();
    *response.status_mut() = status;
    Ok(response)
}

// ---- validation, worded as the source changesets are --------------------

fn validate_email(value: &str) -> Vec<String> {
    if value.is_empty() {
        vec![BLANK.into()]
    } else if value.chars().any(char::is_whitespace)
        || value.len() > 254
        || !value
            .split_once('@')
            .is_some_and(|(local, domain)| !local.is_empty() && !domain.is_empty())
    {
        vec![EMAIL_FORMAT.into()]
    } else {
        Vec::new()
    }
}
fn validate_tenant(value: &str) -> (Option<i64>, Vec<String>) {
    if value.is_empty() {
        return (None, vec![BLANK.into()]);
    }
    match value.parse::<i64>() {
        Ok(tenant) if tenant > 0 => (Some(tenant), Vec::new()),
        _ => (None, vec![INVALID.into()]),
    }
}
fn validate_required(value: &str) -> Vec<String> {
    if value.is_empty() {
        vec![BLANK.into()]
    } else {
        Vec::new()
    }
}
fn validate_new_password(value: &str) -> Vec<String> {
    if value.is_empty() {
        return vec![BLANK.into()];
    }
    match check_policy(value) {
        Ok(()) => Vec::new(),
        Err(violation) => vec![violation.to_string()],
    }
}
fn validate_confirmation(value: &str, password: &str) -> Vec<String> {
    if value.is_empty() {
        vec![BLANK.into()]
    } else if value != password {
        vec![CONFIRMATION_MISMATCH.into()]
    } else {
        Vec::new()
    }
}
fn bounded(value: &str) -> String {
    value.trim().chars().take(254).collect()
}

// ---- sign-in -----------------------------------------------------------------

/// `/` and `/login`: the sign-in card, or the workspace for a signed-in visitor.
pub async fn home(
    State(app): State<App>,
    headers: HeaderMap,
) -> std::result::Result<Response, HttpError> {
    if signed_in(&app, &headers).await {
        return Ok(Redirect::to(WORKSPACE).into_response());
    }
    let key = flash_key(&headers);
    let form = LoginForm {
        session_expired: key == Some(FLASH_EXPIRED),
        ..LoginForm::default()
    };
    let flash = FlashMap {
        success: if key == Some(FLASH_RESET) {
            PASSWORD_RESET
        } else {
            ""
        },
        ..FlashMap::default()
    };
    let mut response = login_card(&app, form, flash, StatusCode::OK).await?;
    if key.is_some() {
        clear_flash(&mut response);
    }
    Ok(response)
}
async fn login_card(
    app: &App,
    mut form: LoginForm,
    flash: FlashMap<'_>,
    status: StatusCode,
) -> std::result::Result<Response, HttpError> {
    form.workspace = login_workspace(app).await;
    card(
        "Sign in",
        LoginPage::new(&form).map_err(|_| Error::Internal)?,
        flash,
        status,
    )
}
/// The sign-in card's workspace strip. A failed lookup degrades the strip and
/// is logged; it never fails the sign-in card.
async fn login_workspace(app: &App) -> LoginWorkspace {
    let lookup = async {
        let client = app.db.get().await.map_err(Error::from)?;
        bilibi_core::company::primary::platform_operator_company(&**client).await
    };
    match lookup.await {
        Ok(Some(company)) => LoginWorkspace::Ready(company.display_name),
        Ok(None) => LoginWorkspace::NotProvisioned,
        Err(error) => {
            eprintln!("warning: platform operator company lookup failed: {error:?}");
            match error {
                Error::Database(_) | Error::Pool(_) => LoginWorkspace::DatabaseUnavailable,
                _ => LoginWorkspace::InvariantViolation,
            }
        }
    }
}
#[derive(Deserialize)]
pub struct LoginSubmission {
    #[serde(rename = "login[tenant]", default)]
    tenant: String,
    #[serde(rename = "login[email]", default)]
    email: String,
    #[serde(rename = "login[password]", default)]
    password: String,
}
pub async fn login(
    State(app): State<App>,
    headers: HeaderMap,
    peer: Option<Extension<ConnectInfo<SocketAddr>>>,
    Form(submission): Form<LoginSubmission>,
) -> std::result::Result<Response, HttpError> {
    origin(&app, &headers)?;
    let mut form = LoginForm {
        tenant: bounded(&submission.tenant),
        email: bounded(&submission.email),
        ..LoginForm::default()
    };
    let (tenant, tenant_errors) = validate_tenant(&form.tenant);
    form.tenant_errors = tenant_errors;
    form.email_errors = validate_email(&form.email);
    form.password_errors = validate_required(&submission.password);
    let Some(tenant) =
        tenant.filter(|_| form.email_errors.is_empty() && form.password_errors.is_empty())
    else {
        return login_card(
            &app,
            form,
            FlashMap::default(),
            StatusCode::UNPROCESSABLE_ENTITY,
        )
        .await;
    };
    match app
        .user
        .login(tenant, &form.email, &submission.password)
        .await
    {
        Ok(session) => {
            let client = app.db.get().await.map_err(Error::from)?;
            app.security
                .sessions
                .describe(
                    &**client,
                    &session.token,
                    api::client_address(peer.map(|Extension(ConnectInfo(peer))| peer), &headers)
                        .map(|ip| ip.to_string())
                        .as_deref(),
                    headers
                        .get(header::USER_AGENT)
                        .and_then(|value| value.to_str().ok()),
                )
                .await?;
            let cookie = format!(
                "bilibi_session={}; HttpOnly; SameSite=Lax; Path=/; Max-Age=86400{}",
                session.token,
                secure(&app)
            );
            let mut response = Redirect::to(WORKSPACE).into_response();
            response.headers_mut().insert(
                header::SET_COOKIE,
                HeaderValue::from_str(&cookie).map_err(|_| Error::Internal)?,
            );
            Ok(response)
        }
        Err(Error::Unauthorized | Error::Invalid) => {
            // Reported twice over, as Bilimbi does: the announced alert for
            // assistive technology and the email field's own error line.
            form.form_error = CREDENTIALS_REJECTED.into();
            form.email_errors = vec![CREDENTIALS_REJECTED.into()];
            login_card(&app, form, FlashMap::default(), StatusCode::UNAUTHORIZED).await
        }
        Err(error) => Err(error.into()),
    }
}

// ---- forgotten password ---------------------------------------------------------

pub async fn forgot_page(
    State(app): State<App>,
    headers: HeaderMap,
) -> std::result::Result<Response, HttpError> {
    if signed_in(&app, &headers).await {
        return Ok(Redirect::to(WORKSPACE).into_response());
    }
    forgot_card(&ForgotForm::default(), StatusCode::OK)
}
fn forgot_card(form: &ForgotForm, status: StatusCode) -> std::result::Result<Response, HttpError> {
    card(
        "Forgot password",
        ForgotPasswordPage::new(form).map_err(|_| Error::Internal)?,
        FlashMap::default(),
        status,
    )
}
#[derive(Deserialize)]
pub struct ForgotSubmission {
    #[serde(rename = "forgot[tenant]", default)]
    tenant: String,
    #[serde(rename = "forgot[email]", default)]
    email: String,
}
pub async fn forgot(
    State(app): State<App>,
    headers: HeaderMap,
    Form(submission): Form<ForgotSubmission>,
) -> std::result::Result<Response, HttpError> {
    origin(&app, &headers)?;
    let mut form = ForgotForm {
        tenant: bounded(&submission.tenant),
        email: bounded(&submission.email),
        ..ForgotForm::default()
    };
    let (tenant, tenant_errors) = validate_tenant(&form.tenant);
    form.tenant_errors = tenant_errors;
    form.email_errors = validate_email(&form.email);
    let Some(tenant) = tenant.filter(|_| form.email_errors.is_empty()) else {
        return forgot_card(&form, StatusCode::UNPROCESSABLE_ENTITY);
    };
    match app.user.request_password_reset(tenant, &form.email).await {
        // The confirmation is the same whether or not the account exists.
        Ok(()) | Err(CredentialError::Other(Error::Invalid)) => {
            form.confirmation = RESET_REQUESTED.into();
            forgot_card(&form, StatusCode::OK)
        }
        Err(CredentialError::Other(error)) => Err(error.into()),
        Err(_) => Err(Error::Internal.into()),
    }
}

// ---- reset -------------------------------------------------------------------------

pub async fn reset_page(
    State(app): State<App>,
    headers: HeaderMap,
    Path(_token): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> std::result::Result<Response, HttpError> {
    if signed_in(&app, &headers).await {
        return Ok(Redirect::to(WORKSPACE).into_response());
    }
    let form = ResetForm {
        tenant: bounded(query.get("tenant").map(String::as_str).unwrap_or("")),
        email: bounded(query.get("email").map(String::as_str).unwrap_or("")),
        ..ResetForm::default()
    };
    reset_card(&form, StatusCode::OK)
}
fn reset_card(form: &ResetForm, status: StatusCode) -> std::result::Result<Response, HttpError> {
    card(
        "Reset password",
        ResetPasswordPage::new(form).map_err(|_| Error::Internal)?,
        FlashMap::default(),
        status,
    )
}
#[derive(Deserialize)]
pub struct ResetSubmission {
    #[serde(rename = "reset[tenant]", default)]
    tenant: String,
    #[serde(rename = "reset[email]", default)]
    email: String,
    #[serde(rename = "reset[password]", default)]
    password: String,
    #[serde(rename = "reset[password_confirmation]", default)]
    password_confirmation: String,
}
pub async fn reset(
    State(app): State<App>,
    headers: HeaderMap,
    Path(token): Path<String>,
    Form(submission): Form<ResetSubmission>,
) -> std::result::Result<Response, HttpError> {
    origin(&app, &headers)?;
    let mut form = ResetForm {
        tenant: bounded(&submission.tenant),
        email: bounded(&submission.email),
        ..ResetForm::default()
    };
    form.email_errors = validate_email(&form.email);
    form.password_errors = validate_new_password(&submission.password);
    form.confirmation_errors =
        validate_confirmation(&submission.password_confirmation, &submission.password);
    if !(form.email_errors.is_empty()
        && form.password_errors.is_empty()
        && form.confirmation_errors.is_empty())
    {
        return reset_card(&form, StatusCode::UNPROCESSABLE_ENTITY);
    }
    // A link without a usable tenant is as invalid as one with a bad token.
    let tenant = validate_tenant(&form.tenant).0.unwrap_or(0);
    let outcome = app
        .user
        .reset_password(tenant, &form.email, &token, &submission.password)
        .await;
    match outcome {
        Ok(()) => {
            let mut response = Redirect::to("/").into_response();
            set_flash(&app, &mut response, FLASH_RESET);
            Ok(response)
        }
        Err(CredentialError::InvalidOrExpiredToken) => {
            form.email_errors = vec![RESET_LINK_INVALID.into()];
            reset_card(&form, StatusCode::UNPROCESSABLE_ENTITY)
        }
        Err(CredentialError::Policy(violation)) => {
            form.password_errors = vec![violation.to_string()];
            reset_card(&form, StatusCode::UNPROCESSABLE_ENTITY)
        }
        Err(CredentialError::Other(error)) => Err(error.into()),
        Err(_) => Err(Error::Internal.into()),
    }
}
