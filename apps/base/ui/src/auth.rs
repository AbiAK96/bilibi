//! The credential pages: the centered layout (Bilimbi `Layouts.auth`) and the
//! sign-in, forgotten-password and reset cards (`LoginLive`,
//! `ForgotPasswordLive`, `ResetPasswordLive`) as typed templates. The host
//! owns the routes, the validation and the credential services; these models
//! own the DOM, the ids and the copy, so a field error lands on the element
//! Bilimbi puts it on. Field values are the submitted text kept for another
//! attempt; a password is never echoed back.
use crate::{Attrs, Field, FlashMap, Html, InputKind, SecretInput};
use askama::Template;

/// The centered credential layout around one card body, with the page-level
/// flash the layout renders itself because the workspace shell is absent.
#[derive(Template)]
#[template(path = "auth.html")]
pub struct AuthLayout<'a> {
    pub body: Html,
    pub flash: FlashMap<'a>,
}

/// The one reason sign-in refuses, worded as Bilimbi does and never saying
/// which part was wrong.
pub const CREDENTIALS_REJECTED: &str = "These credentials do not match our records.";
/// What the sign-in page says when an expired session bounced the visitor here.
pub const SESSION_EXPIRED: &str = "Your session expired. Sign in again to continue.";
/// The forgotten-password confirmation, the same whether the account exists.
pub const RESET_REQUESTED: &str = "A reset link will be sent if the account exists.";
/// The reset page's answer to an unknown, used or expired link.
pub const RESET_LINK_INVALID: &str = "The password reset link is invalid or has expired.";
/// The sign-in page's flash after a completed reset.
pub const PASSWORD_RESET: &str = "Your password has been reset.";

/// What the sign-in card shows for one request.
#[derive(Clone, Debug, Default)]
pub struct LoginForm {
    pub tenant: String,
    pub email: String,
    pub tenant_errors: Vec<String>,
    pub email_errors: Vec<String>,
    pub password_errors: Vec<String>,
    /// The announced failure above the form; the email field repeats it.
    pub form_error: String,
    pub session_expired: bool,
}

#[derive(Template)]
#[template(path = "login.html")]
pub struct LoginPage {
    pub session_expired: bool,
    pub form_error: String,
    pub tenant: Html,
    pub email: Html,
    pub password: Html,
}

impl LoginPage {
    pub fn new(form: &LoginForm) -> askama::Result<Self> {
        Ok(Self {
            session_expired: form.session_expired,
            form_error: form.form_error.clone(),
            tenant: Html::render(&tenant_field("login", &form.tenant, &form.tenant_errors))?,
            email: Html::render(
                &Field::in_form("login", "email")
                    .id("login-email")
                    .kind(InputKind::Email)
                    .label("Email address")
                    .value(&form.email)
                    .errors(form.email_errors.clone())
                    .attrs(Attrs {
                        placeholder: Some("email@example.com".into()),
                        autocomplete: Some("email".into()),
                        required: true,
                        autofocus: true,
                        ..Attrs::default()
                    }),
            )?,
            password: Html::render(
                &Field::in_form("login", "password")
                    .id("login-password")
                    .kind(InputKind::Password)
                    .label("Password")
                    .errors(form.password_errors.clone())
                    .placeholder("Password")
                    .autocomplete("current-password")
                    .required(),
            )?,
        })
    }
}

/// What the forgotten-password card shows for one request.
#[derive(Clone, Debug, Default)]
pub struct ForgotForm {
    pub tenant: String,
    pub email: String,
    pub tenant_errors: Vec<String>,
    pub email_errors: Vec<String>,
    /// Set once a well-formed request was accepted.
    pub confirmation: String,
}

#[derive(Template)]
#[template(path = "forgot_password.html")]
pub struct ForgotPasswordPage {
    pub confirmation: String,
    pub tenant: Html,
    pub email: Html,
}

impl ForgotPasswordPage {
    pub fn new(form: &ForgotForm) -> askama::Result<Self> {
        Ok(Self {
            confirmation: form.confirmation.clone(),
            tenant: Html::render(&tenant_field("forgot", &form.tenant, &form.tenant_errors))?,
            email: Html::render(
                &Field::in_form("forgot", "email")
                    .id("forgot-email")
                    .kind(InputKind::Email)
                    .label("Email address")
                    .value(&form.email)
                    .errors(form.email_errors.clone())
                    .attrs(Attrs {
                        placeholder: Some("email@example.com".into()),
                        autocomplete: Some("email".into()),
                        required: true,
                        autofocus: true,
                        ..Attrs::default()
                    }),
            )?,
        })
    }
}

/// What the reset card shows for one request. The tenant and email come from
/// the emailed link; the passwords are never echoed back.
#[derive(Clone, Debug, Default)]
pub struct ResetForm {
    pub tenant: String,
    pub email: String,
    pub email_errors: Vec<String>,
    pub password_errors: Vec<String>,
    pub confirmation_errors: Vec<String>,
}

#[derive(Template)]
#[template(path = "reset_password.html")]
pub struct ResetPasswordPage {
    pub tenant: Html,
    pub email: Html,
    pub password: Html,
    pub password_confirmation: Html,
}

impl ResetPasswordPage {
    pub fn new(form: &ResetForm) -> askama::Result<Self> {
        Ok(Self {
            tenant: Html::render(
                &Field::in_form("reset", "tenant")
                    .id("reset-tenant")
                    .kind(InputKind::Hidden)
                    .value(&form.tenant),
            )?,
            email: Html::render(
                &Field::in_form("reset", "email")
                    .id("reset-email")
                    .kind(InputKind::Email)
                    .label("Email address")
                    .value(&form.email)
                    .errors(form.email_errors.clone())
                    .attrs(Attrs {
                        autocomplete: Some("email".into()),
                        required: true,
                        autofocus: true,
                        ..Attrs::default()
                    }),
            )?,
            password: Html::render(
                &SecretInput::in_form("reset", "password", "password")
                    .id("reset-password")
                    .label("New password")
                    .errors(form.password_errors.clone())
                    .attrs(new_password()),
            )?,
            password_confirmation: Html::render(
                &SecretInput::in_form("reset", "password_confirmation", "password")
                    .id("reset-password-confirmation")
                    .label("Confirm new password")
                    .errors(form.confirmation_errors.clone())
                    .attrs(new_password()),
            )?,
        })
    }
}

/// A required `new-password` secret, as both reset fields are.
fn new_password() -> Attrs {
    Attrs {
        autocomplete: Some("new-password".into()),
        required: true,
        ..Attrs::default()
    }
}

/// Bilibi's temporary credential lookup is tenant + email, so the anonymous
/// forms carry the tenant that Bilimbi's global login does not need.
fn tenant_field(form: &str, value: &str, errors: &[String]) -> Field {
    Field::in_form(form, "tenant")
        .id(format!("{form}-tenant"))
        .kind(InputKind::Number)
        .label("Tenant ID")
        .value(value)
        .errors(errors.to_vec())
        .attrs(Attrs {
            min: Some("1".into()),
            autocomplete: Some("off".into()),
            required: true,
            ..Attrs::default()
        })
}
