//! The signed-in account's self-service pages (Bilimbi `Core.User.Web`): the
//! settings shell in `templates/settings.html` and `/settings/password`
//! (`PasswordLive`). The page is live: the form submits as the
//! `change-password` event with its field values, the state keeps the field
//! errors of the last attempt, and a success or failure shows in the page's
//! flash group until dismissed. No capability applies: the session proves
//! who, and the current password proves the holder inside the service.
use crate::{password::check_policy, CredentialError, UserService};
use askama::Template;
use bilibi_database::Error;
use bilibi_security::Scope;
use bilibi_ui::{
    page::{
        LiveBody, LiveFuture, LiveInput, LiveRequest, LiveState, MountFuture, Page, PageBody,
        PageFuture, PageRequest,
    },
    Attrs, Button, ButtonVariant, FlashGroup, FlashMap, Header, Html, SecretInput,
};
use std::sync::Arc;

const BLANK: &str = "can't be blank";
const INCORRECT: &str = "is incorrect";
const CONFIRMATION_MISMATCH: &str = "does not match password";
const UPDATED: &str = "Password updated successfully.";
const NOT_UPDATED: &str = "Could not update password.";

/// One entry of the settings nav; an empty `href` marks a page not ported yet.
pub struct SettingsEntry {
    pub key: &'static str,
    pub label: &'static str,
    pub href: &'static str,
}

/// The settings sections in Bilimbi's order. Profile (K16) and appearance
/// (C5) are later slices.
pub fn settings_entries() -> Vec<SettingsEntry> {
    [
        ("profile", "Profile", ""),
        ("password", "Password", "/settings/password"),
        ("appearance", "Appearance", ""),
        (
            "agent-connections",
            "Agent connections",
            "/settings/agent-connections",
        ),
    ]
    .into_iter()
    .map(|(key, label, href)| SettingsEntry { key, label, href })
    .collect()
}

/// The field errors of the last attempt; values are never echoed back.
#[derive(Clone, Debug, Default)]
struct PasswordForm {
    current_errors: Vec<String>,
    password_errors: Vec<String>,
    confirmation_errors: Vec<String>,
}

#[derive(Template)]
#[template(path = "password.html")]
struct PasswordView {
    flash: Html,
    header: Html,
    entries: Vec<SettingsEntry>,
    current: String,
    heading: &'static str,
    subheading: &'static str,
    current_password: Html,
    password: Html,
    password_confirmation: Html,
    save: Html,
}

impl PasswordView {
    fn render_page(form: &PasswordForm, flash: FlashMap<'_>) -> Result<String, Error> {
        let secret = |key: &str, label: &str, autocomplete: &str, errors: &[String]| {
            Html::render(
                &SecretInput::in_form("password_change", key, "password")
                    .label(label)
                    .errors(errors.to_vec())
                    .attrs(Attrs {
                        autocomplete: Some(autocomplete.into()),
                        ..Attrs::default()
                    }),
            )
            .map_err(|_| Error::Internal)
        };
        let fragment = |rendered: askama::Result<Html>| rendered.map_err(|_| Error::Internal);
        let view = PasswordView {
            flash: fragment(Html::render(&FlashGroup {
                id: "flash-group",
                flash,
            }))?,
            header: fragment(Html::render(&Header {
                title: Html::text("Settings"),
                subtitle: "Manage your profile and account settings",
                title_actions: Html::EMPTY,
                actions: Html::EMPTY,
            }))?,
            entries: settings_entries(),
            current: "password".into(),
            heading: "Update Password",
            subheading: "Ensure your account is using a long, random password to stay secure.",
            current_password: secret(
                "current_password",
                "Current password",
                "current-password",
                &form.current_errors,
            )?,
            password: secret(
                "password",
                "New password",
                "new-password",
                &form.password_errors,
            )?,
            password_confirmation: secret(
                "password_confirmation",
                "Confirm password",
                "new-password",
                &form.confirmation_errors,
            )?,
            save: fragment(Html::render(&Button {
                variant: ButtonVariant::Primary,
                id: "password-save",
                button_type: "submit",
                body: Html::text("Save"),
                ..Button::default()
            }))?,
        };
        view.render().map_err(|_| Error::Internal)
    }
}

/// HTTP page ownership stays with User; the host knows only `PageService`.
pub struct Pages {
    pub users: Arc<UserService>,
}

impl Page<Scope, Error> for Pages {
    fn render<'a>(&'a self, request: PageRequest<'a, Scope>) -> PageFuture<'a, Error> {
        Box::pin(async move {
            request.scope.ok_or(Error::Unauthorized)?;
            if request.operation != "password" || !request.query.is_empty() {
                return Err(Error::NotFound);
            }
            Ok(PageBody {
                html: PasswordView::render_page(&PasswordForm::default(), FlashMap::default())?,
                live: true,
            })
        })
    }
    fn mount<'a>(&'a self, request: PageRequest<'a, Scope>) -> MountFuture<'a, Scope, Error> {
        Box::pin(async move {
            if request.operation != "password" || !request.query.is_empty() {
                return Err(Error::NotFound);
            }
            let state: Box<dyn LiveState<Scope, Error>> = Box::new(PasswordPage {
                users: self.users.clone(),
                form: PasswordForm::default(),
                success: false,
                failure: false,
            });
            Ok(Some(state))
        })
    }
}

struct PasswordPage {
    users: Arc<UserService>,
    form: PasswordForm,
    success: bool,
    failure: bool,
}

#[derive(serde::Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
enum PasswordEvent {
    ChangePassword {
        #[serde(rename = "password_change[current_password]", default)]
        current_password: String,
        #[serde(rename = "password_change[password]", default)]
        password: String,
        #[serde(rename = "password_change[password_confirmation]", default)]
        password_confirmation: String,
    },
    ClearFlash {
        key: String,
    },
}

impl PasswordPage {
    async fn change(
        &mut self,
        request: &LiveRequest<'_, Scope>,
        current: &str,
        password: &str,
        confirmation: &str,
    ) -> Result<(), Error> {
        let mut form = PasswordForm::default();
        if current.is_empty() {
            form.current_errors.push(BLANK.into());
        }
        if password.is_empty() {
            form.password_errors.push(BLANK.into());
        } else if let Err(violation) = check_policy(password) {
            form.password_errors.push(violation.to_string());
        }
        if confirmation.is_empty() {
            form.confirmation_errors.push(BLANK.into());
        } else if confirmation != password {
            form.confirmation_errors.push(CONFIRMATION_MISMATCH.into());
        }
        self.success = false;
        self.failure = false;
        if !(form.current_errors.is_empty()
            && form.password_errors.is_empty()
            && form.confirmation_errors.is_empty())
        {
            self.form = form;
            return Ok(());
        }
        match self
            .users
            .change_password(request.credential, &request.scope.csrf, current, password)
            .await
        {
            Ok(()) => self.success = true,
            Err(CredentialError::IncorrectPassword) => form.current_errors.push(INCORRECT.into()),
            Err(CredentialError::Policy(violation)) => {
                form.password_errors.push(violation.to_string())
            }
            // The session itself is what the host must re-check; anything
            // infrastructural is fatal to the page, as everywhere else.
            Err(CredentialError::Other(
                error @ (Error::Unauthorized
                | Error::Forbidden
                | Error::Database(_)
                | Error::Pool(_)
                | Error::Internal),
            )) => return Err(error),
            Err(_) => self.failure = true,
        }
        self.form = form;
        Ok(())
    }
}

impl LiveState<Scope, Error> for PasswordPage {
    fn update<'a>(&'a mut self, request: LiveRequest<'a, Scope>) -> LiveFuture<'a, Error> {
        Box::pin(async move {
            match request.input {
                LiveInput::Refresh => {}
                LiveInput::Params(_) => return Err(Error::Invalid),
                LiveInput::Event(value) => {
                    match serde_json::from_value::<PasswordEvent>(value.clone())
                        .map_err(|_| Error::Invalid)?
                    {
                        PasswordEvent::ChangePassword {
                            current_password,
                            password,
                            password_confirmation,
                        } => {
                            self.change(
                                &request,
                                &current_password,
                                &password,
                                &password_confirmation,
                            )
                            .await?
                        }
                        PasswordEvent::ClearFlash { key } => match key.as_str() {
                            "success" => self.success = false,
                            "error" => self.failure = false,
                            _ => {}
                        },
                    }
                }
            }
            let flash = FlashMap {
                success: if self.success { UPDATED } else { "" },
                error: if self.failure { NOT_UPDATED } else { "" },
                ..FlashMap::default()
            };
            Ok(LiveBody {
                html: PasswordView::render_page(&self.form, flash)?,
                rows: 1,
                ..LiveBody::default()
            })
        })
    }
}

#[derive(Template)]
#[template(path = "bell.html")]
struct Bell {
    unread: i64,
    icon: Html,
}

/// The shell's `shell.notifications` slot for an account that may view
/// notifications; the host renders nothing when `unread` refuses.
pub fn bell(unread: i64) -> askama::Result<Html> {
    Html::render(&Bell {
        unread,
        icon: Html::render(&bilibi_ui::Icon::new("notify"))?,
    })
}
